//! Executor counters and gauges. Read them with [`crate::Executor::metrics`];
//! [`MetricsSnapshot::to_prometheus`] renders the Prometheus text format, so
//! no metrics service or client library is needed.
//!
//! Labels are limited to the executor slot index (bounded by the pool size)
//! and fixed reason names. Nothing request-derived (paths, headers, cookies)
//! is ever recorded.

use std::fmt::Write;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

#[derive(Default)]
pub(crate) struct Counters {
    pub requests_admitted: AtomicU64,
    pub requests_completed: AtomicU64,
    pub rejected_overloaded: AtomicU64,
    pub rejected_queue_timeout: AtomicU64,
    pub rejected_shutting_down: AtomicU64,
    pub rejected_unavailable: AtomicU64,
    /// Memory-aware admission: refusals by request class
    /// (`RequestClass::index`), requests that had to wait for memory,
    /// admissions made only because nothing else ran, and isolates
    /// skipped for being near their heap limit.
    pub rejected_memory_pressure: [AtomicU64; 4],
    pub admission_deferred: AtomicU64,
    pub admission_progress: AtomicU64,
    pub admission_heap_skips: AtomicU64,
    /// The consumer dropped a delivered response before `End`/`Error`:
    /// client disconnects and server-side cancellation.
    pub responses_cancelled: AtomicU64,
    /// Writes that had to wait for the consumer to drain.
    pub backpressure_waits: AtomicU64,
    /// Response events (head, body chunks, end) the HTTP side received.
    pub response_events: AtomicU64,
    /// Of those, events the HTTP side had to wait for: nothing was queued
    /// when it asked, so its task parked until the isolate sent one (each
    /// such wait is a cross-thread wake-up when the worker is idle).
    pub response_event_waits: AtomicU64,
    pub request_timeouts: AtomicU64,
    pub wait_until_timeouts: AtomicU64,
    pub handler_errors: AtomicU64,
    pub request_body_errors: AtomicU64,
    pub queue_wait_micros: AtomicU64,
    pub queue_waits: AtomicU64,
    pub execution_micros: AtomicU64,
    /// Memory reclamations requested from the isolates, by level
    /// (`ReclaimLevel::index`), the isolates' GC time spent on them, and
    /// the V8 heap memory they gave back.
    pub memory_reclaims: [AtomicU64; 2],
    pub memory_reclaim_micros: AtomicU64,
    pub memory_reclaim_heap_released: AtomicU64,
    /// Outbound fetches started (including ones the policy refused).
    pub fetch_requests: AtomicU64,
    /// Fetches currently held by an isolate (not yet closed).
    pub fetch_in_flight: AtomicU64,
    pub fetch_failures: [AtomicU64; FETCH_REASONS.len()],
    /// Time to the response head, as a histogram (`FETCH_BUCKETS`).
    pub fetch_duration_buckets: [AtomicU64; FETCH_BUCKETS.len()],
    pub fetch_duration_micros: AtomicU64,
    pub fetch_duration_count: AtomicU64,
    /// Requests forwarded for external rewrites (including refused ones).
    pub upstream_requests: AtomicU64,
    pub upstream_in_flight: AtomicU64,
    pub upstream_failures: [AtomicU64; FETCH_REASONS.len()],
    /// Lookups that found an entry in the store (Next may still judge it
    /// expired by its tags) and lookups that found none.
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    /// Entry value bytes handed to JavaScript by lookups (each one a copy
    /// JavaScript then parses).
    pub cache_value_bytes: AtomicU64,
    /// Lookups answered `unchanged`: the isolate's decoded copy of the
    /// entry was current, so no value was copied or parsed.
    pub cache_reused: AtomicU64,
    pub cache_sets: AtomicU64,
    pub cache_evictions: AtomicU64,
    pub cache_revalidations: AtomicU64,
    /// Entries or tag files that failed verification and were removed.
    pub cache_corrupt: AtomicU64,
    pub cache_io_errors: AtomicU64,
    /// Values over the entry size limit, not stored.
    pub cache_too_large: AtomicU64,
    /// Response-cache entries stored (ISR pages and route handlers).
    pub cache_response_sets: AtomicU64,
    /// Blocking regenerations taken over by another isolate's (waited for).
    pub cache_regen_waits: AtomicU64,
    /// Waits for another isolate's regeneration that hit the lease limit.
    pub cache_regen_wait_timeouts: AtomicU64,
}

/// Fetch failure reasons (metric label values), indexed by `FETCH_*`.
pub(crate) const FETCH_REASONS: [&str; 8] = [
    "denied",
    "invalid",
    "dns",
    "network",
    "timeout",
    "too_large",
    "redirect",
    "aborted",
];
pub(crate) const FETCH_DENIED: usize = 0;
pub(crate) const FETCH_INVALID: usize = 1;
pub(crate) const FETCH_DNS: usize = 2;
pub(crate) const FETCH_NETWORK: usize = 3;
pub(crate) const FETCH_TIMEOUT: usize = 4;
pub(crate) const FETCH_TOO_LARGE: usize = 5;
pub(crate) const FETCH_REDIRECT: usize = 6;
pub(crate) const FETCH_ABORTED: usize = 7;
/// Upper bounds (seconds) of the fetch duration histogram buckets.
pub(crate) const FETCH_BUCKETS: [f64; 12] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

pub(crate) fn add(counter: &AtomicU64, value: u64) {
    counter.fetch_add(value, Ordering::Relaxed);
}

fn get(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

/// Kinds of events JavaScript reports through `op_nextrs_count`.
pub(crate) const COUNT_REQUEST_TIMEOUT: u32 = 1;
pub(crate) const COUNT_WAIT_UNTIL_TIMEOUT: u32 = 2;
pub(crate) const COUNT_HANDLER_ERROR: u32 = 3;
pub(crate) const COUNT_REQUEST_BODY_ERROR: u32 = 4;

impl Counters {
    pub fn count(&self, kind: u32) {
        let counter = match kind {
            COUNT_REQUEST_TIMEOUT => &self.request_timeouts,
            COUNT_WAIT_UNTIL_TIMEOUT => &self.wait_until_timeouts,
            COUNT_HANDLER_ERROR => &self.handler_errors,
            COUNT_REQUEST_BODY_ERROR => &self.request_body_errors,
            _ => return,
        };
        add(counter, 1);
    }

    pub fn fetch_failed(&self, reason: usize) {
        if let Some(counter) = self.fetch_failures.get(reason) {
            add(counter, 1);
        }
    }

    pub fn upstream_failed(&self, reason: usize) {
        if let Some(counter) = self.upstream_failures.get(reason) {
            add(counter, 1);
        }
    }

    pub fn observe_fetch(&self, elapsed: std::time::Duration) {
        let seconds = elapsed.as_secs_f64();
        if let Some(bucket) = FETCH_BUCKETS.iter().position(|bound| seconds <= *bound) {
            add(&self.fetch_duration_buckets[bucket], 1);
        }
        add(&self.fetch_duration_micros, elapsed.as_micros() as u64);
        add(&self.fetch_duration_count, 1);
    }
}

/// Shared data cache counters and gauges.
#[derive(Debug, Clone, Serialize)]
pub struct CacheSnapshot {
    pub persistent: bool,
    pub entries: u64,
    pub memory_bytes: u64,
    pub storage_bytes: u64,
    pub hits: u64,
    pub misses: u64,
    pub value_bytes: u64,
    pub reused: u64,
    pub sets: u64,
    pub evictions: u64,
    pub revalidated_tags: u64,
    pub corrupt: u64,
    pub io_errors: u64,
    pub too_large: u64,
    pub response_sets: u64,
    pub regeneration_waits: u64,
    pub regeneration_wait_timeouts: u64,
}

impl CacheSnapshot {
    pub(crate) fn new(counters: &Counters, persistent: bool, gauges: (u64, u64, u64)) -> Self {
        Self {
            persistent,
            entries: gauges.0,
            memory_bytes: gauges.1,
            storage_bytes: gauges.2,
            hits: get(&counters.cache_hits),
            misses: get(&counters.cache_misses),
            value_bytes: get(&counters.cache_value_bytes),
            reused: get(&counters.cache_reused),
            sets: get(&counters.cache_sets),
            evictions: get(&counters.cache_evictions),
            revalidated_tags: get(&counters.cache_revalidations),
            corrupt: get(&counters.cache_corrupt),
            io_errors: get(&counters.cache_io_errors),
            too_large: get(&counters.cache_too_large),
            response_sets: get(&counters.cache_response_sets),
            regeneration_waits: get(&counters.cache_regen_waits),
            regeneration_wait_timeouts: get(&counters.cache_regen_wait_timeouts),
        }
    }

    fn write_prometheus(&self, out: &mut String) {
        for (name, kind, help, value) in [
            (
                "nextrs_cache_entries",
                "gauge",
                "Entries in the shared data cache.",
                self.entries,
            ),
            (
                "nextrs_cache_memory_bytes",
                "gauge",
                "Entry bytes held in memory by the shared data cache.",
                self.memory_bytes,
            ),
            (
                "nextrs_cache_storage_bytes",
                "gauge",
                "Entry file bytes on disk (0 without --cache-dir).",
                self.storage_bytes,
            ),
            (
                "nextrs_cache_hits_total",
                "counter",
                "Data cache lookups answered (fresh or stale).",
                self.hits,
            ),
            (
                "nextrs_cache_misses_total",
                "counter",
                "Data cache lookups without a usable entry.",
                self.misses,
            ),
            (
                "nextrs_cache_value_bytes_total",
                "counter",
                "Entry value bytes copied to JavaScript by cache lookups.",
                self.value_bytes,
            ),
            (
                "nextrs_cache_reused_total",
                "counter",
                "Lookups answered from the isolate's own decoded copy of an unchanged entry (no copy, no parse).",
                self.reused,
            ),
            (
                "nextrs_cache_sets_total",
                "counter",
                "Data cache entries stored.",
                self.sets,
            ),
            (
                "nextrs_cache_evictions_total",
                "counter",
                "Entries evicted for the memory or disk limit.",
                self.evictions,
            ),
            (
                "nextrs_cache_revalidated_tags_total",
                "counter",
                "Tags revalidated (revalidateTag, revalidatePath).",
                self.revalidated_tags,
            ),
            (
                "nextrs_cache_response_sets_total",
                "counter",
                "Response-cache entries stored (ISR pages and route handlers).",
                self.response_sets,
            ),
            (
                "nextrs_cache_regeneration_waits_total",
                "counter",
                "Blocking regenerations left to another isolate that was already regenerating.",
                self.regeneration_waits,
            ),
            (
                "nextrs_cache_regeneration_wait_timeouts_total",
                "counter",
                "Waits for another isolate's regeneration that reached the limit.",
                self.regeneration_wait_timeouts,
            ),
        ] {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}"
            );
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_cache_errors_total Data cache entries rejected or not written, by reason.\n# TYPE nextrs_cache_errors_total counter"
        );
        for (reason, value) in [
            ("corrupt", self.corrupt),
            ("io", self.io_errors),
            ("too_large", self.too_large),
        ] {
            let _ = writeln!(
                out,
                "nextrs_cache_errors_total{{reason=\"{reason}\"}} {value}"
            );
        }
    }
}

/// Outbound fetch counters.
#[derive(Debug, Clone, Serialize)]
pub struct FetchSnapshot {
    pub requests: u64,
    pub in_flight: u64,
    pub failures: Vec<(&'static str, u64)>,
    /// Cumulative counts per `le` bound, as Prometheus histograms.
    pub duration_buckets: Vec<(f64, u64)>,
    pub duration_seconds_sum: f64,
    pub duration_count: u64,
}

/// External-rewrite forwarding counters.
#[derive(Debug, Clone, Serialize)]
pub struct UpstreamSnapshot {
    pub requests: u64,
    pub in_flight: u64,
    pub failures: Vec<(&'static str, u64)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsSnapshot {
    pub isolates: usize,
    pub max_active_per_isolate: usize,
    pub max_queued: usize,
    /// Requests waiting for an executor slot.
    pub queued: usize,
    pub requests_admitted: u64,
    pub requests_completed: u64,
    pub rejected_overloaded: u64,
    pub rejected_queue_timeout: u64,
    pub rejected_shutting_down: u64,
    pub rejected_unavailable: u64,
    /// Refused by memory-aware admission (all classes).
    pub rejected_memory_pressure: u64,
    pub responses_cancelled: u64,
    pub backpressure_waits: u64,
    pub response_events: u64,
    pub response_event_waits: u64,
    pub request_timeouts: u64,
    pub wait_until_timeouts: u64,
    pub handler_errors: u64,
    pub request_body_errors: u64,
    pub queue_wait_seconds_total: f64,
    pub queue_waits: u64,
    pub execution_seconds_total: f64,
    /// Memory reclamations (see `Executor::reclaim_memory`): requested
    /// because the server was idle, and because the container was near its
    /// memory limit; the isolates' collection time; the committed V8 heap
    /// memory they gave back.
    pub memory_reclaims_idle: u64,
    pub memory_reclaims_pressure: u64,
    pub memory_reclaim_seconds_total: f64,
    pub memory_reclaim_heap_released_bytes: u64,
    pub fetch: FetchSnapshot,
    pub upstream: UpstreamSnapshot,
    /// `None` when the shared cache is disabled.
    pub cache: Option<CacheSnapshot>,
    /// `None` when memory-aware admission is off.
    pub admission: Option<AdmissionSnapshot>,
    /// `None` without an external-memory limit.
    pub external_memory: Option<crate::ExternalSnapshot>,
    /// Memory outside the V8 heaps (for soaks; not in the Prometheus text).
    pub memory: MemorySnapshot,
    pub executors: Vec<ExecutorSnapshot>,
}

/// What the process holds outside the V8 heaps.
#[derive(Debug, Clone, Serialize)]
pub struct MemorySnapshot {
    /// Web Crypto keys alive in all isolates (their material is in Rust).
    pub crypto_keys: u64,
    /// glibc's mallinfo2: the C heap Rust, V8's off-heap allocations and
    /// native libraries use. `None` where it is not available.
    pub malloc: Option<MallocSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MallocSnapshot {
    /// Bytes obtained with sbrk (the main arena), and with mmap for large
    /// blocks.
    pub arena_bytes: u64,
    pub mmap_bytes: u64,
    /// Bytes in allocated blocks, and free bytes the allocator keeps.
    pub in_use_bytes: u64,
    pub free_bytes: u64,
    /// Free bytes at the top of the heap that malloc_trim could release.
    pub releasable_bytes: u64,
}

impl MallocSnapshot {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    fn current() -> Option<Self> {
        // SAFETY: mallinfo2 has no preconditions; it only reads the
        // allocator's statistics (glibc >= 2.33).
        let info = unsafe { libc::mallinfo2() };
        Some(Self {
            arena_bytes: info.arena as u64,
            mmap_bytes: info.hblkhd as u64,
            in_use_bytes: info.uordblks as u64,
            free_bytes: info.fordblks as u64,
            releasable_bytes: info.keepcost as u64,
        })
    }

    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    fn current() -> Option<Self> {
        None
    }
}

/// Sets glibc's mmap threshold: allocations of at least `bytes` get pages
/// of their own, returned to the system when freed. glibc otherwise raises
/// the threshold (up to 32 MiB) after each large free, so the multi-MiB
/// pixel and source buffers of image work land in per-thread arenas whose
/// free memory stays resident (measured: RSS 655 MiB after a burst whose
/// buffers were all freed, 76 MiB with a 1 MiB threshold). Not applied
/// when the operator set the threshold already (`MALLOC_MMAP_THRESHOLD_`,
/// `GLIBC_TUNABLES`). Returns whether it was applied.
pub fn set_malloc_mmap_threshold(bytes: usize) -> bool {
    if std::env::var_os("MALLOC_MMAP_THRESHOLD_").is_some()
        || std::env::var("GLIBC_TUNABLES").is_ok_and(|t| t.contains("mmap_threshold"))
    {
        return false;
    }
    set_threshold(bytes)
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn set_threshold(bytes: usize) -> bool {
    let Ok(value) = libc::c_int::try_from(bytes) else {
        return false;
    };
    // SAFETY: mallopt only changes allocator parameters; M_MMAP_THRESHOLD
    // takes any value up to HEAP_MAX_SIZE / 2 and fails (returns 0) above.
    unsafe { libc::mallopt(libc::M_MMAP_THRESHOLD, value) == 1 }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn set_threshold(_bytes: usize) -> bool {
    false
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutorSnapshot {
    pub slot: usize,
    /// Incremented on every (re)start of the slot's isolate.
    pub generation: u64,
    pub state: &'static str,
    pub active: usize,
    /// V8 heap in use, heap reserved, and memory held outside the heap by
    /// JavaScript objects (ArrayBuffers), sampled at most once a second
    /// while the isolate runs. 0 while the slot restarts.
    pub heap_used_bytes: usize,
    pub heap_total_bytes: usize,
    pub external_bytes: usize,
    /// The heap limit V8 enforces for this isolate (`--js-heap-limit`, or
    /// V8's default for the memory it was sized from). 0 before sampling.
    pub heap_limit_bytes: usize,
    /// Heap in use right after the latest full (mark-sweep) GC of the
    /// current isolate: live data, without the garbage `heap_used_bytes`
    /// includes between collections. 0 before the first full GC.
    pub heap_used_after_gc_bytes: usize,
    /// Full GCs of the current isolate.
    pub full_gcs: u64,
    pub restarts: u64,
    /// Failures by reason, over the life of the pool.
    pub failures: Vec<(&'static str, u64)>,
    pub last_failure: Option<String>,
    /// V8 near-heap-limit callback calls over the life of the slot, and
    /// those declined (last-resort collections after a refused ArrayBuffer
    /// allocation, with the heap not full).
    pub near_heap_limit_calls: u64,
    pub near_heap_limit_declined: u64,
    /// The latest heap-limit termination of this slot, if any.
    pub last_heap_limit: Option<HeapLimitEpisode>,
}

/// One heap-limit termination (RE-4E): how often V8 called the callback
/// for the terminated isolate, the limits it passed, and how long the
/// termination took to unwind and the slot to be served again.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct HeapLimitEpisode {
    pub generation: u64,
    pub callback_calls: u64,
    /// Old-generation limit V8 passed at the first and the latest call:
    /// each granted call raised it by the headroom.
    pub first_limit_bytes: usize,
    pub last_limit_bytes: usize,
    /// From the termination request to the isolate thread's exit.
    pub unwind_ms: u64,
    /// From the termination request to a replacement isolate being ready
    /// (0 until it is).
    pub replacement_ms: u64,
}

impl MetricsSnapshot {
    pub(crate) fn from_counters(counters: &Counters) -> Self {
        Self {
            isolates: 0,
            max_active_per_isolate: 0,
            max_queued: 0,
            queued: 0,
            requests_admitted: get(&counters.requests_admitted),
            requests_completed: get(&counters.requests_completed),
            rejected_overloaded: get(&counters.rejected_overloaded),
            rejected_queue_timeout: get(&counters.rejected_queue_timeout),
            rejected_shutting_down: get(&counters.rejected_shutting_down),
            rejected_unavailable: get(&counters.rejected_unavailable),
            rejected_memory_pressure: counters.rejected_memory_pressure.iter().map(get).sum(),
            responses_cancelled: get(&counters.responses_cancelled),
            backpressure_waits: get(&counters.backpressure_waits),
            response_events: get(&counters.response_events),
            response_event_waits: get(&counters.response_event_waits),
            request_timeouts: get(&counters.request_timeouts),
            wait_until_timeouts: get(&counters.wait_until_timeouts),
            handler_errors: get(&counters.handler_errors),
            request_body_errors: get(&counters.request_body_errors),
            queue_wait_seconds_total: get(&counters.queue_wait_micros) as f64 / 1e6,
            queue_waits: get(&counters.queue_waits),
            execution_seconds_total: get(&counters.execution_micros) as f64 / 1e6,
            memory_reclaims_idle: get(&counters.memory_reclaims[0]),
            memory_reclaims_pressure: get(&counters.memory_reclaims[1]),
            memory_reclaim_seconds_total: get(&counters.memory_reclaim_micros) as f64 / 1e6,
            memory_reclaim_heap_released_bytes: get(&counters.memory_reclaim_heap_released),
            fetch: FetchSnapshot {
                requests: get(&counters.fetch_requests),
                in_flight: get(&counters.fetch_in_flight),
                failures: FETCH_REASONS
                    .iter()
                    .zip(&counters.fetch_failures)
                    .map(|(reason, counter)| (*reason, get(counter)))
                    .collect(),
                duration_buckets: FETCH_BUCKETS
                    .iter()
                    .zip(&counters.fetch_duration_buckets)
                    .scan(0, |total, (bound, counter)| {
                        *total += get(counter);
                        Some((*bound, *total))
                    })
                    .collect(),
                duration_seconds_sum: get(&counters.fetch_duration_micros) as f64 / 1e6,
                duration_count: get(&counters.fetch_duration_count),
            },
            upstream: UpstreamSnapshot {
                requests: get(&counters.upstream_requests),
                in_flight: get(&counters.upstream_in_flight),
                failures: FETCH_REASONS
                    .iter()
                    .zip(&counters.upstream_failures)
                    .map(|(reason, counter)| (*reason, get(counter)))
                    .collect(),
            },
            cache: None,
            admission: None,
            external_memory: None,
            memory: MemorySnapshot {
                crypto_keys: crate::webcrypto::live_keys(),
                malloc: MallocSnapshot::current(),
            },
            executors: Vec::new(),
        }
    }

    pub fn active(&self) -> usize {
        self.executors.iter().map(|e| e.active).sum()
    }

    pub fn ready_executors(&self) -> usize {
        self.executors.iter().filter(|e| e.state == "ready").count()
    }

    /// Prometheus text exposition format (version 0.0.4).
    pub fn to_prometheus(&self) -> String {
        let mut out = String::new();
        let mut metric = |name: &str, kind: &str, help: &str, value: String| {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}"
            );
        };
        metric(
            "nextrs_executor_isolates",
            "gauge",
            "Configured V8 isolates.",
            self.isolates.to_string(),
        );
        metric(
            "nextrs_executor_ready_isolates",
            "gauge",
            "Isolates currently accepting requests.",
            self.ready_executors().to_string(),
        );
        metric(
            "nextrs_executor_active_requests",
            "gauge",
            "Requests executing in an isolate.",
            self.active().to_string(),
        );
        metric(
            "nextrs_executor_queued_requests",
            "gauge",
            "Requests waiting for an executor slot.",
            self.queued.to_string(),
        );
        for (name, help, value) in [
            (
                "nextrs_executor_requests_admitted_total",
                "Requests handed to an isolate.",
                self.requests_admitted,
            ),
            (
                "nextrs_executor_requests_completed_total",
                "Requests whose handler and waitUntil work finished.",
                self.requests_completed,
            ),
            (
                "nextrs_executor_responses_cancelled_total",
                "Responses dropped before completion (client disconnect or cancellation).",
                self.responses_cancelled,
            ),
            (
                "nextrs_executor_backpressure_waits_total",
                "Response writes that waited for a slow consumer.",
                self.backpressure_waits,
            ),
            (
                "nextrs_executor_response_events_total",
                "Response events (head, body chunks, end) received by the HTTP side.",
                self.response_events,
            ),
            (
                "nextrs_executor_response_event_waits_total",
                "Response events the HTTP side had to wait for (nothing was queued).",
                self.response_event_waits,
            ),
            (
                "nextrs_executor_request_timeouts_total",
                "Requests that exceeded the request deadline.",
                self.request_timeouts,
            ),
            (
                "nextrs_executor_wait_until_timeouts_total",
                "waitUntil/after() work abandoned at the request deadline.",
                self.wait_until_timeouts,
            ),
            (
                "nextrs_executor_handler_errors_total",
                "Route handlers that threw.",
                self.handler_errors,
            ),
            (
                "nextrs_executor_request_body_errors_total",
                "Request bodies that failed (too large, aborted).",
                self.request_body_errors,
            ),
            (
                "nextrs_executor_queue_waits_total",
                "Requests that waited in the admission queue.",
                self.queue_waits,
            ),
        ] {
            metric(name, "counter", help, value.to_string());
        }
        metric(
            "nextrs_executor_queue_wait_seconds_total",
            "counter",
            "Total time requests spent in the admission queue.",
            format!("{:.6}", self.queue_wait_seconds_total),
        );
        metric(
            "nextrs_executor_execution_seconds_total",
            "counter",
            "Total time from delivery to completion of requests.",
            format!("{:.6}", self.execution_seconds_total),
        );
        let _ = writeln!(
            out,
            "# HELP nextrs_executor_rejected_total Requests refused before execution.\n# TYPE nextrs_executor_rejected_total counter"
        );
        for (reason, value) in [
            ("overloaded", self.rejected_overloaded),
            ("queue_timeout", self.rejected_queue_timeout),
            ("shutting_down", self.rejected_shutting_down),
            ("unavailable", self.rejected_unavailable),
            ("memory_pressure", self.rejected_memory_pressure),
        ] {
            let _ = writeln!(
                out,
                "nextrs_executor_rejected_total{{reason=\"{reason}\"}} {value}"
            );
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_executor_slot_active_requests Requests executing per isolate slot.\n# TYPE nextrs_executor_slot_active_requests gauge"
        );
        for e in &self.executors {
            let _ = writeln!(
                out,
                "nextrs_executor_slot_active_requests{{slot=\"{}\",state=\"{}\"}} {}",
                e.slot, e.state, e.active
            );
        }
        for (name, help, value) in [
            (
                "nextrs_executor_heap_used_bytes",
                "V8 heap in use per isolate slot.",
                (|e: &ExecutorSnapshot| e.heap_used_bytes) as fn(&ExecutorSnapshot) -> usize,
            ),
            (
                "nextrs_executor_heap_total_bytes",
                "V8 heap reserved per isolate slot.",
                |e: &ExecutorSnapshot| e.heap_total_bytes,
            ),
            (
                "nextrs_executor_external_bytes",
                "Memory held outside the V8 heap by JavaScript objects, per isolate slot.",
                |e: &ExecutorSnapshot| e.external_bytes,
            ),
            (
                "nextrs_executor_heap_limit_bytes",
                "V8 heap limit, per isolate slot.",
                |e: &ExecutorSnapshot| e.heap_limit_bytes,
            ),
            (
                "nextrs_executor_heap_used_after_gc_bytes",
                "V8 heap in use after the latest full GC, per isolate slot.",
                |e: &ExecutorSnapshot| e.heap_used_after_gc_bytes,
            ),
        ] {
            let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} gauge");
            for e in &self.executors {
                let _ = writeln!(out, "{name}{{slot=\"{}\"}} {}", e.slot, value(e));
            }
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_executor_restarts_total Isolate restarts per slot.\n# TYPE nextrs_executor_restarts_total counter"
        );
        for e in &self.executors {
            let _ = writeln!(
                out,
                "nextrs_executor_restarts_total{{slot=\"{}\"}} {}",
                e.slot, e.restarts
            );
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_executor_full_gcs_total Full (mark-compact) V8 collections per slot, this isolate generation.\n# TYPE nextrs_executor_full_gcs_total counter"
        );
        for e in &self.executors {
            let _ = writeln!(
                out,
                "nextrs_executor_full_gcs_total{{slot=\"{}\"}} {}",
                e.slot, e.full_gcs
            );
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_executor_failures_total Isolate failures per slot and reason.\n# TYPE nextrs_executor_failures_total counter"
        );
        for e in &self.executors {
            for (reason, value) in &e.failures {
                let _ = writeln!(
                    out,
                    "nextrs_executor_failures_total{{slot=\"{}\",reason=\"{reason}\"}} {value}",
                    e.slot
                );
            }
        }
        self.fetch.write_prometheus(&mut out);
        self.upstream.write_prometheus(&mut out);
        if let Some(cache) = &self.cache {
            cache.write_prometheus(&mut out);
        }
        if let Some(admission) = &self.admission {
            admission.write_prometheus(&mut out);
        }
        if let Some(external) = &self.external_memory {
            for (name, kind, help, value) in [
                (
                    "nextrs_external_memory_limit_bytes",
                    "gauge",
                    "Limit on ArrayBuffer memory allocated for JavaScript, all isolates.",
                    external.limit_bytes,
                ),
                (
                    "nextrs_external_memory_used_bytes",
                    "gauge",
                    "ArrayBuffer memory allocated through the budgeted allocator, live.",
                    external.used_bytes,
                ),
                (
                    "nextrs_external_memory_peak_bytes",
                    "gauge",
                    "Highest ArrayBuffer memory allocated through the budgeted allocator.",
                    external.peak_bytes,
                ),
                (
                    "nextrs_external_memory_refused_total",
                    "counter",
                    "ArrayBuffer allocations refused by the limit (each a RangeError in JavaScript).",
                    external.refused,
                ),
                (
                    "nextrs_external_memory_refused_bytes_total",
                    "counter",
                    "Bytes of refused ArrayBuffer allocations.",
                    external.refused_bytes,
                ),
            ] {
                let _ = writeln!(
                    out,
                    "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}"
                );
            }
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_executor_memory_reclaims_total Memory reclamations requested from the isolates, by trigger.\n\
             # TYPE nextrs_executor_memory_reclaims_total counter\n\
             nextrs_executor_memory_reclaims_total{{trigger=\"idle\"}} {}\n\
             nextrs_executor_memory_reclaims_total{{trigger=\"pressure\"}} {}",
            self.memory_reclaims_idle, self.memory_reclaims_pressure
        );
        let _ = writeln!(
            out,
            "# HELP nextrs_executor_memory_reclaim_seconds_total Garbage collection time the isolates spent on memory reclamations.\n\
             # TYPE nextrs_executor_memory_reclaim_seconds_total counter\n\
             nextrs_executor_memory_reclaim_seconds_total {:.6}\n\
             # HELP nextrs_executor_memory_reclaim_heap_released_bytes_total Committed V8 heap memory given back by memory reclamations.\n\
             # TYPE nextrs_executor_memory_reclaim_heap_released_bytes_total counter\n\
             nextrs_executor_memory_reclaim_heap_released_bytes_total {}",
            self.memory_reclaim_seconds_total, self.memory_reclaim_heap_released_bytes
        );
        out
    }
}

/// Memory-aware admission ([`crate::MemoryAdmission`]).
#[derive(Debug, Clone, Serialize)]
pub struct AdmissionSnapshot {
    /// `normal`, `elevated`, `critical` or `degraded`.
    pub state: &'static str,
    pub budget_bytes: u64,
    /// The latest measured total (usage minus inactive page cache).
    pub working_set_bytes: u64,
    /// Estimated cost of requests admitted since that reading.
    pub reserved_bytes: u64,
    pub reading_age_seconds: f64,
    pub queued_bytes: u64,
    pub state_transitions: u64,
    pub sample_failures: u64,
    pub deferred: u64,
    pub progress_admissions: u64,
    pub heap_skips: u64,
    /// Refusals by request class.
    pub rejected: Vec<(&'static str, u64)>,
}

impl AdmissionSnapshot {
    pub(crate) fn new(
        counters: &Counters,
        gate: &crate::admission::Gate,
        queued_bytes: usize,
        now: std::time::Instant,
    ) -> Self {
        Self {
            state: gate.state().as_str(),
            budget_bytes: gate.budget().unwrap_or(0),
            working_set_bytes: gate.working_set(),
            reserved_bytes: gate.reserved(),
            reading_age_seconds: gate.reading_age(now).map_or(-1.0, |a| a.as_secs_f64()),
            queued_bytes: u64::try_from(queued_bytes).unwrap_or(u64::MAX),
            state_transitions: gate.transitions,
            sample_failures: gate.sample_failures,
            deferred: get(&counters.admission_deferred),
            progress_admissions: get(&counters.admission_progress),
            heap_skips: get(&counters.admission_heap_skips),
            rejected: crate::admission::RequestClass::ALL
                .iter()
                .map(|c| {
                    (
                        c.as_str(),
                        get(&counters.rejected_memory_pressure[c.index()]),
                    )
                })
                .collect(),
        }
    }

    fn write_prometheus(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "# HELP nextrs_admission_state Memory admission state (1 for the current one).
# TYPE nextrs_admission_state gauge"
        );
        for state in ["normal", "elevated", "critical", "degraded"] {
            let _ = writeln!(
                out,
                "nextrs_admission_state{{state=\"{state}\"}} {}",
                u8::from(self.state == state)
            );
        }
        for (name, kind, help, value) in [
            (
                "nextrs_admission_budget_bytes",
                "gauge",
                "Memory budget admission compares against.",
                self.budget_bytes.to_string(),
            ),
            (
                "nextrs_admission_working_set_bytes",
                "gauge",
                "Latest measured working set (usage minus inactive page cache).",
                self.working_set_bytes.to_string(),
            ),
            (
                "nextrs_admission_reserved_bytes",
                "gauge",
                "Estimated memory of requests admitted since the latest reading.",
                self.reserved_bytes.to_string(),
            ),
            (
                "nextrs_admission_reading_age_seconds",
                "gauge",
                "Age of the latest memory reading (-1: none).",
                format!("{:.3}", self.reading_age_seconds),
            ),
            (
                "nextrs_admission_queued_bytes",
                "gauge",
                "Header and URL bytes held by queued requests.",
                self.queued_bytes.to_string(),
            ),
            (
                "nextrs_admission_state_transitions_total",
                "counter",
                "Memory admission state changes.",
                self.state_transitions.to_string(),
            ),
            (
                "nextrs_admission_sample_failures_total",
                "counter",
                "Memory readings that failed.",
                self.sample_failures.to_string(),
            ),
            (
                "nextrs_admission_deferred_total",
                "counter",
                "Requests that waited in the queue for memory.",
                self.deferred.to_string(),
            ),
            (
                "nextrs_admission_progress_total",
                "counter",
                "Requests admitted under pressure because no other request was running.",
                self.progress_admissions.to_string(),
            ),
            (
                "nextrs_admission_heap_skips_total",
                "counter",
                "Times an isolate near its heap limit was passed over.",
                self.heap_skips.to_string(),
            ),
        ] {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}"
            );
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_admission_rejected_total Requests refused for memory pressure, by request class.\n# TYPE nextrs_admission_rejected_total counter"
        );
        for (class, value) in &self.rejected {
            let _ = writeln!(
                out,
                "nextrs_admission_rejected_total{{class=\"{class}\"}} {value}"
            );
        }
    }
}

impl UpstreamSnapshot {
    fn write_prometheus(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "# HELP nextrs_external_rewrite_requests_total Requests forwarded for external rewrites, including refused ones.\n# TYPE nextrs_external_rewrite_requests_total counter\nnextrs_external_rewrite_requests_total {}",
            self.requests
        );
        let _ = writeln!(
            out,
            "# HELP nextrs_external_rewrite_in_flight External-rewrite requests open.\n# TYPE nextrs_external_rewrite_in_flight gauge\nnextrs_external_rewrite_in_flight {}",
            self.in_flight
        );
        let _ = writeln!(
            out,
            "# HELP nextrs_external_rewrite_failures_total External-rewrite failures by reason.\n# TYPE nextrs_external_rewrite_failures_total counter"
        );
        for (reason, value) in &self.failures {
            let _ = writeln!(
                out,
                "nextrs_external_rewrite_failures_total{{reason=\"{reason}\"}} {value}"
            );
        }
    }
}

impl FetchSnapshot {
    fn write_prometheus(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "# HELP nextrs_fetch_requests_total Outbound fetches started, including refused ones.\n# TYPE nextrs_fetch_requests_total counter\nnextrs_fetch_requests_total {}",
            self.requests
        );
        let _ = writeln!(
            out,
            "# HELP nextrs_fetch_in_flight Outbound fetches held open by isolates.\n# TYPE nextrs_fetch_in_flight gauge\nnextrs_fetch_in_flight {}",
            self.in_flight
        );
        let _ = writeln!(
            out,
            "# HELP nextrs_fetch_failures_total Outbound fetch failures by reason.\n# TYPE nextrs_fetch_failures_total counter"
        );
        for (reason, value) in &self.failures {
            let _ = writeln!(
                out,
                "nextrs_fetch_failures_total{{reason=\"{reason}\"}} {value}"
            );
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_fetch_duration_seconds Time from sending an outbound fetch to its response head.\n# TYPE nextrs_fetch_duration_seconds histogram"
        );
        for (bound, count) in &self.duration_buckets {
            let _ = writeln!(
                out,
                "nextrs_fetch_duration_seconds_bucket{{le=\"{bound}\"}} {count}"
            );
        }
        let _ = writeln!(
            out,
            "nextrs_fetch_duration_seconds_bucket{{le=\"+Inf\"}} {count}\nnextrs_fetch_duration_seconds_sum {sum}\nnextrs_fetch_duration_seconds_count {count}",
            count = self.duration_count,
            sum = self.duration_seconds_sum
        );
    }
}
