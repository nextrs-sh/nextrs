//! Outbound `fetch` through the host network capability, against a local
//! upstream started by each test (no public services). Covers the policy
//! (default deny, allowlist, address checks after DNS, redirects), Fetch
//! semantics, timeouts, size limits, abort and cancellation, streaming in
//! both directions with backpressure, request scoping, async context, and
//! isolate restarts.
mod common;

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use common::*;
use http_body_util::{BodyExt, Full, StreamBody, combinators::BoxBody};
use hyper::body::{Frame, Incoming};
use hyper::{Request, Response};
use nextrs_runtime::{
    AddrRange, Executor, Limits, NetworkConfig, NetworkLimits, NetworkPolicy, RenderRequest,
    ResponseEvent,
};
use serde_json::Value;
use sha2::Digest;
use tokio::sync::{Notify, mpsc};

// ---------------------------------------------------------------------------
// Upstream
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Upstream {
    /// Bytes `/flood` has handed to hyper.
    flooded: AtomicU64,
    /// Responses whose client went away before they ended (/hold, /stall).
    disconnects: AtomicU64,
    gate: Notify,
}

type Body = BoxBody<Bytes, Infallible>;

fn full(status: u16, content_type: &str, body: impl Into<Bytes>) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Full::new(body.into()).boxed())
        .unwrap()
}

/// A body fed through a channel. `on_close` runs when hyper drops the body
/// before the producer finished (the client disconnected).
fn channel_body(capacity: usize) -> (mpsc::Sender<Bytes>, Body) {
    let (tx, rx) = mpsc::channel::<Bytes>(capacity);
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|chunk| (Ok::<_, Infallible>(Frame::data(chunk)), rx))
    });
    (tx, BodyExt::boxed(StreamBody::new(stream)))
}

fn query(req: &Request<Incoming>) -> BTreeMap<String, String> {
    req.uri()
        .query()
        .unwrap_or("")
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_owned(), percent_decode(v)))
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn route(state: Arc<Upstream>, req: Request<Incoming>) -> Response<Body> {
    let q = query(&req);
    let num =
        |name: &str, default: u64| q.get(name).and_then(|v| v.parse().ok()).unwrap_or(default);
    match req.uri().path() {
        "/text" => {
            tokio::time::sleep(Duration::from_millis(num("delay", 0))).await;
            full(
                200,
                "text/plain",
                q.get("v").cloned().unwrap_or("hello".into()),
            )
        }
        "/echo" => {
            let method = req.method().to_string();
            let headers: BTreeMap<String, String> = req
                .headers()
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_str().unwrap_or("").to_owned()))
                .collect();
            let body = req.into_body().collect().await.unwrap().to_bytes();
            let json = serde_json::json!({
                "method": method,
                "headers": headers,
                "length": body.len(),
                "sha256": hex(&sha2::Sha256::digest(&body)),
                "body": if body.len() <= 256 { String::from_utf8_lossy(&body).into_owned() } else { String::new() },
            });
            full(200, "application/json", json.to_string())
        }
        "/slow-head" => {
            let (tx, body) = channel_body(1);
            tokio::time::sleep(Duration::from_millis(num("ms", 5000))).await;
            drop(tx);
            Response::new(body)
        }
        "/stall" => {
            // Head and one chunk, then nothing until the client leaves.
            let (tx, body) = channel_body(1);
            tokio::spawn(async move {
                let _ = tx.send(Bytes::from_static(b"first")).await;
                tx.closed().await;
                state.disconnects.fetch_add(1, Ordering::SeqCst);
            });
            Response::new(body)
        }
        "/hold" => {
            // A chunk every 20 ms until the client leaves.
            let (tx, body) = channel_body(1);
            tokio::spawn(async move {
                loop {
                    if tx.send(Bytes::from_static(b"tick\n")).await.is_err() {
                        state.disconnects.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                    tokio::select! {
                        () = tokio::time::sleep(Duration::from_millis(20)) => {}
                        () = tx.closed() => {
                            state.disconnects.fetch_add(1, Ordering::SeqCst);
                            return;
                        }
                    }
                }
            });
            Response::new(body)
        }
        "/big" => {
            let size = num("size", 1024) as usize;
            if q.get("cl").map(String::as_str) == Some("1") {
                return full(200, "application/octet-stream", vec![7u8; size]);
            }
            let (tx, body) = channel_body(1);
            tokio::spawn(async move {
                let mut left = size;
                while left > 0 {
                    let n = left.min(64 * 1024);
                    if tx.send(Bytes::from(vec![7u8; n])).await.is_err() {
                        return;
                    }
                    left -= n;
                }
            });
            Response::new(body)
        }
        "/flood" => {
            let (tx, body) = channel_body(1);
            tokio::spawn(async move {
                let chunk = Bytes::from(vec![1u8; 64 * 1024]);
                while tx.send(chunk.clone()).await.is_ok() {
                    state
                        .flooded
                        .fetch_add(chunk.len() as u64, Ordering::SeqCst);
                }
            });
            Response::new(body)
        }
        "/gate" => {
            let (tx, body) = channel_body(1);
            tokio::spawn(async move {
                let _ = tx.send(Bytes::from_static(b"first;")).await;
                state.gate.notified().await;
                let _ = tx.send(Bytes::from_static(b"second")).await;
            });
            Response::new(body)
        }
        "/redirect" => {
            let status = num("status", 302) as u16;
            Response::builder()
                .status(status)
                .header("location", q.get("to").cloned().unwrap_or_default())
                .body(Full::new(Bytes::new()).boxed())
                .unwrap()
        }
        "/loop" => Response::builder()
            .status(302)
            .header("location", "/loop")
            .body(Full::new(Bytes::new()).boxed())
            .unwrap(),
        "/gzip" => Response::builder()
            .header("content-type", "text/plain")
            .header("content-encoding", "gzip")
            .header("content-length", GZIP_BODY.len().to_string())
            .header(
                "x-accept-encoding",
                req.headers()
                    .get("accept-encoding")
                    .map(|v| v.to_str().unwrap_or("").to_owned())
                    .unwrap_or_default(),
            )
            .body(Full::new(Bytes::from_static(GZIP_BODY)).boxed())
            .unwrap(),
        "/reason" => Response::builder()
            .status(299)
            .body(Full::new(Bytes::from_static(b"odd")).boxed())
            .unwrap(),
        _ => full(404, "text/plain", "not found"),
    }
}

/// gzip of "hello gzip " x 100 (1,100 bytes decoded).
const GZIP_BODY: &[u8] = &[
    31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 203, 72, 205, 201, 201, 87, 72, 175, 202, 44, 80, 200, 24,
    101, 142, 50, 71, 153, 163, 76, 114, 153, 0, 76, 86, 215, 15, 76, 4, 0, 0,
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Starts an upstream on 127.0.0.1; returns its base URL.
async fn upstream() -> (String, u16, Arc<Upstream>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = Arc::new(Upstream::default());
    let shared = state.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                return;
            };
            let state = shared.clone();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(move |req| {
                    let state = state.clone();
                    async move { Ok::<_, Infallible>(route(state, req).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(hyper_util::rt::TokioIo::new(socket), service)
                    .await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}"), port, state)
}

// ---------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------

fn network(allow: &[String], addrs: &[&str]) -> NetworkConfig {
    NetworkConfig {
        policy: NetworkPolicy {
            allow: allow.iter().map(|r| r.parse().unwrap()).collect(),
            allow_addrs: addrs
                .iter()
                .map(|a| a.parse::<AddrRange>().unwrap())
                .collect(),
        },
        limits: NetworkLimits {
            connect_timeout: Duration::from_secs(2),
            headers_timeout: Duration::from_millis(1500),
            body_idle_timeout: Duration::from_millis(1500),
            max_response_bytes: 4 * 1024 * 1024,
            max_in_flight_per_isolate: 256,
        },
    }
}

fn start(network: Option<NetworkConfig>, limits: Limits) -> Arc<Executor> {
    let mut config = config(limits);
    config.network = network;
    Executor::start(config).expect("executor starts")
}

/// An executor allowed to reach the given upstream ports on 127.0.0.1.
fn allowed(ports: &[u16]) -> Arc<Executor> {
    let allow: Vec<String> = ports
        .iter()
        .map(|p| format!("http://127.0.0.1:{p}"))
        .collect();
    start(Some(network(&allow, &["127.0.0.1"])), small_limits())
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn fetch_request(case: &str, params: &[(&str, &str)], headers: &[(&str, &str)]) -> RenderRequest {
    let mut url = format!("/fetch?case={case}");
    for (name, value) in params {
        url.push_str(&format!("&{name}={}", encode(value)));
    }
    let mut request = request("fetch.js", headers);
    request.url = url;
    request
}

async fn run(executor: &Executor, case: &str, params: &[(&str, &str)]) -> Value {
    let out = collect(executor, fetch_request(case, params, &[])).await;
    assert_eq!(out.status, 200, "{case}: {}", out.text());
    serde_json::from_slice(&out.body).unwrap_or_else(|_| panic!("{case}: {}", out.text()))
}

fn error_code(value: &Value) -> &str {
    value["error"]["code"].as_str().unwrap_or("(none)")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn authorized_get_returns_status_headers_and_body() {
    let (base, port, _) = upstream().await;
    let executor = allowed(&[port]);
    let r = run(
        &executor,
        "get",
        &[("target", &format!("{base}/text?v=hi"))],
    )
    .await;
    assert_eq!(r["status"], 200);
    assert_eq!(r["ok"], true);
    assert_eq!(r["statusText"], "OK");
    assert_eq!(r["type"], "basic");
    assert_eq!(r["url"], format!("{base}/text?v=hi"));
    assert_eq!(r["redirected"], false);
    assert_eq!(r["contentType"], "text/plain");
    assert_eq!(r["headersImmutable"], true);
    assert_eq!(r["body"], "hi");
    // A non-canonical status keeps its code.
    let r = run(&executor, "get", &[("target", &format!("{base}/reason"))]).await;
    assert_eq!(r["status"], 299);
    assert_eq!(r["body"], "odd");
}

#[tokio::test(flavor = "multi_thread")]
async fn compressed_responses_are_decoded() {
    let (base, port, _) = upstream().await;
    let executor = allowed(&[port]);
    let r = run(&executor, "get", &[("target", &format!("{base}/gzip"))]).await;
    assert_eq!(r["body"], "hello gzip ".repeat(100));
    assert_eq!(r["contentType"], "text/plain");
    // Recorded differences from Node, which keeps both headers (with the
    // compressed length) and sends "gzip, deflate" over http: the decoder
    // removes content-encoding and content-length, and accept-encoding is
    // the client's.
    assert!(
        r["contentEncoding"].is_null() && r["contentLength"].is_null(),
        "{r}"
    );
    assert_eq!(r["acceptEncodingSent"], "gzip,deflate,br");
}

#[tokio::test(flavor = "multi_thread")]
async fn methods_bodies_and_headers_reach_the_upstream() {
    let (base, port, _) = upstream().await;
    let executor = allowed(&[port]);
    let echo = format!("{base}/echo");
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let r = run(
            &executor,
            "method",
            &[("target", &echo), ("method", method), ("body", "payload")],
        )
        .await;
        assert_eq!(r["echo"]["method"], method);
        assert_eq!(r["echo"]["body"], "payload");
        assert_eq!(r["echo"]["headers"]["content-length"], "7");
        assert_eq!(r["echo"]["headers"]["x-probe"], "1");
    }
    let r = run(&executor, "method", &[("target", &echo), ("method", "GET")]).await;
    assert_eq!(r["echo"]["method"], "GET");
    assert_eq!(r["echo"]["length"], 0);
    let r = run(
        &executor,
        "method",
        &[("target", &echo), ("method", "HEAD")],
    )
    .await;
    assert_eq!(r["status"], 200);

    // Node's default request headers, and caller headers winning over them.
    let r = run(&executor, "init", &[("target", &echo)]).await;
    assert_eq!(r["headers"]["accept"], "*/*");
    assert_eq!(r["headers"]["accept-language"], "*");
    assert_eq!(r["headers"]["sec-fetch-mode"], "cors");
    assert_eq!(r["headers"]["user-agent"], "node");
    let r = run(
        &executor,
        "init",
        &[
            ("target", &echo),
            (
                "headers",
                r#"{"user-agent":"custom","accept":"application/json","content-length":"999"}"#,
            ),
        ],
    )
    .await;
    assert_eq!(r["headers"]["user-agent"], "custom");
    assert_eq!(r["headers"]["accept"], "application/json");
    // A caller's Content-Length is ignored: it is computed from the body.
    assert!(r["headers"].get("content-length").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn network_access_is_denied_by_default() {
    let (base, _, _) = upstream().await;
    let executor = start(None, small_limits());
    let r = run(&executor, "get", &[("target", &format!("{base}/text"))]).await;
    assert_eq!(r["error"]["name"], "TypeError");
    assert_eq!(r["error"]["message"], "fetch failed");
    assert_eq!(error_code(&r), "ERR_NEXTRS_NET_DENIED");
}

#[tokio::test(flavor = "multi_thread")]
async fn destinations_outside_the_policy_are_denied() {
    let (base, port, _) = upstream().await;
    let (other, _, _) = upstream().await;
    let executor = allowed(&[port]);
    for target in [
        format!("{other}/text"),                  // another port
        format!("https://127.0.0.1:{port}/text"), // another scheme
        "http://169.254.169.254/latest/meta-data/".to_owned(),
        "http://[::1]/".to_owned(),
        "http://localhost/".to_owned(),
        "file:///etc/passwd".to_owned(),
    ] {
        let r = run(&executor, "get", &[("target", &target)]).await;
        assert_eq!(error_code(&r), "ERR_NEXTRS_NET_DENIED", "{target}: {r}");
    }
    // Credentials in the URL are a TypeError before any policy check.
    let with_credentials = base.replace("http://", "http://user:secret@");
    let r = run(
        &executor,
        "get",
        &[("target", &format!("{with_credentials}/text"))],
    )
    .await;
    assert_eq!(r["error"]["name"], "TypeError");
    assert!(
        !r.to_string().contains("secret"),
        "credentials must not be echoed: {r}"
    );

    // Allowing the host is not enough: its address must be allowed too.
    let executor = start(
        Some(network(&[format!("http://127.0.0.1:{port}")], &[])),
        small_limits(),
    );
    let r = run(&executor, "get", &[("target", &format!("{base}/text"))]).await;
    assert_eq!(error_code(&r), "ERR_NEXTRS_NET_DENIED");
    assert!(
        r["error"]["cause"].as_str().unwrap().contains("loopback"),
        "{r}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hostnames_are_checked_after_resolution() {
    let (_, port, _) = upstream().await;
    let rule = format!("http://localhost:{port}");
    let target = format!("http://localhost:{port}/text?v=resolved");
    // An allowed name that resolves to loopback is refused...
    let executor = start(
        Some(network(std::slice::from_ref(&rule), &[])),
        small_limits(),
    );
    let r = run(&executor, "get", &[("target", &target)]).await;
    assert_eq!(error_code(&r), "ERR_NEXTRS_NET_DENIED", "{r}");
    assert!(
        r["error"]["cause"].as_str().unwrap().contains("loopback"),
        "{r}"
    );
    // ...unless every address it resolves to is allowed.
    let executor = start(
        Some(network(&[rule], &["127.0.0.0/8", "::1"])),
        small_limits(),
    );
    let r = run(&executor, "get", &[("target", &target)]).await;
    assert_eq!(r["body"], "resolved", "{r}");
}

#[tokio::test(flavor = "multi_thread")]
async fn redirects_are_checked_at_every_hop() {
    let (base, port, _) = upstream().await;
    let (other, other_port, _) = upstream().await;
    let executor = allowed(&[port, other_port]);
    let redirect =
        |status: u16, to: &str| format!("{base}/redirect?status={status}&to={}", encode(to));

    // Followed to an allowed destination.
    let r = run(
        &executor,
        "redirect",
        &[("target", &redirect(302, &format!("{other}/text?v=moved")))],
    )
    .await;
    assert_eq!(r["status"], 200);
    assert_eq!(r["redirected"], true);
    assert_eq!(r["url"], format!("{other}/text?v=moved"));
    assert_eq!(r["echo"], "moved");
    // Relative locations resolve against the current URL.
    let r = run(
        &executor,
        "redirect",
        &[("target", &redirect(301, "/text?v=relative"))],
    )
    .await;
    assert_eq!(r["echo"], "relative");

    // Refused: an origin outside the allowlist, a metadata address, a
    // loopback name, another scheme.
    let (outside, _, _) = upstream().await;
    for to in [
        format!("{outside}/text"),
        "http://169.254.169.254/latest/meta-data/".to_owned(),
        format!("http://localhost:{port}/text"),
        "file:///etc/passwd".to_owned(),
        format!("https://127.0.0.1:{port}/text"),
    ] {
        let r = run(&executor, "redirect", &[("target", &redirect(302, &to))]).await;
        assert_eq!(error_code(&r), "ERR_NEXTRS_NET_DENIED", "{to}: {r}");
    }

    // Modes.
    let target = redirect(302, &format!("{other}/text"));
    let r = run(
        &executor,
        "redirect",
        &[("target", &target), ("mode", "manual")],
    )
    .await;
    assert_eq!(r["status"], 302);
    assert_eq!(r["location"], format!("{other}/text"));
    assert_eq!(r["redirected"], false);
    let r = run(
        &executor,
        "redirect",
        &[("target", &target), ("mode", "error")],
    )
    .await;
    assert_eq!(error_code(&r), "ERR_NEXTRS_NET_REDIRECT");
    let r = run(
        &executor,
        "redirect",
        &[("target", &format!("{base}/loop"))],
    )
    .await;
    assert_eq!(error_code(&r), "ERR_NEXTRS_NET_REDIRECT");
    assert!(r["error"]["cause"].as_str().unwrap().contains("too many"));
}

#[tokio::test(flavor = "multi_thread")]
async fn redirects_follow_fetch_method_and_header_rules() {
    let (base, port, _) = upstream().await;
    let (other, other_port, _) = upstream().await;
    let executor = allowed(&[port, other_port]);
    let redirect =
        |status: u16, to: &str| format!("{base}/redirect?status={status}&to={}", encode(to));
    let echo = format!("{base}/echo");

    // 303 turns any method into GET without a body; 307 keeps both.
    let r = run(
        &executor,
        "redirect",
        &[
            ("target", &redirect(303, &echo)),
            ("method", "POST"),
            ("body", "data"),
        ],
    )
    .await;
    assert_eq!(r["echo"]["method"], "GET");
    assert_eq!(r["echo"]["length"], 0);
    assert!(r["echo"]["headers"].get("content-type").is_none());
    let r = run(
        &executor,
        "redirect",
        &[
            ("target", &redirect(307, &echo)),
            ("method", "POST"),
            ("body", "data"),
        ],
    )
    .await;
    assert_eq!(r["echo"]["method"], "POST");
    assert_eq!(r["echo"]["body"], "data");
    let r = run(
        &executor,
        "redirect",
        &[
            ("target", &redirect(302, &echo)),
            ("method", "POST"),
            ("body", "data"),
        ],
    )
    .await;
    assert_eq!(r["echo"]["method"], "GET");
    // A streamed body cannot be replayed on 307/308.
    let r = run(
        &executor,
        "redirect",
        &[
            ("target", &redirect(307, &echo)),
            ("method", "POST"),
            ("stream", "1"),
        ],
    )
    .await;
    assert_eq!(error_code(&r), "ERR_NEXTRS_NET_REDIRECT", "{r}");

    // Credentials stay with their origin.
    let secret = r#"{"authorization":"Bearer secret","cookie":"session=1","x-other":"kept"}"#;
    let r = run(
        &executor,
        "redirect",
        &[
            ("target", &redirect(302, &format!("{other}/echo"))),
            ("headers", secret),
        ],
    )
    .await;
    assert!(r["echo"]["headers"].get("authorization").is_none(), "{r}");
    assert!(r["echo"]["headers"].get("cookie").is_none(), "{r}");
    assert_eq!(r["echo"]["headers"]["x-other"], "kept");
    let r = run(
        &executor,
        "redirect",
        &[("target", &redirect(302, &echo)), ("headers", secret)],
    )
    .await;
    assert_eq!(r["echo"]["headers"]["authorization"], "Bearer secret");
}

#[tokio::test(flavor = "multi_thread")]
async fn timeouts_and_connection_failures_are_network_errors() {
    let (base, port, state) = upstream().await;
    // A port nothing listens on.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_port = closed.local_addr().unwrap().port();
    drop(closed);
    let executor = allowed(&[port, closed_port]);

    let started = Instant::now();
    let r = run(
        &executor,
        "get",
        &[("target", &format!("{base}/slow-head?ms=10000"))],
    )
    .await;
    assert_eq!(error_code(&r), "UND_ERR_HEADERS_TIMEOUT", "{r}");
    assert!(started.elapsed() < Duration::from_secs(5));

    let r = run(
        &executor,
        "read-all",
        &[("target", &format!("{base}/stall"))],
    )
    .await;
    assert_eq!(error_code(&r), "UND_ERR_BODY_TIMEOUT", "{r}");
    assert_eq!(r["error"]["message"], "terminated");
    assert!(
        eventually(Duration::from_secs(3), async || state
            .disconnects
            .load(Ordering::SeqCst)
            >= 1)
        .await
    );

    let r = run(
        &executor,
        "get",
        &[("target", &format!("http://127.0.0.1:{closed_port}/"))],
    )
    .await;
    assert_eq!(error_code(&r), "ECONNREFUSED", "{r}");
}

#[tokio::test(flavor = "multi_thread")]
async fn abort_signals_reject_and_close_the_upstream_connection() {
    let (base, port, state) = upstream().await;
    let executor = allowed(&[port]);

    let r = run(
        &executor,
        "abort-before",
        &[("target", &format!("{base}/text"))],
    )
    .await;
    assert_eq!(r["aborted"], true);
    assert_eq!(r["error"]["message"], "custom reason");

    // While waiting for the head.
    let r = run(
        &executor,
        "abort",
        &[
            ("target", &format!("{base}/slow-head?ms=3000")),
            ("ms", "50"),
        ],
    )
    .await;
    assert_eq!(r["aborted"], true);
    assert_eq!(r["error"]["name"], "AbortError");
    assert!(r["ms"].as_u64().unwrap() < 1000, "{r}");

    // While the body streams: the reader errors, the upstream sees a
    // disconnect.
    let r = run(
        &executor,
        "abort-body",
        &[("target", &format!("{base}/hold"))],
    )
    .await;
    assert_eq!(r["errored"], true, "{r}");
    assert_eq!(r["error"]["name"], "AbortError");
    assert!(
        eventually(Duration::from_secs(3), async || state
            .disconnects
            .load(Ordering::SeqCst)
            >= 1)
        .await
    );

    // Cancelling the body closes the connection too.
    let before = state.disconnects.load(Ordering::SeqCst);
    let r = run(
        &executor,
        "cancel-body",
        &[("target", &format!("{base}/hold"))],
    )
    .await;
    assert!(r["first"].as_u64().unwrap() > 0);
    assert!(
        eventually(Duration::from_secs(3), async || state
            .disconnects
            .load(Ordering::SeqCst)
            > before)
        .await
    );
    assert!(
        eventually(Duration::from_secs(3), async || executor
            .metrics()
            .fetch
            .in_flight
            == 0)
        .await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn response_bodies_are_bounded() {
    let (base, port, _) = upstream().await;
    let executor = allowed(&[port]);
    // Within the limit.
    let r = run(
        &executor,
        "read-all",
        &[("target", &format!("{base}/big?size=3000000"))],
    )
    .await;
    assert_eq!(r["length"], 3_000_000, "{r}");
    // Declared too large: refused before the body.
    let r = run(
        &executor,
        "get",
        &[("target", &format!("{base}/big?size=5000000&cl=1"))],
    )
    .await;
    assert_eq!(error_code(&r), "ERR_NEXTRS_NET_RESPONSE_TOO_LARGE", "{r}");
    // Chunked and too large: the body errors when it crosses the limit.
    let r = run(
        &executor,
        "read-all",
        &[("target", &format!("{base}/big?size=5000000"))],
    )
    .await;
    assert_eq!(error_code(&r), "ERR_NEXTRS_NET_RESPONSE_TOO_LARGE", "{r}");
    assert_eq!(r["error"]["message"], "terminated");
}

#[tokio::test(flavor = "multi_thread")]
async fn streamed_responses_are_forwarded_as_they_arrive() {
    let (base, port, state) = upstream().await;
    let executor = allowed(&[port]);
    let mut events = executor
        .render(fetch_request(
            "stream",
            &[("target", &format!("{base}/gate"))],
            &[],
        ))
        .await
        .unwrap();
    // The first upstream chunk reaches the client while the upstream is
    // still holding the rest.
    let mut seen = Vec::new();
    while let Some(event) = events.recv().await {
        if let ResponseEvent::Chunk(chunk) = event {
            seen.extend_from_slice(&chunk);
            break;
        }
    }
    assert_eq!(seen, b"first;");
    state.gate.notify_one();
    let rest = drain(events).await;
    assert!(rest.ended);
    assert_eq!(rest.text(), "second");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_reader_stalls_the_upstream_instead_of_buffering() {
    let (base, port, state) = upstream().await;
    let executor = allowed(&[port]);
    let r = run(
        &executor,
        "slow-reader",
        &[("target", &format!("{base}/flood")), ("ms", "1500")],
    )
    .await;
    assert_eq!(r["done"], true);
    // The flood produces at memory speed; only socket buffers and one
    // chunk in the host may hold it while JavaScript does not read.
    let flooded = state.flooded.load(Ordering::SeqCst);
    assert!(
        flooded < 24 * 1024 * 1024,
        "upstream produced {flooded} bytes for a stalled reader"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fetches_left_open_end_with_their_request() {
    let (base, port, state) = upstream().await;
    let executor = allowed(&[port]);
    // A fetch that is never awaited.
    run(
        &executor,
        "orphan",
        &[("target", &format!("{base}/hold")), ("ms", "100")],
    )
    .await;
    assert!(
        eventually(Duration::from_secs(3), async || state
            .disconnects
            .load(Ordering::SeqCst)
            >= 1)
        .await
    );
    // A response whose body is never read.
    let r = run(&executor, "no-read", &[("target", &format!("{base}/hold"))]).await;
    assert_eq!(r["status"], 200);
    assert!(
        eventually(Duration::from_secs(3), async || state
            .disconnects
            .load(Ordering::SeqCst)
            >= 2)
        .await
    );
    assert!(
        eventually(Duration::from_secs(3), async || executor
            .metrics()
            .fetch
            .in_flight
            == 0)
        .await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn request_bodies_stream_to_the_upstream() {
    let (base, port, _) = upstream().await;
    let executor = allowed(&[port]);
    let r = run(
        &executor,
        "upload",
        &[
            ("target", &format!("{base}/echo")),
            ("chunks", "32"),
            ("size", "65536"),
        ],
    )
    .await;
    assert_eq!(r["status"], 200);
    assert_eq!(r["echo"]["length"], 32 * 65536);
    assert_eq!(r["echo"]["headers"]["transfer-encoding"], "chunked");
    let mut expected = sha2::Sha256::new();
    for i in 0..32u8 {
        expected.update(vec![i % 251; 65536]);
    }
    assert_eq!(r["echo"]["sha256"], hex(&expected.finalize()));
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_fetches_keep_their_request_context() {
    let (base, port, _) = upstream().await;
    let allow = vec![format!("http://127.0.0.1:{port}")];
    let executor = start(
        Some(network(&allow, &["127.0.0.1"])),
        Limits {
            isolates: 2,
            ..small_limits()
        },
    );
    let target = format!("{base}/text");
    let tasks = (0..96).map(|i| {
        let executor = executor.clone();
        let target = target.clone();
        tokio::spawn(async move {
            let id = format!("req-{i}");
            let delay = ((i * 7) % 23).to_string();
            let out = collect(
                &executor,
                fetch_request(
                    "context",
                    &[("target", &target), ("delay", &delay)],
                    &[("x-id", &id)],
                ),
            )
            .await;
            let r: Value = serde_json::from_slice(&out.body).unwrap();
            (id, r)
        })
    });
    for task in tasks {
        let (id, r) = task.await.unwrap();
        assert_eq!(r["text"], id, "{r}");
        assert_eq!(r["store"], id, "{r}");
        assert_eq!(r["header"], id, "{r}");
    }
    // Many fetches at once from one request.
    let r = run(&executor, "no-await", &[("target", &target), ("n", "50")]).await;
    assert_eq!(r["all"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn forbidden_request_headers_are_rejected() {
    let (base, port, _) = upstream().await;
    let executor = allowed(&[port]);
    for name in [
        "host",
        "connection",
        "transfer-encoding",
        "upgrade",
        "proxy-authorization",
        "keep-alive",
    ] {
        let r = run(
            &executor,
            "forbidden-header",
            &[("target", &format!("{base}/echo")), ("name", name)],
        )
        .await;
        assert_eq!(r["error"]["name"], "TypeError", "{name}: {r}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_isolate_restart_aborts_its_fetches_and_the_next_one_works() {
    let (base, port, state) = upstream().await;
    let executor = allowed(&[port]);
    // A fetch in flight (streaming, not yet finished)...
    let held = executor
        .render(fetch_request(
            "stream",
            &[("target", &format!("{base}/hold"))],
            &[],
        ))
        .await
        .unwrap();
    assert!(
        eventually(Duration::from_secs(2), async || executor
            .metrics()
            .fetch
            .in_flight
            == 1)
        .await
    );
    // ...when its isolate is terminated for stalling.
    let stalled = collect(&executor, {
        let mut r = request("faults.js", &[("x-mode", "spin"), ("x-delay", "3000")]);
        r.url = "/spin".into();
        r
    })
    .await;
    assert_ne!(stalled.status, 200);
    let held = drain(held).await;
    assert!(
        !held.ended,
        "the streaming response must be cut, not completed"
    );
    assert!(
        eventually(Duration::from_secs(3), async || state
            .disconnects
            .load(Ordering::SeqCst)
            >= 1)
        .await
    );
    assert!(
        eventually(Duration::from_secs(3), async || executor
            .metrics()
            .fetch
            .in_flight
            == 0)
        .await
    );
    // The replacement isolate fetches normally.
    let r = run(
        &executor,
        "get",
        &[("target", &format!("{base}/text?v=after"))],
    )
    .await;
    assert_eq!(r["body"], "after");
}

#[tokio::test(flavor = "multi_thread")]
async fn fetch_metrics_count_requests_failures_and_latency() {
    let (base, port, _) = upstream().await;
    let executor = allowed(&[port]);
    run(&executor, "get", &[("target", &format!("{base}/text"))]).await;
    run(&executor, "get", &[("target", "http://169.254.169.254/")]).await;
    let fetch = executor.metrics().fetch;
    assert_eq!(fetch.requests, 2);
    assert_eq!(fetch.duration_count, 1);
    let denied = fetch
        .failures
        .iter()
        .find(|(r, _)| *r == "denied")
        .unwrap()
        .1;
    assert_eq!(denied, 1);
    let text = executor.metrics().to_prometheus();
    assert!(text.contains("nextrs_fetch_failures_total{reason=\"denied\"} 1"));
    assert!(text.contains("nextrs_fetch_duration_seconds_count 1"));
    // Nothing request-derived in any label.
    assert!(!text.contains("127.0.0.1") && !text.contains("169.254"));
}

/// TLS against a real public host. Needs internet access, so it is not run
/// by default (CI uses no public services): `cargo test --test fetch -- --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs internet access"]
async fn public_https_smoke() {
    let executor = start(
        Some(network(
            &["example.com".to_owned(), "www.example.com".to_owned()],
            &[],
        )),
        small_limits(),
    );
    let r = run(&executor, "get", &[("target", "https://example.com/")]).await;
    assert_eq!(r["status"], 200, "{r}");
    assert!(r["body"].as_str().unwrap().contains("Example Domain"));
    // Certificates are verified: a host with a bad certificate fails.
    let executor = start(
        Some(network(&["expired.badssl.com".to_owned()], &[])),
        small_limits(),
    );
    let r = run(
        &executor,
        "get",
        &[("target", "https://expired.badssl.com/")],
    )
    .await;
    assert_eq!(r["error"]["message"], "fetch failed", "{r}");
}
