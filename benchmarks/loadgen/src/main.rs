//! HTTP/1.1 load generator for the NextRS benchmarks.
//!
//! Two modes:
//!
//! * **closed loop** (`--rate 0`, the default): `--connections` keep-alive
//!   connections each send their next request as soon as the previous
//!   response has been read. Measures saturation throughput; its latency is
//!   service time, which hides queueing (coordinated omission).
//! * **open loop** (`--rate R`): request `i` is *scheduled* at
//!   `start + i / R`, independently of how fast the server answers. Latency
//!   is measured from the scheduled time, so a stalled server is charged for
//!   every request it delayed (coordinated-omission-aware, as wrk2). Service
//!   time (from the actual send) is recorded separately. If every connection
//!   is busy the schedule is still kept: tickets queue and their latency
//!   grows, and `late_starts` counts requests sent >1 ms after schedule.
//!
//! Every response is checked: status, required body substrings, forbidden
//! body substrings. A benchmark run with failed checks must not be used.
//!
//! The request spec is JSON (see `Spec`). Requests are taken round-robin
//! by ticket number; `{seq}` in a templated request's path, header values or
//! body is replaced by the ticket number (zero-padded to 10 digits, so no
//! identity is a prefix of another) and `{run}` by a random id of
//! this invocation (reported as `run_id`): unique identities, mutation
//! ledgers. Output is one JSON document (`--out`, or stdout).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use clap::Parser;
use hdrhistogram::Histogram;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

#[derive(Parser, Debug, Clone)]
#[command(about = "HTTP/1.1 load generator (closed and open loop)")]
struct Args {
    /// Target address, host:port.
    #[arg(long)]
    target: String,
    /// Request spec (JSON).
    #[arg(long)]
    spec: String,
    /// Keep-alive connections.
    #[arg(long, default_value_t = 32)]
    connections: usize,
    /// Offered rate in requests/second; 0 = closed loop.
    #[arg(long, default_value_t = 0.0)]
    rate: f64,
    /// Measured duration in seconds.
    #[arg(long, default_value_t = 10.0)]
    duration: f64,
    /// Warm-up seconds before measuring (same mode and load, not recorded).
    #[arg(long, default_value_t = 2.0)]
    warmup: f64,
    /// Stop after this many measured requests (closed loop), 0 = by duration.
    #[arg(long, default_value_t = 0)]
    requests: u64,
    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 30.0)]
    timeout: f64,
    /// Tokio worker threads.
    #[arg(long, default_value_t = 2)]
    threads: usize,
    /// Write the result JSON here instead of stdout.
    #[arg(long)]
    out: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct Spec {
    requests: Vec<ReqSpec>,
}

#[derive(Deserialize, Debug, Clone)]
struct ReqSpec {
    #[serde(default = "get")]
    method: String,
    path: String,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default = "ok")]
    expect_status: u16,
    #[serde(default)]
    expect_body: Vec<String>,
    #[serde(default)]
    forbid_body: Vec<String>,
    #[serde(default)]
    templated: bool,
    /// Optional label: latency is also reported per kind (mixed workloads).
    #[serde(default)]
    kind: Option<String>,
}

fn get() -> String {
    "GET".into()
}
fn ok() -> u16 {
    200
}

#[derive(Serialize, Default)]
struct Percentiles {
    count: u64,
    mean_ms: f64,
    p50_ms: f64,
    p90_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    p999_ms: f64,
    max_ms: f64,
}

fn kind_histogram() -> Histogram<u64> {
    Histogram::<u64>::new_with_bounds(1, 120_000_000, 2).unwrap()
}

fn percentiles(h: &Histogram<u64>) -> Percentiles {
    let ms = |us: u64| us as f64 / 1000.0;
    Percentiles {
        count: h.len(),
        mean_ms: h.mean() / 1000.0,
        p50_ms: ms(h.value_at_quantile(0.50)),
        p90_ms: ms(h.value_at_quantile(0.90)),
        p95_ms: ms(h.value_at_quantile(0.95)),
        p99_ms: ms(h.value_at_quantile(0.99)),
        p999_ms: ms(h.value_at_quantile(0.999)),
        max_ms: ms(h.max()),
    }
}

struct Recorder {
    latency: Histogram<u64>,
    service: Histogram<u64>,
    ttfb: Histogram<u64>,
    ok: u64,
    status_mismatch: u64,
    body_mismatch: u64,
    io_errors: u64,
    timeouts: u64,
    late_starts: u64,
    bytes: u64,
    samples: Vec<String>,
    /// Every response that passed its checks, warm-up included, by spec
    /// index: what a mutation ledger must contain exactly once.
    passed_any: Vec<u64>,
    /// Every request that did not pass (warm-up included).
    failed_any: u64,
    /// Latency per request kind (index into `Shared::kinds`).
    by_kind: Vec<Histogram<u64>>,
}

impl Recorder {
    fn new() -> Self {
        let h = || Histogram::<u64>::new_with_bounds(1, 120_000_000, 3).unwrap();
        Recorder {
            latency: h(),
            service: h(),
            ttfb: h(),
            ok: 0,
            status_mismatch: 0,
            body_mismatch: 0,
            io_errors: 0,
            timeouts: 0,
            late_starts: 0,
            bytes: 0,
            samples: Vec::new(),
            passed_any: Vec::new(),
            failed_any: 0,
            by_kind: Vec::new(),
        }
    }
    fn merge(&mut self, other: &Recorder) {
        self.latency.add(&other.latency).unwrap();
        self.service.add(&other.service).unwrap();
        self.ttfb.add(&other.ttfb).unwrap();
        self.ok += other.ok;
        self.status_mismatch += other.status_mismatch;
        self.body_mismatch += other.body_mismatch;
        self.io_errors += other.io_errors;
        self.timeouts += other.timeouts;
        self.late_starts += other.late_starts;
        self.bytes += other.bytes;
        self.failed_any += other.failed_any;
        while self.by_kind.len() < other.by_kind.len() {
            self.by_kind.push(kind_histogram());
        }
        for (i, h) in other.by_kind.iter().enumerate() {
            self.by_kind[i].add(h).unwrap();
        }
        if self.passed_any.len() < other.passed_any.len() {
            self.passed_any.resize(other.passed_any.len(), 0);
        }
        for (i, n) in other.passed_any.iter().enumerate() {
            self.passed_any[i] += n;
        }
        for s in &other.samples {
            if self.samples.len() < 10 {
                self.samples.push(s.clone());
            }
        }
    }
}

fn self_cpu() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let f: Vec<&str> = stat.rsplit_once(") ").map(|(_, r)| r.split(' ').collect()).unwrap_or_default();
    let ticks = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
    (ticks(11) + ticks(12)) / 100.0
}

fn run_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
    h.write_u32(std::process::id());
    format!("{:016x}", h.finish())
}

#[derive(Serialize)]
struct Output {
    run_id: String,
    mode: &'static str,
    target: String,
    connections: usize,
    offered_rate: f64,
    duration_s: f64,
    warmup_s: f64,
    completed: u64,
    achieved_rps: f64,
    ok: u64,
    status_mismatch: u64,
    body_mismatch: u64,
    io_errors: u64,
    timeouts: u64,
    late_starts: u64,
    bytes_received: u64,
    /// Open loop: from the scheduled send time. Closed loop: equals service.
    latency: Percentiles,
    /// From the actual send to the end of the body.
    service: Percentiles,
    /// From the actual send to the first body byte (or the head for an
    /// empty body).
    ttfb: Percentiles,
    check_failure_samples: Vec<String>,
    passed_any_by_index: Vec<u64>,
    failed_any: u64,
    /// The generator's own CPU seconds (user + system): close to
    /// `threads * duration` means the generator, not the server, limited
    /// the run.
    loadgen_cpu_s: f64,
    /// Latency (as `latency`) per request kind, when the spec labels kinds.
    by_kind: std::collections::BTreeMap<String, Percentiles>,
}

struct Prepared {
    spec: ReqSpec,
    body: Bytes,
}

fn materialize(p: &Prepared, seq: u64, run: &str, host: &str) -> Request<Full<Bytes>> {
    let s = format!("{seq:010}");
    let sub = |v: &str| {
        if p.spec.templated {
            v.replace("{seq}", &s).replace("{run}", run)
        } else {
            v.to_owned()
        }
    };
    let mut b = Request::builder()
        .method(p.spec.method.as_str())
        .uri(sub(&p.spec.path))
        .header("host", host);
    for (k, v) in &p.spec.headers {
        b = b.header(k.as_str(), sub(v));
    }
    let body = if p.spec.templated && !p.body.is_empty() {
        Bytes::from(sub(std::str::from_utf8(&p.body).unwrap()))
    } else {
        p.body.clone()
    };
    b.body(Full::new(body)).unwrap()
}

type Sender = hyper::client::conn::http1::SendRequest<Full<Bytes>>;

async fn connect(target: &str) -> std::io::Result<Sender> {
    let stream = TcpStream::connect(target).await?;
    stream.set_nodelay(true)?;
    let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(std::io::Error::other)?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    Ok(sender)
}

enum Sent {
    Ok { ttfb: Duration, status: u16, body: Vec<u8> },
    Io,
    Timeout,
}

async fn send(sender: &mut Sender, req: Request<Full<Bytes>>, timeout: Duration) -> (Sent, Instant) {
    let start = Instant::now();
    let fut = async {
        sender.ready().await.ok()?;
        let res = sender.send_request(req).await.ok()?;
        let status = res.status().as_u16();
        let mut body = res.into_body();
        let mut buf = Vec::new();
        let mut first: Option<Duration> = None;
        while let Some(frame) = body.frame().await {
            let frame = frame.ok()?;
            if let Some(d) = frame.data_ref() {
                if first.is_none() && !d.is_empty() {
                    first = Some(start.elapsed());
                }
                buf.extend_from_slice(d);
            }
        }
        Some(Sent::Ok { ttfb: first.unwrap_or_else(|| start.elapsed()), status, body: buf })
    };
    match tokio::time::timeout(timeout, fut).await {
        Ok(Some(s)) => (s, start),
        Ok(None) => (Sent::Io, start),
        Err(_) => (Sent::Timeout, start),
    }
}

fn check(p: &Prepared, seq: u64, run: &str, status: u16, body: &[u8]) -> Result<(), (bool, String)> {
    if status != p.spec.expect_status {
        return Err((true, format!("{} {}: status {status}, expected {}", p.spec.method, p.spec.path, p.spec.expect_status)));
    }
    // A templated request's required substrings are templated too, so a
    // response must carry this request's own identity (a response meant
    // for another request fails).
    for want in &p.spec.expect_body {
        let want = if p.spec.templated { want.replace("{seq}", &format!("{seq:010}")).replace("{run}", run) } else { want.clone() };
        if memchr::memmem::find(body, want.as_bytes()).is_none() {
            return Err((false, format!("{} {}: body lacks {want:?}", p.spec.method, p.spec.path)));
        }
    }
    for bad in &p.spec.forbid_body {
        if memchr::memmem::find(body, bad.as_bytes()).is_some() {
            return Err((false, format!("{} {}: body contains forbidden {bad:?}", p.spec.method, p.spec.path)));
        }
    }
    Ok(())
}

struct Shared {
    args: Args,
    reqs: Vec<Prepared>,
    ticket: AtomicU64,
    measuring: AtomicBool,
    stop: AtomicBool,
    /// Ticket number at which measurement starts (open loop).
    measure_from_ticket: AtomicU64,
    measured: AtomicU64,
    /// Random per invocation: `{run}` in templated requests.
    run_id: String,
    /// Distinct `kind` labels, and each request's index into them.
    kinds: Vec<String>,
    kind_of: Vec<Option<usize>>,
    /// Open loop: scheduled tickets from the pacer thread.
    tickets: Mutex<tokio::sync::mpsc::UnboundedReceiver<(u64, Instant)>>,
}

async fn worker(shared: Arc<Shared>, out: Arc<Mutex<Recorder>>) {
    let args = &shared.args;
    let timeout = Duration::from_secs_f64(args.timeout);
    let open = args.rate > 0.0;
    let mut rec = Recorder::new();
    let mut sender: Option<Sender> = None;
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        let (seq, scheduled) = if open {
            let next = shared.tickets.lock().await.recv().await;
            match next {
                Some((seq, at)) if !shared.stop.load(Ordering::Relaxed) => (seq, Some(at)),
                _ => break,
            }
        } else {
            (shared.ticket.fetch_add(1, Ordering::Relaxed), None)
        };
        let measuring = if open {
            seq >= shared.measure_from_ticket.load(Ordering::Relaxed)
        } else {
            shared.measuring.load(Ordering::Relaxed)
        };
        if sender.as_ref().is_none_or(|s| s.is_closed()) {
            match connect(&args.target).await {
                Ok(s) => sender = Some(s),
                Err(_) => {
                    rec.failed_any += 1;
                    if measuring {
                        rec.io_errors += 1;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
            }
        }
        let p = &shared.reqs[(seq % shared.reqs.len() as u64) as usize];
        let req = materialize(p, seq, &shared.run_id, &args.target);
        let (sent, start) = send(sender.as_mut().unwrap(), req, timeout).await;
        let done = Instant::now();
        let idx = (seq % shared.reqs.len() as u64) as usize;
        if rec.passed_any.len() < shared.reqs.len() {
            rec.passed_any.resize(shared.reqs.len(), 0);
        }
        let passed = match &sent {
            Sent::Ok { status, body, .. } => check(p, seq, &shared.run_id, *status, body),
            _ => Err((false, String::new())),
        };
        if passed.is_ok() {
            rec.passed_any[idx] += 1;
        } else {
            rec.failed_any += 1;
        }
        if !measuring {
            if !matches!(sent, Sent::Ok { .. }) {
                sender = None;
            }
            continue;
        }
        match sent {
            Sent::Ok { ttfb, body, .. } => {
                rec.bytes += body.len() as u64;
                match passed {
                    Ok(()) => rec.ok += 1,
                    Err((is_status, msg)) => {
                        if is_status {
                            rec.status_mismatch += 1;
                        } else {
                            rec.body_mismatch += 1;
                        }
                        if rec.samples.len() < 10 {
                            rec.samples.push(msg);
                        }
                    }
                }
                let service = (done - start).as_micros() as u64;
                let lat = match scheduled {
                    Some(at) => {
                        if start.saturating_duration_since(at) > Duration::from_millis(1) {
                            rec.late_starts += 1;
                        }
                        (done - at).as_micros() as u64
                    }
                    None => service,
                };
                rec.service.saturating_record(service.max(1));
                rec.latency.saturating_record(lat.max(1));
                if let Some(k) = shared.kind_of[idx] {
                    while rec.by_kind.len() <= k {
                        rec.by_kind.push(kind_histogram());
                    }
                    rec.by_kind[k].saturating_record(lat.max(1));
                }
                rec.ttfb.saturating_record((ttfb.as_micros() as u64).max(1));
            }
            Sent::Io => {
                rec.io_errors += 1;
                sender = None;
            }
            Sent::Timeout => {
                rec.timeouts += 1;
                sender = None;
            }
        }
        let n = shared.measured.fetch_add(1, Ordering::Relaxed) + 1;
        if args.requests > 0 && n >= args.requests {
            shared.stop.store(true, Ordering::Relaxed);
        }
    }
    out.lock().await.merge(&rec);
}

fn main() {
    let args = Args::parse();
    let spec: Spec = serde_json::from_str(&std::fs::read_to_string(&args.spec).expect("read spec")).expect("parse spec");
    assert!(!spec.requests.is_empty(), "spec has no requests");
    let mut kinds: Vec<String> = Vec::new();
    let kind_of: Vec<Option<usize>> = spec
        .requests
        .iter()
        .map(|r| {
            r.kind.as_ref().map(|k| match kinds.iter().position(|x| x == k) {
                Some(i) => i,
                None => {
                    kinds.push(k.clone());
                    kinds.len() - 1
                }
            })
        })
        .collect();
    let reqs = spec
        .requests
        .into_iter()
        .map(|s| Prepared { body: Bytes::from(s.body.clone().unwrap_or_default()), spec: s })
        .collect();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(args.threads)
        .enable_all()
        .build()
        .unwrap();
    let open = args.rate > 0.0;
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let shared = Arc::new(Shared {
        measure_from_ticket: AtomicU64::new(if open { (args.warmup * args.rate).ceil() as u64 } else { 0 }),
        args: args.clone(),
        reqs,
        ticket: AtomicU64::new(0),
        // Closed loop without a warm-up measures from the first request.
        measuring: AtomicBool::new(!open && args.warmup <= 0.0),
        stop: AtomicBool::new(false),
        measured: AtomicU64::new(0),
        tickets: Mutex::new(rx),
        run_id: run_id(),
        kinds,
        kind_of,
    });
    // The pacer: an OS thread with nanosleep precision (tokio's timer has
    // 1 ms granularity, which would add up to 1 ms to every open-loop
    // latency). Tickets that fall due together are released together.
    let pacer = open.then(|| {
        let shared = shared.clone();
        let rate = args.rate;
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let mut i: u64 = 0;
            while !shared.stop.load(Ordering::Relaxed) {
                let at = t0 + Duration::from_secs_f64(i as f64 / rate);
                let now = Instant::now();
                if at > now {
                    std::thread::sleep(at - now);
                }
                if tx.send((i, at)).is_err() {
                    break;
                }
                i += 1;
            }
            // Dropping tx ends the workers' recv once the queue drains;
            // queued tickets past the stop are not sent.
        })
    });
    let rec = Arc::new(Mutex::new(Recorder::new()));
    let measured_secs = rt.block_on(async {
        let handles: Vec<_> = (0..args.connections)
            .map(|_| tokio::spawn(worker(shared.clone(), rec.clone())))
            .collect();
        tokio::time::sleep(Duration::from_secs_f64(args.warmup)).await;
        shared.measuring.store(true, Ordering::Relaxed);
        let m0 = Instant::now();
        if args.requests > 0 {
            while !shared.stop.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        } else {
            tokio::time::sleep(Duration::from_secs_f64(args.duration)).await;
            shared.stop.store(true, Ordering::Relaxed);
        }
        let secs = m0.elapsed().as_secs_f64();
        if let Some(p) = pacer {
            let _ = tokio::task::spawn_blocking(move || p.join()).await;
        }
        // In-flight requests finish (bounded by the timeout) and are counted.
        for h in handles {
            let _ = h.await;
        }
        secs
    });
    let r = rt.block_on(async { std::mem::replace(&mut *rec.lock().await, Recorder::new()) });
    let completed = r.ok + r.status_mismatch + r.body_mismatch;
    let out = Output {
        run_id: shared.run_id.clone(),
        mode: if open { "open" } else { "closed" },
        target: args.target.clone(),
        connections: args.connections,
        offered_rate: args.rate,
        duration_s: measured_secs,
        warmup_s: args.warmup,
        completed,
        achieved_rps: completed as f64 / measured_secs,
        ok: r.ok,
        status_mismatch: r.status_mismatch,
        body_mismatch: r.body_mismatch,
        io_errors: r.io_errors,
        timeouts: r.timeouts,
        late_starts: r.late_starts,
        bytes_received: r.bytes,
        latency: percentiles(&r.latency),
        service: percentiles(&r.service),
        ttfb: percentiles(&r.ttfb),
        check_failure_samples: r.samples,
        passed_any_by_index: r.passed_any,
        failed_any: r.failed_any,
        loadgen_cpu_s: self_cpu(),
        by_kind: r
            .by_kind
            .iter()
            .enumerate()
            .map(|(i, h)| (shared.kinds[i].clone(), percentiles(h)))
            .collect(),
    };
    let json = serde_json::to_string_pretty(&out).unwrap();
    match &args.out {
        Some(p) => std::fs::write(p, json).expect("write output"),
        None => println!("{json}"),
    }
}
