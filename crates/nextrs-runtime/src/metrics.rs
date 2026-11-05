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
    /// The consumer dropped a delivered response before `End`/`Error`:
    /// client disconnects and server-side cancellation.
    pub responses_cancelled: AtomicU64,
    /// Writes that had to wait for the consumer to drain.
    pub backpressure_waits: AtomicU64,
    pub request_timeouts: AtomicU64,
    pub wait_until_timeouts: AtomicU64,
    pub handler_errors: AtomicU64,
    pub request_body_errors: AtomicU64,
    pub queue_wait_micros: AtomicU64,
    pub queue_waits: AtomicU64,
    pub execution_micros: AtomicU64,
    /// Outbound fetches started (including ones the policy refused).
    pub fetch_requests: AtomicU64,
    /// Fetches currently held by an isolate (not yet closed).
    pub fetch_in_flight: AtomicU64,
    pub fetch_failures: [AtomicU64; FETCH_REASONS.len()],
    /// Time to the response head, as a histogram (`FETCH_BUCKETS`).
    pub fetch_duration_buckets: [AtomicU64; FETCH_BUCKETS.len()],
    pub fetch_duration_micros: AtomicU64,
    pub fetch_duration_count: AtomicU64,
    /// Lookups that found an entry in the store (Next may still judge it
    /// expired by its tags) and lookups that found none.
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
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
    pub responses_cancelled: u64,
    pub backpressure_waits: u64,
    pub request_timeouts: u64,
    pub wait_until_timeouts: u64,
    pub handler_errors: u64,
    pub request_body_errors: u64,
    pub queue_wait_seconds_total: f64,
    pub queue_waits: u64,
    pub execution_seconds_total: f64,
    pub fetch: FetchSnapshot,
    /// `None` when the shared cache is disabled.
    pub cache: Option<CacheSnapshot>,
    pub executors: Vec<ExecutorSnapshot>,
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
            responses_cancelled: get(&counters.responses_cancelled),
            backpressure_waits: get(&counters.backpressure_waits),
            request_timeouts: get(&counters.request_timeouts),
            wait_until_timeouts: get(&counters.wait_until_timeouts),
            handler_errors: get(&counters.handler_errors),
            request_body_errors: get(&counters.request_body_errors),
            queue_wait_seconds_total: get(&counters.queue_wait_micros) as f64 / 1e6,
            queue_waits: get(&counters.queue_waits),
            execution_seconds_total: get(&counters.execution_micros) as f64 / 1e6,
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
            cache: None,
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
        if let Some(cache) = &self.cache {
            cache.write_prometheus(&mut out);
        }
        out
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
