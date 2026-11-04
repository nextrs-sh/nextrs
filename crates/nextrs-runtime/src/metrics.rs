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
}

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
        out
    }
}
