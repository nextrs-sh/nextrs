//! An optional limit on ArrayBuffer memory (RE-4D): the isolates'
//! `ArrayBuffer::Allocator` counts every backing store V8 allocates through
//! it (`new ArrayBuffer`, typed arrays, `Buffer.alloc`, `Buffer.concat`,
//! `ArrayBuffer.prototype.slice`, structured clones) against one budget per
//! executor, and refuses an allocation that would cross it.
//!
//! * **Refusal is a JavaScript error, not a crash, for the allocations it
//!   applies to.** V8 answers a refused allocation by collecting garbage
//!   and retrying (`Heap::AllocateExternalBackingStore`, which frees dead
//!   buffers first), then throws `RangeError: Array buffer allocation
//!   failed`. The request that asked fails; the isolate and the process
//!   keep serving.
//! * **Small allocations are never refused.** V8 aborts the process when
//!   it cannot materialize the buffer of an on-heap typed array
//!   (`JSTypedArray::GetBuffer`, at most 64 bytes, `V8_TYPED_ARRAY_MAX_SIZE_IN_HEAP`),
//!   and when an embedder API allocation with the default failure mode
//!   fails (`ArrayBuffer::New`/`NewBackingStore` from Rust). Below
//!   [`REFUSABLE_MIN`] allocations are counted but always granted, which
//!   covers the first case by a factor of 1000. NextRS's own ops hand
//!   buffers to V8 from Rust memory (`from_vec`), not through this
//!   allocator; the dependencies' embedder calls that do go through it are
//!   not on NextRS's request path (RE-4-EXTERNAL-MEMORY-ANALYSIS).
//! * **What it does not cover.** Buffers adopted from Rust (request-body
//!   and fetch chunks, `fs.readFileSync`, op results) are counted by V8 as
//!   external memory but not here; they are bounded, where they are, by
//!   their op's own limits. Native memory outside V8 is not covered at all.
//!
//! The allocator uses glibc's `calloc`/`malloc`/`free`, as V8's default
//! allocator does; only the accounting is added. Not usable with the V8
//! sandbox, which the pinned build does not enable.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use deno_core::v8;

/// Allocations at least this large may be refused; smaller ones are
/// always granted (and counted).
pub const REFUSABLE_MIN: usize = 64 * 1024;

thread_local! {
    /// When this thread's allocator last refused an allocation.
    static LAST_REFUSAL: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// How long after a refusal the near-heap-limit callback treats a call as
/// V8's last-resort collection for that refusal (see `refused_recently`).
const REFUSAL_WINDOW: Duration = Duration::from_secs(1);

/// Whether this thread's allocator refused an allocation within the last
/// second. V8 answers a refused allocation with a last-resort collection
/// that also calls the near-heap-limit callback
/// (`Heap::CollectAllAvailableGarbage(kLastResort)`), whatever the heap's
/// size; the callback uses this, together with the heap's size, to tell
/// that call from a heap that has really reached its limit.
pub(crate) fn refused_recently() -> bool {
    LAST_REFUSAL.with(|at| at.get().is_some_and(|t| t.elapsed() < REFUSAL_WINDOW))
}

/// The budget shared by an executor's isolates.
#[derive(Debug)]
pub(crate) struct ExternalBudget {
    limit: usize,
    used: AtomicUsize,
    peak: AtomicUsize,
    refused: AtomicU64,
    refused_bytes: AtomicU64,
}

impl ExternalBudget {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            used: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            refused: AtomicU64::new(0),
            refused_bytes: AtomicU64::new(0),
        })
    }

    /// Counts `len` bytes; false (nothing counted) if they may be refused
    /// and would cross the limit.
    fn reserve(&self, len: usize) -> bool {
        let refusable = len >= REFUSABLE_MIN;
        let mut used = self.used.load(Ordering::Relaxed);
        loop {
            let next = used.saturating_add(len);
            if refusable && next > self.limit {
                LAST_REFUSAL.with(|at| at.set(Some(Instant::now())));
                self.refused.fetch_add(1, Ordering::Relaxed);
                self.refused_bytes.fetch_add(len as u64, Ordering::Relaxed);
                return false;
            }
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Relaxed)
            {
                Ok(_) => {
                    self.peak.fetch_max(next, Ordering::Relaxed);
                    return true;
                }
                Err(current) => used = current,
            }
        }
    }

    fn release(&self, len: usize) {
        self.used.fetch_sub(len, Ordering::AcqRel);
    }

    pub fn snapshot(&self) -> ExternalSnapshot {
        ExternalSnapshot {
            limit_bytes: self.limit as u64,
            used_bytes: self.used.load(Ordering::Relaxed) as u64,
            peak_bytes: self.peak.load(Ordering::Relaxed) as u64,
            refused: self.refused.load(Ordering::Relaxed),
            refused_bytes: self.refused_bytes.load(Ordering::Relaxed),
        }
    }

    /// A V8 allocator drawing on this budget. Each holds one reference to
    /// the budget, given back when V8 destroys the allocator (after the
    /// last backing store it made is freed).
    pub fn allocator(self: &Arc<Self>) -> v8::UniqueRef<v8::Allocator> {
        let handle = Arc::into_raw(self.clone());
        // SAFETY: `handle` is a valid `ExternalBudget` until `drop_budget`
        // releases it, which V8 calls once, when the allocator is
        // destroyed. The vtable's functions are thread-safe (atomics and
        // libc's allocator) and never call into V8.
        unsafe { v8::new_rust_allocator(handle, &VTABLE) }
    }
}

/// The external-memory budget, for metrics.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ExternalSnapshot {
    pub limit_bytes: u64,
    /// Backing-store bytes allocated through the budgeted allocator, live.
    pub used_bytes: u64,
    pub peak_bytes: u64,
    /// Allocations refused (each a RangeError in JavaScript).
    pub refused: u64,
    pub refused_bytes: u64,
}

static VTABLE: v8::RustAllocatorVtable<ExternalBudget> = v8::RustAllocatorVtable {
    allocate,
    allocate_uninitialized,
    free,
    drop: drop_budget,
};

unsafe extern "C" fn allocate(budget: &ExternalBudget, len: usize) -> *mut c_void {
    if !budget.reserve(len) {
        return std::ptr::null_mut();
    }
    // SAFETY: calloc with a non-zero size; a zero-length request gets one
    // byte so that the pointer is unique and freeable.
    let data = unsafe { libc::calloc(1, len.max(1)) };
    if data.is_null() {
        budget.release(len);
    }
    data
}

unsafe extern "C" fn allocate_uninitialized(budget: &ExternalBudget, len: usize) -> *mut c_void {
    if !budget.reserve(len) {
        return std::ptr::null_mut();
    }
    // SAFETY: as above, uninitialized.
    let data = unsafe { libc::malloc(len.max(1)) };
    if data.is_null() {
        budget.release(len);
    }
    data
}

unsafe extern "C" fn free(budget: &ExternalBudget, data: *mut c_void, len: usize) {
    // SAFETY: V8 frees only what this allocator returned, with the length
    // it was allocated with.
    unsafe { libc::free(data) };
    budget.release(len);
}

unsafe extern "C" fn drop_budget(budget: *const ExternalBudget) {
    // SAFETY: from `Arc::into_raw` in `allocator`; V8 calls this once.
    drop(unsafe { Arc::from_raw(budget) });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_allocations_are_refused_past_the_limit_small_ones_never() {
        let budget = ExternalBudget::new(1 << 20);
        assert!(budget.reserve(768 * 1024));
        assert!(!budget.reserve(512 * 1024), "would cross the limit");
        assert_eq!(budget.snapshot().refused, 1);
        assert_eq!(budget.snapshot().used_bytes, 768 * 1024);
        // Under the refusable size: granted even past the limit.
        for _ in 0..10 {
            assert!(budget.reserve(REFUSABLE_MIN - 1));
        }
        assert!(budget.snapshot().used_bytes > 1 << 20);
        budget.release(768 * 1024);
        assert!(budget.reserve(256 * 1024));
        assert_eq!(
            budget.snapshot().peak_bytes,
            768 * 1024 + 10 * (REFUSABLE_MIN as u64 - 1)
        );
    }

    #[test]
    fn concurrent_reservations_never_cross_the_limit() {
        let budget = ExternalBudget::new(64 * REFUSABLE_MIN);
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let budget = budget.clone();
                std::thread::spawn(move || {
                    let mut granted = 0;
                    for _ in 0..1000 {
                        if budget.reserve(REFUSABLE_MIN) {
                            granted += 1;
                            assert!(budget.used.load(Ordering::Relaxed) <= 64 * REFUSABLE_MIN);
                            budget.release(REFUSABLE_MIN);
                        }
                    }
                    granted
                })
            })
            .collect();
        let granted: u32 = threads.into_iter().map(|t| t.join().unwrap()).sum();
        assert!(granted > 0);
        assert_eq!(
            budget.snapshot().used_bytes,
            0,
            "every reservation released"
        );
        assert!(budget.snapshot().peak_bytes <= 64 * REFUSABLE_MIN as u64);
    }
}
