//! HTTP-level metrics for the main listener (Phase 3C observability):
//! requests by status class, time to response headers, full response
//! duration, requests in flight, connections, and how response bodies
//! ended. Labels are bounded by construction: a status class (`1xx`..`5xx`)
//! and fixed outcome names. No path, route, host, cookie or header value is
//! ever a label.
//!
//! Durations are measured inside the server: from the moment hyper hands
//! over a parsed request to the moment the response head is ready (`head`)
//! and to the moment the last body byte has been handed to hyper, or the
//! body was dropped (`duration`). Time in the kernel's accept backlog and in
//! hyper's header parsing is not included.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Instant;

use axum::body::Body;
use bytes::Bytes;
use http_body::{Body as _, Frame, SizeHint};

/// Upper bounds of the latency histogram buckets, in seconds (`+Inf` is
/// implicit).
pub const BUCKETS: [f64; 14] = [
    0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

/// A fixed-bucket histogram (cumulative on export).
#[derive(Default)]
pub struct Histogram {
    buckets: [AtomicU64; BUCKETS.len() + 1],
    sum_micros: AtomicU64,
}

impl Histogram {
    pub fn observe(&self, seconds: f64) {
        let index = BUCKETS
            .iter()
            .position(|bound| seconds <= *bound)
            .unwrap_or(BUCKETS.len());
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
        self.sum_micros
            .fetch_add((seconds * 1e6) as u64, Ordering::Relaxed);
    }

    /// (cumulative counts per bucket including `+Inf`, sum in seconds).
    pub fn snapshot(&self) -> (Vec<u64>, f64) {
        let mut total = 0;
        let counts = self
            .buckets
            .iter()
            .map(|b| {
                total += b.load(Ordering::Relaxed);
                total
            })
            .collect();
        (counts, self.sum_micros.load(Ordering::Relaxed) as f64 / 1e6)
    }
}

/// Status classes, in label order.
pub const CLASSES: [&str; 6] = ["1xx", "2xx", "3xx", "4xx", "5xx", "other"];

fn class(status: u16) -> usize {
    match status {
        100..=199 => 0,
        200..=299 => 1,
        300..=399 => 2,
        400..=499 => 3,
        500..=599 => 4,
        _ => 5,
    }
}

/// How a response body ended.
pub const OUTCOMES: [&str; 3] = ["completed", "aborted", "failed"];

/// Upper bounds of the serving-stage histogram buckets, in seconds (`+Inf`
/// is implicit): finer than [`BUCKETS`], since routing and in-memory
/// answers take microseconds.
pub const STAGE_BUCKETS: [f64; 14] = [
    0.000_025, 0.000_05, 0.000_1, 0.000_25, 0.000_5, 0.001, 0.002_5, 0.005, 0.01, 0.025, 0.05, 0.1,
    0.25, 1.0,
];

/// A fixed-bucket histogram over [`STAGE_BUCKETS`] (cumulative on export).
#[derive(Default)]
pub struct StageHistogram {
    buckets: [AtomicU64; STAGE_BUCKETS.len() + 1],
    sum_nanos: AtomicU64,
}

impl StageHistogram {
    pub fn observe(&self, elapsed: std::time::Duration) {
        let seconds = elapsed.as_secs_f64();
        let index = STAGE_BUCKETS
            .iter()
            .position(|bound| seconds <= *bound)
            .unwrap_or(STAGE_BUCKETS.len());
        self.buckets[index].fetch_add(1, Ordering::Relaxed);
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.sum_nanos.fetch_add(nanos, Ordering::Relaxed);
    }

    /// (cumulative counts per bucket including `+Inf`, sum in seconds).
    pub fn snapshot(&self) -> (Vec<u64>, f64) {
        let mut total = 0u64;
        let counts = self
            .buckets
            .iter()
            .map(|b| {
                total = total.saturating_add(b.load(Ordering::Relaxed));
                total
            })
            .collect();
        (counts, self.sum_nanos.load(Ordering::Relaxed) as f64 / 1e9)
    }

    fn write(&self, out: &mut String, name: &str, labels: &str) {
        use std::fmt::Write;
        let (counts, sum) = self.snapshot();
        let total = counts.last().copied().unwrap_or(0);
        if total == 0 {
            return;
        }
        let sep = if labels.is_empty() { "" } else { "," };
        for (bound, count) in STAGE_BUCKETS.iter().zip(&counts) {
            let _ = writeln!(out, "{name}_bucket{{{labels}{sep}le=\"{bound}\"}} {count}");
        }
        let _ = writeln!(out, "{name}_bucket{{{labels}{sep}le=\"+Inf\"}} {total}");
        let braces = |s: &str| {
            if s.is_empty() {
                String::new()
            } else {
                format!("{{{s}}}")
            }
        };
        let _ = writeln!(out, "{name}_sum{} {sum}", braces(labels));
        let _ = writeln!(out, "{name}_count{} {total}", braces(labels));
    }
}

/// What answered a request (label values of `nextrs_http_served_total`):
/// a `/_next/static` file, a `public/` file, the image optimizer, a
/// response Rust held in memory (prerendered HTML or RSC, the not-found
/// page), Next's handler in an isolate, a redirect, a not-found answer
/// without JavaScript, or anything else (the proxy's own answers, external
/// rewrites, errors). Bounded by construction.
pub const VIA: [&str; 8] = [
    "static",
    "public",
    "image",
    "prerendered",
    "executor",
    "redirect",
    "not_found",
    "other",
];

/// Index into [`VIA`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Via {
    Static = 0,
    Public = 1,
    Image = 2,
    Prerendered = 3,
    Executor = 4,
    Redirect = 5,
    NotFound = 6,
    Other = 7,
}

/// Where the server spends a request's time before the response head:
/// routing (next.config rules and, when there is one, the proxy), then
/// serving, by what answered. Low overhead (two clock reads and a few
/// relaxed atomic adds per request) and always on.
#[derive(Default)]
pub struct ServeStages {
    served: [AtomicU64; VIA.len()],
    /// The routing phases (`resolve`), proxy included.
    routing: StageHistogram,
    /// From the request reaching the application to its response head,
    /// before compression, by [`VIA`].
    head: [StageHistogram; VIA.len()],
    /// Requests the proxy (`proxy.ts` / middleware) ran for.
    proxy_runs: AtomicU64,
}

impl ServeStages {
    pub(crate) fn routed(&self, elapsed: std::time::Duration) {
        self.routing.observe(elapsed);
    }

    pub(crate) fn served(&self, via: Via, elapsed: std::time::Duration) {
        let i = via as usize;
        self.served[i].fetch_add(1, Ordering::Relaxed);
        self.head[i].observe(elapsed);
    }

    pub(crate) fn proxy_ran(&self) {
        self.proxy_runs.fetch_add(1, Ordering::Relaxed);
    }

    fn write_prometheus(&self, out: &mut String) {
        use std::fmt::Write;
        let _ = writeln!(
            out,
            "# HELP nextrs_http_served_total Requests by what answered them (static file, public file, image optimizer, response held in memory, Next's handler in an isolate, redirect, not-found without JavaScript, other).\n\
             # TYPE nextrs_http_served_total counter"
        );
        for (i, via) in VIA.iter().enumerate() {
            let _ = writeln!(
                out,
                "nextrs_http_served_total{{via=\"{via}\"}} {}",
                self.served[i].load(Ordering::Relaxed)
            );
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_http_proxy_runs_total Requests the proxy (middleware) ran for.\n\
             # TYPE nextrs_http_proxy_runs_total counter\n\
             nextrs_http_proxy_runs_total {}",
            self.proxy_runs.load(Ordering::Relaxed)
        );
        let name = "nextrs_http_routing_seconds";
        let _ = writeln!(
            out,
            "# HELP {name} Time in the routing phases (next.config rules, the proxy, filesystem and route matching).\n# TYPE {name} histogram"
        );
        self.routing.write(out, name, "");
        let name = "nextrs_http_served_head_seconds";
        let _ = writeln!(
            out,
            "# HELP {name} Time from request to response head before compression, by what answered.\n# TYPE {name} histogram"
        );
        for (i, via) in VIA.iter().enumerate() {
            self.head[i].write(out, name, &format!("via=\"{via}\""));
        }
    }

    fn to_json(&self) -> serde_json::Value {
        let hist = |h: &StageHistogram| {
            let (counts, sum) = h.snapshot();
            serde_json::json!({ "count": counts.last().copied().unwrap_or(0), "sum_seconds": sum, "buckets": counts })
        };
        serde_json::json!({
            "served": VIA.iter().zip(&self.served).map(|(v, n)| ((*v).to_owned(), n.load(Ordering::Relaxed).into())).collect::<serde_json::Map<_, _>>(),
            "proxy_runs": self.proxy_runs.load(Ordering::Relaxed),
            "routing_seconds": hist(&self.routing),
            "served_head_seconds": VIA.iter().zip(&self.head).map(|(v, h)| ((*v).to_owned(), hist(h))).collect::<serde_json::Map<_, _>>(),
            "bucket_bounds_seconds": STAGE_BUCKETS,
        })
    }
}

#[derive(Default)]
pub struct HttpMetrics {
    pub requests: [AtomicU64; 6],
    pub in_flight: AtomicI64,
    pub head: [Histogram; 6],
    pub duration: [Histogram; 6],
    /// completed: the body was delivered to the end; aborted: dropped
    /// before its end (client disconnect, write timeout, shutdown);
    /// failed: the body itself returned an error (a stream that broke).
    pub bodies: [AtomicU64; 3],
    pub connections_open: AtomicI64,
    pub connections_total: AtomicU64,
    /// Set when the server stops accepting connections (graceful shutdown):
    /// readiness turns false from then on.
    pub draining: AtomicBool,
    pub started: std::sync::OnceLock<Instant>,
    /// Routing and serving stages (see [`ServeStages`]).
    pub stages: ServeStages,
}

impl std::fmt::Debug for HttpMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpMetrics")
            .field("in_flight", &self.in_flight.load(Ordering::Relaxed))
            .field(
                "connections_open",
                &self.connections_open.load(Ordering::Relaxed),
            )
            .field("draining", &self.draining.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl HttpMetrics {
    pub fn new() -> Arc<Self> {
        let metrics = Self::default();
        let _ = metrics.started.set(Instant::now());
        Arc::new(metrics)
    }

    pub fn uptime_seconds(&self) -> f64 {
        self.started
            .get()
            .map_or(0.0, |started| started.elapsed().as_secs_f64())
    }

    /// Called when a request reaches the application.
    pub(crate) fn start(self: &Arc<Self>) -> RequestTimer {
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        RequestTimer {
            metrics: self.clone(),
            started: Instant::now(),
            done: false,
        }
    }

    pub(crate) fn connection_opened(&self) {
        self.connections_open.fetch_add(1, Ordering::Relaxed);
        self.connections_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn connection_closed(&self) {
        self.connections_open.fetch_sub(1, Ordering::Relaxed);
    }

    /// Prometheus text format.
    pub fn to_prometheus(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "# HELP nextrs_http_requests_total Requests answered, by status class.\n\
             # TYPE nextrs_http_requests_total counter"
        );
        for (i, c) in CLASSES.iter().enumerate() {
            let _ = writeln!(
                out,
                "nextrs_http_requests_total{{class=\"{c}\"}} {}",
                self.requests[i].load(Ordering::Relaxed)
            );
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_http_requests_in_flight Requests received whose response has not ended.\n\
             # TYPE nextrs_http_requests_in_flight gauge\n\
             nextrs_http_requests_in_flight {}",
            self.in_flight.load(Ordering::Relaxed).max(0)
        );
        for (name, help, hist) in [
            (
                "nextrs_http_response_head_seconds",
                "Time from request to response head (status and headers ready).",
                &self.head,
            ),
            (
                "nextrs_http_request_duration_seconds",
                "Time from request to the end of the response body (or its abandonment).",
                &self.duration,
            ),
        ] {
            let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} histogram");
            for (i, c) in CLASSES.iter().enumerate() {
                let (counts, sum) = hist[i].snapshot();
                if counts.last().copied().unwrap_or(0) == 0 {
                    continue;
                }
                for (bound, count) in BUCKETS.iter().zip(&counts) {
                    let _ = writeln!(out, "{name}_bucket{{class=\"{c}\",le=\"{bound}\"}} {count}");
                }
                let total = counts.last().copied().unwrap_or(0);
                let _ = writeln!(out, "{name}_bucket{{class=\"{c}\",le=\"+Inf\"}} {total}");
                let _ = writeln!(out, "{name}_sum{{class=\"{c}\"}} {sum}");
                let _ = writeln!(out, "{name}_count{{class=\"{c}\"}} {total}");
            }
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_http_response_bodies_total Response bodies by how they ended.\n\
             # TYPE nextrs_http_response_bodies_total counter"
        );
        for (i, o) in OUTCOMES.iter().enumerate() {
            let _ = writeln!(
                out,
                "nextrs_http_response_bodies_total{{outcome=\"{o}\"}} {}",
                self.bodies[i].load(Ordering::Relaxed)
            );
        }
        let _ = writeln!(
            out,
            "# HELP nextrs_http_connections_open Client connections open.\n\
             # TYPE nextrs_http_connections_open gauge\n\
             nextrs_http_connections_open {}\n\
             # HELP nextrs_http_connections_total Client connections accepted.\n\
             # TYPE nextrs_http_connections_total counter\n\
             nextrs_http_connections_total {}\n\
             # HELP nextrs_http_draining 1 once graceful shutdown has begun.\n\
             # TYPE nextrs_http_draining gauge\n\
             nextrs_http_draining {}",
            self.connections_open.load(Ordering::Relaxed).max(0),
            self.connections_total.load(Ordering::Relaxed),
            u8::from(self.draining.load(Ordering::Relaxed)),
        );
        self.stages.write_prometheus(&mut out);
        write_stream_compression(&mut out);
        out
    }

    /// The same values for `/metrics.json`.
    pub fn to_json(&self) -> serde_json::Value {
        let by_class = |values: &[AtomicU64; 6]| {
            CLASSES
                .iter()
                .zip(values)
                .map(|(c, v)| ((*c).to_owned(), v.load(Ordering::Relaxed).into()))
                .collect::<serde_json::Map<_, _>>()
        };
        let quantiles = |hist: &[Histogram; 6]| {
            CLASSES
                .iter()
                .zip(hist)
                .filter_map(|(c, h)| {
                    let (counts, sum) = h.snapshot();
                    let total = *counts.last()?;
                    (total > 0).then(|| {
                        ((*c).to_owned(), serde_json::json!({ "count": total, "sum_seconds": sum, "buckets": counts }))
                    })
                })
                .collect::<serde_json::Map<_, _>>()
        };
        serde_json::json!({
            "requests": by_class(&self.requests),
            "in_flight": self.in_flight.load(Ordering::Relaxed).max(0),
            "head_seconds": quantiles(&self.head),
            "duration_seconds": quantiles(&self.duration),
            "bucket_bounds_seconds": BUCKETS,
            "bodies": OUTCOMES.iter().zip(&self.bodies).map(|(o, v)| ((*o).to_owned(), v.load(Ordering::Relaxed).into())).collect::<serde_json::Map<_, _>>(),
            "connections_open": self.connections_open.load(Ordering::Relaxed).max(0),
            "connections_total": self.connections_total.load(Ordering::Relaxed),
            "draining": self.draining.load(Ordering::Relaxed),
            "stages": self.stages.to_json(),
            "stream_compression": crate::compress::stream_stats().to_json(),
        })
    }
}

/// Streamed gzip (responses compressed as they are produced: everything
/// Next's handler answers, and bodies the compression cache does not hold).
fn write_stream_compression(out: &mut String) {
    use std::fmt::Write;
    let stats = crate::compress::stream_stats();
    for (name, kind, help, value) in [
        (
            "nextrs_compression_stream_responses_total",
            "counter",
            "Responses gzip-compressed as they streamed.",
            stats.responses,
        ),
        (
            "nextrs_compression_stream_input_bytes_total",
            "counter",
            "Identity bytes fed to streamed gzip.",
            stats.input_bytes,
        ),
        (
            "nextrs_compression_stream_output_bytes_total",
            "counter",
            "Gzip bytes produced by streamed compression.",
            stats.output_bytes,
        ),
        (
            "nextrs_compression_stream_input_frames_total",
            "counter",
            "Body frames fed to streamed gzip (each ends in a sync flush).",
            stats.input_frames,
        ),
    ] {
        let _ = writeln!(
            out,
            "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}"
        );
    }
    let _ = writeln!(
        out,
        "# HELP nextrs_compression_stream_seconds_total Time spent in the gzip encoder for streamed responses.\n\
         # TYPE nextrs_compression_stream_seconds_total counter\n\
         nextrs_compression_stream_seconds_total {}",
        stats.encode_nanos as f64 / 1e9
    );
}

/// One request's timing. Dropped without `respond` (the handler future was
/// cancelled: the connection went away first), it still leaves the
/// in-flight count.
pub(crate) struct RequestTimer {
    metrics: Arc<HttpMetrics>,
    started: Instant,
    done: bool,
}

impl RequestTimer {
    /// The response head is ready: count it and time the body.
    pub(crate) fn respond(
        mut self,
        response: axum::response::Response,
    ) -> axum::response::Response {
        let class = class(response.status().as_u16());
        let m = &self.metrics;
        m.requests[class].fetch_add(1, Ordering::Relaxed);
        m.head[class].observe(self.started.elapsed().as_secs_f64());
        self.done = true;
        let metrics = self.metrics.clone();
        let started = self.started;
        let declared = response
            .headers()
            .get(axum::http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        response.map(|body| {
            let finished = body.is_end_stream();
            let mut metered = MeteredBody {
                inner: body,
                metrics,
                started,
                class,
                ended: false,
                declared,
                sent: 0,
            };
            if finished {
                metered.end(0);
            }
            Body::new(metered)
        })
    }
}

impl Drop for RequestTimer {
    fn drop(&mut self) {
        if !self.done {
            // Cancelled before a response existed.
            self.metrics.in_flight.fetch_sub(1, Ordering::Relaxed);
            self.metrics.bodies[1].fetch_add(1, Ordering::Relaxed);
        }
    }
}

struct MeteredBody {
    inner: Body,
    metrics: Arc<HttpMetrics>,
    started: Instant,
    class: usize,
    ended: bool,
    /// Content-Length, if declared: hyper stops reading at the last
    /// declared byte, so reaching it is a complete body.
    declared: Option<u64>,
    sent: u64,
}

impl MeteredBody {
    /// `outcome`: an index into OUTCOMES.
    fn end(&mut self, outcome: usize) {
        if self.ended {
            return;
        }
        self.ended = true;
        let m = &self.metrics;
        m.in_flight.fetch_sub(1, Ordering::Relaxed);
        m.bodies[outcome].fetch_add(1, Ordering::Relaxed);
        m.duration[self.class].observe(self.started.elapsed().as_secs_f64());
    }
}

impl http_body::Body for MeteredBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let polled = Pin::new(&mut self.inner).poll_frame(cx);
        match &polled {
            Poll::Ready(None) => self.end(0),
            Poll::Ready(Some(Err(_))) => self.end(2),
            Poll::Ready(Some(Ok(frame))) => {
                self.sent += frame.data_ref().map_or(0, |d| d.len() as u64);
                if self.inner.is_end_stream()
                    || self.declared.is_some_and(|declared| self.sent >= declared)
                {
                    self.end(0);
                }
            }
            _ => {}
        }
        polled
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl Drop for MeteredBody {
    fn drop(&mut self) {
        // Dropped before its end: the connection closed under it.
        self.end(1);
    }
}

/// This process's memory and CPU, read from `/proc/self` at scrape time
/// (Linux; elsewhere only the uptime).
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ProcessSnapshot {
    pub resident_bytes: u64,
    pub resident_anon_bytes: u64,
    pub resident_file_bytes: u64,
    pub swap_bytes: u64,
    pub peak_resident_bytes: u64,
    pub cpu_seconds: f64,
    pub threads: u64,
    pub open_fds: u64,
    pub uptime_seconds: f64,
}

impl ProcessSnapshot {
    pub fn current(uptime_seconds: f64) -> Self {
        let mut s = Self {
            uptime_seconds,
            ..Self::default()
        };
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            let kib = |key: &str| {
                status
                    .lines()
                    .find_map(|l| l.strip_prefix(key))
                    .and_then(|rest| {
                        rest.trim()
                            .trim_end_matches(" kB")
                            .trim()
                            .parse::<u64>()
                            .ok()
                    })
                    .unwrap_or(0)
                    * 1024
            };
            s.resident_bytes = kib("VmRSS:");
            s.resident_anon_bytes = kib("RssAnon:");
            s.resident_file_bytes = kib("RssFile:") + kib("RssShmem:");
            s.swap_bytes = kib("VmSwap:");
            s.peak_resident_bytes = kib("VmHWM:");
            s.threads = kib("Threads:") / 1024;
        }
        if let Ok(stat) = std::fs::read_to_string("/proc/self/stat")
            && let Some((_, rest)) = stat.rsplit_once(") ")
        {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            // SAFETY: sysconf has no preconditions.
            let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
            let field = |i: usize| {
                fields
                    .get(i)
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            // utime and stime are fields 14 and 15 (1-based) of stat; the
            // split starts at field 3.
            s.cpu_seconds = (field(11) + field(12)) / ticks;
        }
        if let Ok(dir) = std::fs::read_dir("/proc/self/fd") {
            s.open_fds = dir.count() as u64;
        }
        s
    }
}

pub(crate) fn process_json(uptime_seconds: f64) -> serde_json::Value {
    serde_json::to_value(ProcessSnapshot::current(uptime_seconds)).unwrap_or_default()
}

/// Process and native-allocator gauges in the Prometheus text format.
pub(crate) fn process_prometheus(
    uptime_seconds: f64,
    malloc: Option<&nextrs_runtime::MallocSnapshot>,
) -> String {
    use std::fmt::Write;
    let p = ProcessSnapshot::current(uptime_seconds);
    let mut out = String::new();
    let mut gauge = |name: &str, help: &str, value: f64| {
        let _ = writeln!(
            out,
            "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}"
        );
    };
    gauge(
        "nextrs_process_resident_memory_bytes",
        "Resident set size.",
        p.resident_bytes as f64,
    );
    gauge(
        "nextrs_process_resident_memory_anon_bytes",
        "Anonymous resident memory (heaps; what grows with load).",
        p.resident_anon_bytes as f64,
    );
    gauge(
        "nextrs_process_resident_memory_file_bytes",
        "File-backed and shared resident memory.",
        p.resident_file_bytes as f64,
    );
    gauge(
        "nextrs_process_swap_bytes",
        "Swapped-out memory.",
        p.swap_bytes as f64,
    );
    gauge(
        "nextrs_process_resident_memory_peak_bytes",
        "Peak resident set size.",
        p.peak_resident_bytes as f64,
    );
    gauge("nextrs_process_threads", "Threads.", p.threads as f64);
    gauge(
        "nextrs_process_open_fds",
        "Open file descriptors (sockets included).",
        p.open_fds as f64,
    );
    gauge(
        "nextrs_process_uptime_seconds",
        "Seconds since the server started.",
        p.uptime_seconds,
    );
    let _ = writeln!(
        out,
        "# HELP nextrs_process_cpu_seconds_total User and system CPU time.\n\
         # TYPE nextrs_process_cpu_seconds_total counter\n\
         nextrs_process_cpu_seconds_total {}",
        p.cpu_seconds
    );
    if let Some(m) = malloc {
        let mut gauge = |name: &str, help: &str, value: u64| {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} gauge\n{name} {value}"
            );
        };
        gauge(
            "nextrs_malloc_in_use_bytes",
            "Native allocator: bytes in allocated blocks (arenas and mmap).",
            m.in_use_bytes + m.mmap_bytes,
        );
        gauge(
            "nextrs_malloc_free_bytes",
            "Native allocator: free bytes it keeps (retained, not a leak by itself).",
            m.free_bytes,
        );
        gauge(
            "nextrs_malloc_releasable_bytes",
            "Native allocator: free bytes at the top of the heap malloc_trim could return.",
            m.releasable_bytes,
        );
        gauge(
            "nextrs_malloc_arena_bytes",
            "Native allocator: bytes obtained for arenas.",
            m.arena_bytes,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_buckets_are_cumulative() {
        let h = Histogram::default();
        h.observe(0.0005);
        h.observe(0.003);
        h.observe(100.0);
        let (counts, sum) = h.snapshot();
        assert_eq!(counts[0], 1);
        assert_eq!(counts[2], 2); // <= 5 ms
        assert_eq!(*counts.last().unwrap(), 3);
        assert!((sum - 100.0035).abs() < 1e-3);
    }

    #[tokio::test]
    async fn bodies_are_counted_once_by_how_they_end() {
        let metrics = HttpMetrics::new();
        // Delivered to the end.
        let response = metrics
            .start()
            .respond(axum::response::Response::new(Body::from("hello")));
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(bytes, "hello");
        // Dropped before it was read.
        let response = metrics.start().respond(
            axum::response::Response::builder()
                .status(503)
                .body(Body::from_stream(futures_util::stream::pending::<
                    Result<Bytes, std::io::Error>,
                >()))
                .unwrap(),
        );
        drop(response);
        // Cancelled before a response existed.
        drop(metrics.start());
        assert_eq!(metrics.bodies[0].load(Ordering::Relaxed), 1);
        assert_eq!(metrics.bodies[1].load(Ordering::Relaxed), 2);
        assert_eq!(metrics.requests[1].load(Ordering::Relaxed), 1);
        assert_eq!(metrics.requests[4].load(Ordering::Relaxed), 1);
        assert_eq!(metrics.in_flight.load(Ordering::Relaxed), 0);
        let text = metrics.to_prometheus();
        assert!(
            text.contains("nextrs_http_requests_total{class=\"5xx\"} 1"),
            "{text}"
        );
        assert!(
            text.contains("nextrs_http_response_bodies_total{outcome=\"aborted\"} 2"),
            "{text}"
        );
    }
}
