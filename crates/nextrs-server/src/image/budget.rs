//! Memory admission for image work: what a transformation will allocate
//! (estimated from the source's header, `nextrs_image::estimate`) is
//! reserved from a process-wide budget before it starts, beside the
//! concurrency limit; source bodies held in memory (downloading, waiting,
//! being transformed) count against a budget of their own.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Reservations are counted in KiB (a semaphore's permits are u32).
const UNIT: u64 = 1024;

/// Bytes transformations may reserve at once. A reservation is held until
/// the work ends on its thread, also after its request timed out.
pub(crate) struct MemoryBudget {
    permits: Arc<Semaphore>,
    units: u64,
}

impl MemoryBudget {
    pub fn new(bytes: u64) -> Self {
        let units = (bytes / UNIT).clamp(1, u64::from(u32::MAX));
        Self {
            permits: Arc::new(Semaphore::new(units as usize)),
            units,
        }
    }

    fn units(bytes: u64) -> u32 {
        bytes.div_ceil(UNIT).clamp(1, u64::from(u32::MAX)) as u32
    }

    /// Whether `bytes` can ever be reserved (else the work is refused).
    pub fn fits(&self, bytes: u64) -> bool {
        u64::from(Self::units(bytes)) <= self.units
    }

    pub fn try_reserve(&self, bytes: u64) -> Option<OwnedSemaphorePermit> {
        self.permits
            .clone()
            .try_acquire_many_owned(Self::units(bytes))
            .ok()
    }

    /// Waits (first come, first served: a large reservation is not
    /// overtaken by smaller ones) until `bytes` are free.
    pub async fn reserve(&self, bytes: u64) -> Option<OwnedSemaphorePermit> {
        self.permits
            .clone()
            .acquire_many_owned(Self::units(bytes))
            .await
            .ok()
    }

    pub fn total_bytes(&self) -> u64 {
        self.units * UNIT
    }

    pub fn reserved_bytes(&self) -> u64 {
        (self.units - self.permits.available_permits() as u64) * UNIT
    }
}

/// Source bytes held in memory at once; beyond it, new image work answers
/// 503 (as a full queue does) instead of buffering more.
pub(crate) struct SourceBudget {
    held: AtomicU64,
    max: u64,
    /// Sources refused because the budget was full.
    pub rejected: AtomicU64,
}

impl SourceBudget {
    pub fn new(max: u64) -> Arc<Self> {
        Arc::new(Self {
            held: AtomicU64::new(0),
            max,
            rejected: AtomicU64::new(0),
        })
    }

    pub fn held_bytes(&self) -> u64 {
        self.held.load(Ordering::Relaxed)
    }

    pub fn max_bytes(&self) -> u64 {
        self.max
    }
}

/// The bytes one source holds, returned when it is dropped.
pub(crate) struct SourceLease {
    budget: Arc<SourceBudget>,
    bytes: u64,
}

impl std::fmt::Debug for SourceLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SourceLease({} bytes)", self.bytes)
    }
}

impl SourceLease {
    pub fn new(budget: &Arc<SourceBudget>) -> Self {
        Self {
            budget: budget.clone(),
            bytes: 0,
        }
    }

    /// Adds `n` bytes; `false` (nothing added) when the budget would be
    /// exceeded.
    pub fn grow(&mut self, n: u64) -> bool {
        let budget = &self.budget;
        let mut held = budget.held.load(Ordering::Relaxed);
        loop {
            if held + n > budget.max {
                budget.rejected.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            match budget.held.compare_exchange_weak(
                held,
                held + n,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.bytes += n;
                    return true;
                }
                Err(current) => held = current,
            }
        }
    }
}

impl Drop for SourceLease {
    fn drop(&mut self) {
        #[cfg(feature = "negative-controls")]
        if super::controls::leak_source() {
            return;
        }
        self.budget.held.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reservations_return_on_drop() {
        let budget = MemoryBudget::new(10 * UNIT);
        assert!(budget.fits(10 * UNIT) && !budget.fits(10 * UNIT + 1));
        let a = budget.try_reserve(6 * UNIT).unwrap();
        assert!(budget.try_reserve(5 * UNIT).is_none());
        assert_eq!(budget.reserved_bytes(), 6 * UNIT);
        let waiting = tokio::spawn({
            let permits = budget.permits.clone();
            async move { permits.acquire_many_owned(5).await.map(drop) }
        });
        tokio::task::yield_now().await;
        drop(a);
        waiting.await.unwrap().unwrap();
        assert_eq!(budget.reserved_bytes(), 0);
    }

    #[test]
    fn source_leases_are_bounded_and_returned() {
        let budget = SourceBudget::new(100);
        let mut a = SourceLease::new(&budget);
        assert!(a.grow(60));
        let mut b = SourceLease::new(&budget);
        assert!(!b.grow(41));
        assert!(b.grow(40));
        assert_eq!(budget.held_bytes(), 100);
        drop(a);
        assert_eq!(budget.held_bytes(), 40);
        drop(b);
        assert_eq!(budget.held_bytes(), 0);
    }
}
