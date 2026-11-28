//! RE-4.1D: the resource controls end to end, on real Next.js builds over a
//! real listener. Server Actions whose `redirect()` makes Next fetch the
//! target from its own server (an in-process nested request) under memory
//! pressure, emergency, concurrency exhaustion, cancellation and shutdown;
//! and a proxy (middleware) that reads request bodies with the native
//! buffer budget on. Needs the fixture builds:
//!
//!   (cd fixtures/next-app && npm ci && npx next build)
//!   (cd fixtures/next-proxy && npm ci && npx next build)
//!   cargo test -p nextrs-server --test resource_e2e -- --ignored

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nextrs_runtime::{
    Executor, ExecutorConfig, Limits, MemoryAdmission, MemoryProbe, MemoryReading,
};
use nextrs_server::{HttpMetrics, NativeBufferBudget, ServeOptions, ServerOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MIB: u64 = 1024 * 1024;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// Memory usage the test sets, of a 1000 MiB limit.
struct Probe(Mutex<u64>);

impl MemoryProbe for Probe {
    fn read(&self, _full: bool) -> Option<MemoryReading> {
        Some(MemoryReading {
            limit: Some(1000 * MIB),
            usage: *self.0.lock().unwrap() * MIB,
            inactive_file: Some(0),
        })
    }

    fn describe(&self) -> String {
        "test probe".into()
    }
}

struct Server {
    addr: SocketAddr,
    executor: Arc<Executor>,
    http: Arc<HttpMetrics>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

impl Server {
    async fn start(name: &str, limits: Limits, budget: Option<Arc<NativeBufferBudget>>) -> Server {
        let info = nextrs_adapter::inspect(&fixture(name)).expect("fixture is built");
        let mut config = ExecutorConfig::new(info.project_dir.clone(), info.dist_dir.clone());
        config.limits = limits;
        let executor = Executor::start(config).expect("executor starts");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let http = HttpMetrics::new();
        let app = nextrs_server::app(
            &info,
            &ServerOptions {
                executor: Some(executor.clone()),
                listen: addr,
                metrics: http.clone(),
                native_buffer_budget: budget,
                ..ServerOptions::default()
            },
        )
        .await
        .unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(nextrs_server::serve(
            listener,
            app,
            ServeOptions {
                metrics: http.clone(),
                shutdown_grace: Duration::from_secs(5),
                ..ServeOptions::default()
            },
            async {
                let _ = stopped.await;
            },
        ));
        Server {
            addr,
            executor,
            http,
            stop: Some(stop),
            task: Some(task),
        }
    }

    fn arrivals(&self) -> u64 {
        self.http
            .arrivals
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn commitments(&self) -> (u64, u64) {
        let admission = self.executor.metrics().admission.expect("admission on");
        (admission.commitments, admission.committed_bytes)
    }

    async fn settles(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.commitments() == (0, 0) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    async fn stop(mut self) {
        let _ = self.stop.take().unwrap().send(());
        let _ = self.task.take().unwrap().await;
        let executor = self.executor.clone();
        tokio::task::spawn_blocking(move || executor.shutdown())
            .await
            .unwrap();
    }
}

struct Reply {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> Option<String> {
        let prefix = format!("{}:", name.to_ascii_lowercase());
        self.head.lines().find_map(|line| {
            line.to_ascii_lowercase()
                .starts_with(&prefix)
                .then(|| line[prefix.len()..].trim().to_owned())
        })
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// One HTTP/1.1 request on its own connection; reads to the close.
async fn send(
    addr: SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> Reply {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a response head");
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head[9..12].parse().unwrap();
    let mut body = raw[split + 4..].to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        body = dechunk(&body);
    }
    Reply { status, head, body }
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let Some(end) = raw.windows(2).position(|w| w == b"\r\n") else {
            return out;
        };
        let size = std::str::from_utf8(&raw[..end])
            .ok()
            .and_then(|s| usize::from_str_radix(s.split(';').next().unwrap().trim(), 16).ok())
            .unwrap_or(0);
        if size == 0 || raw.len() < end + 2 + size {
            return out;
        }
        out.extend_from_slice(&raw[end + 2..end + 2 + size]);
        raw = &raw[end + 4 + size..];
    }
}

fn action_id(name: &str) -> String {
    let text = std::fs::read_to_string(
        fixture("next-app").join(".next/server/server-reference-manifest.json"),
    )
    .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
    manifest["node"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, v)| v["exportedName"] == name)
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| panic!("{name} action"))
}

/// The multipart reply React sends for `action(formData)`: the form's
/// fields prefixed `_1_`, and `0` pointing at them.
fn form_reply(fields: &[(&str, &str)]) -> (String, Vec<u8>) {
    let boundary = "nextrs-re41-boundary";
    let mut body = String::new();
    for (name, value) in fields
        .iter()
        .map(|(n, v)| (format!("_1_{n}"), *v))
        .chain([("0".to_owned(), "[\"$K1\"]")])
    {
        body.push_str(&format!(
            "--{boundary}\r\ncontent-disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str(&format!("--{boundary}--\r\n"));
    (
        format!("multipart/form-data; boundary={boundary}"),
        body.into_bytes(),
    )
}

/// A client-side call of `redirectTo` that redirects to `to`: Next answers
/// with the target's RSC payload, which it fetches from its own server.
async fn redirect_action(server: &Server, to: &str) -> Reply {
    let (content_type, body) = form_reply(&[("to", to)]);
    send(
        server.addr,
        "POST",
        "/actions/redirect",
        &[
            ("accept", "text/x-component".into()),
            ("next-action", action_id("redirectTo")),
            ("origin", format!("http://{}", server.addr)),
            ("content-type", content_type),
        ],
        &body,
    )
    .await
}

fn admission(probe: &Arc<Probe>, tune: impl FnOnce(&mut MemoryAdmission)) -> Limits {
    let mut gate = MemoryAdmission {
        sample_interval: Duration::ZERO,
        stat_interval: Duration::ZERO,
        cooldown: Duration::ZERO,
        probe: Some(probe.clone()),
        ..MemoryAdmission::default()
    };
    tune(&mut gate);
    Limits {
        memory_admission: Some(gate),
        ..Limits::default()
    }
}

/// The redirect reached the page through the in-process route: one
/// request on the listener, an RSC answer naming the target.
fn assert_redirected(reply: &Reply, to: &str, what: &str) {
    assert_eq!(
        reply.status,
        200,
        "{what}: {}\n{}",
        reply.head,
        reply.text()
    );
    let redirect = reply.header("x-action-redirect").unwrap_or_default();
    assert!(
        redirect.starts_with(to),
        "{what}: x-action-redirect {redirect:?}"
    );
    assert!(
        reply
            .header("content-type")
            .unwrap_or_default()
            .starts_with("text/x-component"),
        "{what}: {}",
        reply.head
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_redirecting_server_action_completes_its_nested_request_at_critical() {
    let probe = Arc::new(Probe(Mutex::new(990)));
    let server = Server::start("next-app", admission(&probe, |_| {}), None).await;
    assert_eq!(
        server.executor.metrics().admission.unwrap().state,
        "critical"
    );
    for (i, to) in ["/actions/redirect/done?from=action", "/books/7"]
        .iter()
        .enumerate()
    {
        let before = server.arrivals();
        let reply = redirect_action(&server, to).await;
        assert_redirected(&reply, to, to);
        if i == 0 {
            assert!(reply.text().contains("Redirected"), "{}", reply.text());
        } else {
            assert!(reply.text().contains("7"), "{}", reply.text());
        }
        // The target was fetched in-process (the listener saw only the
        // action), as a nested request that the critical state did not
        // refuse although its parent was running.
        assert_eq!(server.arrivals() - before, 1, "{to}: nested in-process");
        assert!(server.settles().await, "{to}: {:?}", server.commitments());
    }
    let admission = server.executor.metrics().admission.unwrap();
    assert!(
        admission.progress_admissions >= 2,
        "parents admitted by the progress rule"
    );
    assert_eq!(
        admission.rejected.iter().map(|(_, n)| n).sum::<u64>(),
        0,
        "no nested request refused"
    );
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn emergency_admits_one_action_per_interval_and_never_runs_a_refused_one() {
    let probe = Arc::new(Probe(Mutex::new(995)));
    let server = Server::start(
        "next-app",
        admission(&probe, |gate| {
            gate.emergency_percent = Some(98.0);
            gate.emergency_progress_interval = Some(Duration::from_secs(30));
        }),
        None,
    )
    .await;
    assert_eq!(
        server.executor.metrics().admission.unwrap().state,
        "emergency"
    );
    let first = redirect_action(&server, "/actions/redirect/done?from=action").await;
    assert_redirected(
        &first,
        "/actions/redirect/done",
        "the interval's one action",
    );
    // Within the interval: refused before delivery (Next never sees it).
    let second = redirect_action(&server, "/actions/redirect/done?from=again").await;
    assert_eq!(second.status, 503, "{}", second.head);
    assert_eq!(
        second.header("x-nextrs-error").as_deref(),
        Some("executor-memory-pressure")
    );
    assert!(second.header("x-action-redirect").is_none());
    // Memory falls: admitted again without waiting for the interval.
    *probe.0.lock().unwrap() = 500;
    let third = redirect_action(&server, "/actions/redirect/done?from=recovered").await;
    assert_redirected(&third, "/actions/redirect/done", "after recovery");
    assert!(server.settles().await);
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn concurrent_redirecting_actions_with_concurrency_exhausted() {
    // One isolate, two concurrent JavaScript requests: eight actions at
    // once, each redirecting to a dynamic page, whose nested request needs
    // a slot too. Nested requests are not memory-gated but queue behind
    // other parents. Every action must end (within a few queue timeouts),
    // and leave nothing behind.
    let probe = Arc::new(Probe(Mutex::new(100)));
    let mut limits = admission(&probe, |_| {});
    limits.isolates = 1;
    limits.max_active_per_isolate = 2;
    limits.queue_timeout = Duration::from_secs(3);
    let server = Server::start("next-app", limits, None).await;
    let started = Instant::now();
    let tasks: Vec<_> = (0..8)
        .map(|i| {
            let (addr, id) = (server.addr, action_id("redirectTo"));
            tokio::spawn(async move {
                let (content_type, body) = form_reply(&[("to", "/books/5")]);
                let reply = send(
                    addr,
                    "POST",
                    "/actions/redirect",
                    &[
                        ("accept", "text/x-component".into()),
                        ("next-action", id),
                        ("origin", format!("http://{addr}")),
                        ("content-type", content_type),
                    ],
                    &body,
                )
                .await;
                (i, reply.status, reply.header("x-action-redirect").is_some())
            })
        })
        .collect();
    let mut outcomes = Vec::new();
    for task in tasks {
        outcomes.push(task.await.unwrap());
    }
    let elapsed = started.elapsed();
    println!("concurrency exhausted: {outcomes:?} in {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(30),
        "no deadlock: {elapsed:?}"
    );
    assert!(
        outcomes
            .iter()
            .all(|(_, status, _)| [200, 503].contains(status))
    );
    assert!(
        outcomes
            .iter()
            .any(|(_, status, redirected)| *status == 200 && *redirected)
    );
    assert!(server.settles().await, "{:?}", server.commitments());
    // Still serving.
    let after = redirect_action(&server, "/actions/redirect/done").await;
    assert_redirected(&after, "/actions/redirect/done", "afterwards");
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_cancelled_action_and_a_shutdown_leave_no_commitment() {
    let probe = Arc::new(Probe(Mutex::new(990)));
    let server = Server::start("next-app", admission(&probe, |_| {}), None).await;
    // Client gone right after sending the action.
    for _ in 0..4 {
        let (content_type, body) = form_reply(&[("to", "/books/9")]);
        let mut stream = tokio::net::TcpStream::connect(server.addr).await.unwrap();
        let head = format!(
            "POST /actions/redirect HTTP/1.1\r\nhost: {}\r\naccept: text/x-component\r\nnext-action: {}\r\norigin: http://{}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\n\r\n",
            server.addr,
            action_id("redirectTo"),
            server.addr,
            body.len()
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        drop(stream);
    }
    assert!(
        server.settles().await,
        "cancelled: {:?}",
        server.commitments()
    );
    let reply = redirect_action(&server, "/actions/redirect/done").await;
    assert_redirected(&reply, "/actions/redirect/done", "after cancellations");
    // Shutdown with an action in flight finishes, and releases it.
    let (addr, id) = (server.addr, action_id("redirectTo"));
    let in_flight = tokio::spawn(async move {
        let (content_type, body) = form_reply(&[("to", "/books/11")]);
        send(
            addr,
            "POST",
            "/actions/redirect",
            &[
                ("accept", "text/x-component".into()),
                ("next-action", id),
                ("origin", format!("http://{addr}")),
                ("content-type", content_type),
            ],
            &body,
        )
        .await
        .status
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    let executor = server.executor.clone();
    tokio::time::timeout(Duration::from_secs(20), server.stop())
        .await
        .expect("shutdown finishes");
    let status = in_flight.await.unwrap();
    assert!([200, 503].contains(&status), "{status}");
    let admission = executor.metrics().admission.unwrap();
    assert_eq!((admission.commitments, admission.committed_bytes), (0, 0));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn an_external_request_cannot_claim_to_be_nested() {
    // Nothing a client sends marks a request nested: at critical, with a
    // request running, a direct request is refused whatever it carries.
    let probe = Arc::new(Probe(Mutex::new(990)));
    let server = Server::start("next-app", admission(&probe, |_| {}), None).await;
    let (addr, id) = (server.addr, action_id("redirectTo"));
    let holder = tokio::spawn(async move {
        let (content_type, body) = form_reply(&[("to", "/books/1")]);
        send(
            addr,
            "POST",
            "/actions/redirect",
            &[
                ("accept", "text/x-component".into()),
                ("next-action", id),
                ("origin", format!("http://{addr}")),
                ("content-type", content_type),
            ],
            &body,
        )
        .await
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.commitments().0 == 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    if server.commitments().0 > 0 {
        let spoofed = send(
            server.addr,
            "GET",
            "/books/3",
            &[
                (
                    "x-middleware-subrequest",
                    "middleware:middleware:middleware".into(),
                ),
                ("x-nextrs-nested", "1".into()),
                ("x-nextjs-internal", "1".into()),
            ],
            b"",
        )
        .await;
        assert_eq!(spoofed.status, 503, "{}", spoofed.head);
    }
    let _ = holder.await;
    assert!(server.settles().await);
    server.stop().await;
}

// ---- Proxy-buffered request bodies (next-proxy) -------------------------

fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| b'a' + (i % 26) as u8).collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-proxy to be built (npm ci && next build)"]
async fn the_proxy_reads_budgeted_request_bodies_intact() {
    let budget = NativeBufferBudget::new(256 * 1024, Duration::from_millis(500));
    let server = Server::start("next-proxy", Limits::default(), Some(budget.clone())).await;
    // The proxy reads the body and answers with it; then reads it and
    // rewrites to a route handler, which reads the same body again.
    for len in [0, 1, 1000, 64 * 1024, 200 * 1024] {
        let body = pattern(len);
        let direct = send(
            server.addr,
            "POST",
            "/proxy/body",
            &[("content-type", "text/plain".into())],
            &body,
        )
        .await;
        assert_eq!(direct.status, 200, "{len}: {}", direct.head);
        let json: serde_json::Value = serde_json::from_slice(&direct.body).unwrap();
        assert_eq!(json["length"], len, "{len}");
        assert!(
            json["text"].as_str().unwrap().as_bytes() == &body[..],
            "{len}: bytes"
        );

        let passed = send(
            server.addr,
            "POST",
            "/proxy/body-pass",
            &[("content-type", "text/plain".into())],
            &body,
        )
        .await;
        assert_eq!(passed.status, 200, "{len}: {}", passed.head);
        assert_eq!(passed.header("x-proxy-body-length"), Some(len.to_string()));
        let json: serde_json::Value = serde_json::from_slice(&passed.body).unwrap();
        assert!(
            json["body"].as_str().unwrap().as_bytes() == &body[..],
            "{len}: route copy"
        );
        assert_eq!(budget.reserved_bytes(), 0, "{len}: released");
    }
    // Larger than the whole budget: refused (413), never partly read.
    let big = send(
        server.addr,
        "POST",
        "/proxy/body",
        &[],
        &pattern(300 * 1024),
    )
    .await;
    assert_eq!(big.status, 413, "{}", big.head);
    assert_eq!(
        big.header("x-nextrs-error").as_deref(),
        Some("native-buffer-budget-exceeded")
    );
    // Concurrent bodies share the budget, wait for room and are all intact.
    let tasks: Vec<_> = (0..16)
        .map(|i| {
            let addr = server.addr;
            tokio::spawn(async move {
                let body = pattern(60 * 1024 + i);
                let reply = send(addr, "POST", "/proxy/body-pass", &[], &body).await;
                let json: serde_json::Value =
                    serde_json::from_slice(&reply.body).unwrap_or_default();
                (
                    reply.status,
                    json["body"].as_str().map(|s| s.as_bytes() == &body[..]),
                )
            })
        })
        .collect();
    for task in tasks {
        let (status, intact) = task.await.unwrap();
        assert!(
            (status == 200 && intact == Some(true)) || status == 503,
            "{status} {intact:?}"
        );
    }
    // A client that sends half its body and goes away.
    {
        let mut stream = tokio::net::TcpStream::connect(server.addr).await.unwrap();
        let head = format!(
            "POST /proxy/body HTTP/1.1\r\nhost: {}\r\ncontent-length: 100000\r\n\r\n",
            server.addr
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(&pattern(50_000)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while budget.reserved_bytes() != 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(budget.reserved_bytes(), 0, "everything released");
    let text = budget.to_prometheus();
    println!("{text}");
    assert!(!text.contains("nextrs_native_buffer_reserved_bytes_total{kind=\"proxy_body\"} 0"));
    server.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn a_nested_request_waits_for_a_slot_at_most_the_queue_timeout() {
    // Pre-existing, not memory-related (RE-4.1-NESTED-REQUEST-COMPATIBILITY):
    // a nested request needs an isolate slot of its own. With the only
    // slot held by its parent, it waits in the queue until the queue
    // timeout, fails, and Next answers the action without the target's
    // payload. Bounded, never a deadlock; a prerendered target needs no
    // slot at all.
    let probe = Arc::new(Probe(Mutex::new(100)));
    let mut limits = admission(&probe, |_| {});
    limits.isolates = 1;
    limits.max_active_per_isolate = 1;
    limits.queue_timeout = Duration::from_secs(2);
    let server = Server::start("next-app", limits, None).await;
    let started = Instant::now();
    let prerendered = redirect_action(&server, "/actions/redirect/done").await;
    assert_redirected(&prerendered, "/actions/redirect/done", "prerendered target");
    assert!(prerendered.text().contains("Redirected"));
    assert!(started.elapsed() < Duration::from_secs(2), "no slot needed");
    let started = Instant::now();
    let dynamic = redirect_action(&server, "/books/7").await;
    let elapsed = started.elapsed();
    println!(
        "dynamic target, one slot: {} redirect={:?} body {} bytes in {elapsed:?}",
        dynamic.status,
        dynamic.header("x-action-redirect"),
        dynamic.body.len()
    );
    assert!(
        elapsed >= Duration::from_secs(2) && elapsed < Duration::from_secs(6),
        "{elapsed:?}"
    );
    assert_eq!(dynamic.status, 200);
    assert!(server.settles().await);
    server.stop().await;
}
