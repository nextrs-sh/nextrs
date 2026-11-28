//! The native buffer budget (RE-4C, RE-4.1A) on static files, against a
//! real Next.js build snapshot: a response holds its file's bytes until its
//! body is dropped; a request that finds no room waits, then gets a 503
//! with Retry-After; HEAD and 304 reserve nothing; a range reserves only
//! itself; a file above the whole-read limit is streamed in chunks that fit
//! the budget, with the same responses as without a budget (full, ranges,
//! conditionals, HEAD, gzip), under concurrency, slow clients, disconnects
//! and files changing while they are sent; nothing leaks.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use nextrs_server::{NativeBufferBudget, ServerOptions};
use tower::ServiceExt;

/// 10030 bytes (W/"272e-…"), 10 KiB of budget.
const CHUNK: &str = "/_next/static/chunks/turbopack-3-ry682tkyiqk.js";
const CHUNK_LEN: usize = 0x272e;

fn snapshot() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/next-16.4.3-default")
}

async fn app(budget: &Arc<NativeBufferBudget>) -> Router {
    let info = nextrs_adapter::inspect(&snapshot()).unwrap();
    nextrs_server::app(
        &info,
        &ServerOptions {
            native_buffer_budget: Some(budget.clone()),
            ..ServerOptions::default()
        },
    )
    .await
    .unwrap()
}

async fn call(app: &Router, request: Request<Body>) -> axum::response::Response {
    app.clone().oneshot(request).await.unwrap()
}

/// Reads a body as hyper does: one frame at a time, each dropped once
/// "written" (copied out). Collecting it (`to_bytes`) would keep every
/// chunk, and the budgeted bytes with them.
async fn drain(body: Body) -> Result<Vec<u8>, axum::Error> {
    use futures_util::StreamExt;
    let mut stream = body.into_data_stream();
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        out.extend_from_slice(&chunk?);
    }
    Ok(out)
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

#[tokio::test]
async fn a_response_holds_its_bytes_until_its_body_is_dropped() {
    let budget = NativeBufferBudget::new(64 * 1024, Duration::from_millis(50));
    let app = app(&budget).await;
    let response = call(&app, get(CHUNK)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(budget.reserved_bytes(), 10 * 1024);
    let body = drain(response.into_body()).await.unwrap();
    assert_eq!(body.len(), CHUNK_LEN);
    assert_eq!(budget.reserved_bytes(), 0, "released once the body is gone");
}

/// Four whole reads of CHUNK (10 KiB each) fill a 40 KiB budget, whose
/// whole-read limit is exactly 10 KiB.
async fn fill(app: &Router) -> Vec<axum::response::Response> {
    let mut held = Vec::new();
    for _ in 0..4 {
        let response = call(app, get(CHUNK)).await;
        assert_eq!(response.status(), StatusCode::OK);
        held.push(response);
    }
    held
}

#[tokio::test]
async fn no_room_waits_then_answers_503_and_room_again_serves() {
    let budget = NativeBufferBudget::new(40 * 1024, Duration::from_millis(50));
    let app = app(&budget).await;
    let mut held = fill(&app).await;
    assert_eq!(budget.reserved_bytes(), 40 * 1024);
    let refused = call(&app, get(CHUNK)).await;
    assert_eq!(refused.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(refused.headers()["retry-after"], "1");
    assert_eq!(refused.headers()["x-nextrs-error"], "native-buffer-budget");
    // A waiter is served as soon as the held body goes away.
    let waiting = {
        let app = app.clone();
        tokio::spawn(async move { call(&app, get(CHUNK)).await.status() })
    };
    tokio::time::sleep(Duration::from_millis(10)).await;
    held.pop();
    assert_eq!(waiting.await.unwrap(), StatusCode::OK);
    drop(held);
    assert_eq!(budget.reserved_bytes(), 0);
    assert!(
        budget
            .to_prometheus()
            .contains("nextrs_native_buffer_refused_total{kind=\"static_file\"} 1")
    );
}

#[tokio::test]
async fn head_and_not_modified_reserve_nothing() {
    let budget = NativeBufferBudget::new(40 * 1024, Duration::from_millis(20));
    let app = app(&budget).await;
    let held = fill(&app).await;
    let etag = held[0].headers()["etag"].clone();
    let head = call(&app, Request::head(CHUNK).body(Body::empty()).unwrap()).await;
    assert_eq!(head.status(), StatusCode::OK);
    let revalidated = call(
        &app,
        Request::get(CHUNK)
            .header("if-none-match", etag)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(revalidated.status(), StatusCode::NOT_MODIFIED);
    drop(held);
    assert_eq!(budget.reserved_bytes(), 0);
}

#[tokio::test]
async fn a_range_reserves_only_its_bytes() {
    // Whole-read limit 11 KiB: four whole reads hold 40 KiB, 4 KiB left.
    let budget = NativeBufferBudget::new(44 * 1024, Duration::from_millis(20));
    let app = app(&budget).await;
    let held = fill(&app).await;
    assert_eq!(
        call(&app, get(CHUNK)).await.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "a whole read does not fit"
    );
    let partial = call(
        &app,
        Request::get(CHUNK)
            .header("range", "bytes=0-999")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
    let body = drain(partial.into_body()).await.unwrap();
    assert_eq!(body.len(), 1000);
    drop(held);
    assert_eq!(budget.reserved_bytes(), 0);
}

#[tokio::test]
async fn a_file_larger_than_the_budget_is_streamed_within_it() {
    // RE-4 served it alone, holding the whole file and the whole budget.
    let budget = NativeBufferBudget::new(4 * 1024, Duration::from_millis(20));
    let app = app(&budget).await;
    let response = call(&app, get(CHUNK)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-length"],
        CHUNK_LEN.to_string().as_str()
    );
    assert_eq!(budget.reserved_bytes(), 1024, "only the first 1 KiB chunk");
    // Two more streams fit beside it.
    let others = [call(&app, get(CHUNK)).await, call(&app, get(CHUNK)).await];
    assert!(others.iter().all(|r| r.status() == StatusCode::OK));
    assert!(budget.reserved_bytes() <= 4 * 1024);
    let expected = std::fs::read(snapshot().join(".next").join(&CHUNK[7..])).unwrap();
    let body = drain(response.into_body()).await.unwrap();
    assert_eq!(&body[..], &expected[..]);
    for other in others {
        let body = drain(other.into_body()).await.unwrap();
        assert_eq!(&body[..], &expected[..]);
    }
    assert_eq!(budget.reserved_bytes(), 0);
}

#[tokio::test]
async fn many_concurrent_requests_stay_within_the_budget_and_release_everything() {
    let budget = NativeBufferBudget::new(40 * 1024, Duration::from_secs(5));
    let app = app(&budget).await;
    let tasks: Vec<_> = (0..64)
        .map(|_| {
            let app = app.clone();
            let budget = budget.clone();
            tokio::spawn(async move {
                let response = call(&app, get(CHUNK)).await;
                assert!(budget.reserved_bytes() <= 40 * 1024);
                let status = response.status();
                let body = drain(response.into_body()).await.unwrap();
                (status, body.len())
            })
        })
        .collect();
    for task in tasks {
        assert_eq!(task.await.unwrap(), (StatusCode::OK, CHUNK_LEN));
    }
    assert_eq!(budget.reserved_bytes(), 0);
}

#[tokio::test]
async fn without_a_budget_nothing_changes() {
    let info = nextrs_adapter::inspect(&snapshot()).unwrap();
    let app = nextrs_server::app(&info, &ServerOptions::default())
        .await
        .unwrap();
    let response = call(&app, get(CHUNK)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-length"],
        CHUNK_LEN.to_string().as_str()
    );
}

// ---- Large files (RE-4.1A) ----------------------------------------------

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

const LARGE: &str = "/_next/static/chunks/large.js";
const LARGE_LEN: usize = 6 * 1024 * 1024 + 123;
const PUBLIC: &str = "/media/big.bin";
const PUBLIC_LEN: usize = 3 * 1024 * 1024 + 5;

/// Compressible, position-dependent (a wrong offset shows).
fn large_contents() -> Vec<u8> {
    (0..LARGE_LEN as u32)
        .map(|i| b"0123456789abcdef"[((i / 7) % 16) as usize])
        .collect()
}

/// Not compressible by type (`application/octet-stream`).
fn public_contents() -> Vec<u8> {
    (0..PUBLIC_LEN as u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect()
}

/// A copy of the snapshot with a 6 MiB chunk and a 3 MiB public file.
struct Large {
    dir: PathBuf,
}

impl Drop for Large {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn large(name: &str) -> Large {
    let dir = std::env::temp_dir().join(format!("nextrs-native-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot(), &dir);
    std::fs::write(dir.join(".next/static/chunks/large.js"), large_contents()).unwrap();
    std::fs::create_dir_all(dir.join("public/media")).unwrap();
    std::fs::write(dir.join("public/media/big.bin"), public_contents()).unwrap();
    // Old enough for Last-Modified to be a strong validator (If-Range).
    std::thread::sleep(Duration::from_millis(1100));
    Large { dir }
}

async fn app_on(dir: &Path, budget: Option<Arc<NativeBufferBudget>>) -> Router {
    let info = nextrs_adapter::inspect(dir).unwrap();
    nextrs_server::app(
        &info,
        &ServerOptions {
            native_buffer_budget: budget,
            ..ServerOptions::default()
        },
    )
    .await
    .unwrap()
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .expect("a complete gzip stream");
    out
}

/// Status, headers that must match, and the decoded body.
async fn observe(app: &Router, request: Request<Body>) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let response = call(app, request).await;
    let status = response.status().as_u16();
    let mut headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .filter(|(name, _)| *name != "date")
        .map(|(n, v)| (n.to_string(), v.to_str().unwrap().to_owned()))
        .collect();
    headers.sort();
    let gzip = response
        .headers()
        .get("content-encoding")
        .is_some_and(|v| v == "gzip");
    let body = drain(response.into_body()).await.unwrap();
    let body = if gzip { gunzip(&body) } else { body };
    (status, headers, body)
}

/// A request variant: name, method, headers.
type Case = (&'static str, Method, Vec<(&'static str, String)>);

#[tokio::test]
async fn streamed_responses_match_the_unbudgeted_server() {
    let fixture = large("parity");
    // 64 KiB: 16 KiB chunks; both files are far above the whole-read limit.
    let budget = NativeBufferBudget::new(64 * 1024, Duration::from_millis(200));
    let plain = app_on(&fixture.dir, None).await;
    let budgeted = app_on(&fixture.dir, Some(budget.clone())).await;
    let probe = call(&plain, get(LARGE)).await;
    let etag = probe.headers()["etag"].to_str().unwrap().to_owned();
    let modified = probe.headers()["last-modified"]
        .to_str()
        .unwrap()
        .to_owned();
    drop(probe);
    let cases: Vec<Case> = vec![
        ("full", Method::GET, vec![]),
        (
            "gzip",
            Method::GET,
            vec![("accept-encoding", "gzip".into())],
        ),
        ("head", Method::HEAD, vec![]),
        (
            "range",
            Method::GET,
            vec![("range", "bytes=1000-2000000".into())],
        ),
        (
            "suffix",
            Method::GET,
            vec![("range", "bytes=-70000".into())],
        ),
        (
            "open",
            Method::GET,
            vec![("range", "bytes=6000000-".into())],
        ),
        (
            "unsatisfiable",
            Method::GET,
            vec![("range", "bytes=99999999-".into())],
        ),
        (
            "multi",
            Method::GET,
            vec![("range", "bytes=0-1,5-9".into())],
        ),
        (
            "not-modified",
            Method::GET,
            vec![("if-none-match", etag.clone())],
        ),
        (
            "modified-since",
            Method::GET,
            vec![("if-modified-since", modified.clone())],
        ),
        (
            "precondition",
            Method::GET,
            vec![("if-match", "\"other\"".into())],
        ),
        (
            "if-range-etag",
            Method::GET,
            vec![
                ("range", "bytes=10-99999".into()),
                ("if-range", etag.clone()),
            ],
        ),
        (
            "if-range-date",
            Method::GET,
            vec![
                ("range", "bytes=10-99999".into()),
                ("if-range", modified.clone()),
            ],
        ),
        (
            "range-with-gzip",
            Method::GET,
            vec![
                ("range", "bytes=10-99999".into()),
                ("accept-encoding", "gzip".into()),
            ],
        ),
    ];
    for uri in [LARGE, PUBLIC] {
        for (name, method, headers) in &cases {
            let build = || {
                let mut builder = Request::builder().method(method.clone()).uri(uri);
                for (header, value) in headers {
                    builder = builder.header(*header, value.as_str());
                }
                builder.body(Body::empty()).unwrap()
            };
            let expected = observe(&plain, build()).await;
            let actual = observe(&budgeted, build()).await;
            assert_eq!(actual.0, expected.0, "{uri} {name}: status");
            assert_eq!(actual.1, expected.1, "{uri} {name}: headers");
            assert!(actual.2 == expected.2, "{uri} {name}: body differs");
            assert_eq!(budget.reserved_bytes(), 0, "{uri} {name}: released");
        }
    }
    let text = budget.to_prometheus();
    assert!(
        !text.contains("nextrs_native_buffer_reserved_bytes_total{kind=\"static_stream\"} 0"),
        "{text}"
    );
}

/// The highest reservation seen while `work` runs (sampled every 200 us).
async fn peak_during<F: std::future::Future>(
    budget: &Arc<NativeBufferBudget>,
    work: F,
) -> (u64, F::Output) {
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sampler = {
        let (budget, done) = (budget.clone(), done.clone());
        tokio::spawn(async move {
            let mut peak = 0;
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                peak = peak.max(budget.reserved_bytes());
                tokio::time::sleep(Duration::from_micros(200)).await;
            }
            peak
        })
    };
    let output = work.await;
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    let peak = sampler.await.unwrap().max(budget.reserved_bytes());
    (peak, output)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_full_and_range_requests_stay_within_the_budget() {
    let fixture = large("concurrent");
    let budget = NativeBufferBudget::new(128 * 1024, Duration::from_secs(10));
    let app = app_on(&fixture.dir, Some(budget.clone())).await;
    let contents = Arc::new(large_contents());
    let (peak, ()) = peak_during(&budget, async {
        let tasks: Vec<_> = (0..32usize)
            .map(|i| {
                let (app, contents) = (app.clone(), contents.clone());
                tokio::spawn(async move {
                    let mut builder = Request::get(LARGE);
                    let (start, end) = (i * 150_001, i * 150_001 + 300_000 + i);
                    match i % 4 {
                        0 => {}
                        1 => builder = builder.header("accept-encoding", "gzip"),
                        _ => builder = builder.header("range", format!("bytes={start}-{end}")),
                    }
                    let (status, _, body) =
                        observe(&app, builder.body(Body::empty()).unwrap()).await;
                    if i % 4 < 2 {
                        assert_eq!(status, 200);
                        assert!(body == *contents, "request {i}: full body");
                    } else {
                        assert_eq!(status, 206);
                        assert!(body == contents[start..=end], "request {i}: range");
                    }
                })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
    })
    .await;
    assert!(peak <= 128 * 1024, "peak {peak}");
    assert!(peak > 0);
    assert_eq!(budget.reserved_bytes(), 0);
}

#[tokio::test]
async fn a_file_truncated_or_replaced_while_streaming() {
    let fixture = large("changing");
    let budget = NativeBufferBudget::new(64 * 1024, Duration::from_millis(200));
    let app = app_on(&fixture.dir, Some(budget.clone())).await;
    let path = fixture.dir.join(".next/static/chunks/large.js");

    // Replaced by a rename: the open handle keeps sending the old file.
    let response = call(&app, get(LARGE)).await;
    let staged = fixture.dir.join("replacement.js");
    std::fs::write(&staged, b"replaced").unwrap();
    std::fs::rename(&staged, &path).unwrap();
    let body = drain(response.into_body()).await.unwrap();
    assert!(body == large_contents(), "the file as it was opened");
    let replaced = observe(&app, get(LARGE)).await;
    assert_eq!(replaced.2, b"replaced", "later requests see the new file");

    // Truncated in place: the body fails instead of sending other bytes
    // (hyper then closes the connection).
    std::fs::write(&path, large_contents()).unwrap();
    let response = call(&app, get(LARGE)).await;
    assert_eq!(
        response.headers()["content-length"],
        LARGE_LEN.to_string().as_str()
    );
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(100_000)
        .unwrap();
    assert!(drain(response.into_body()).await.is_err());
    assert_eq!(budget.reserved_bytes(), 0);
}

// ---- Over real connections ---------------------------------------------

/// Sends a GET, returns the declared Content-Length and the socket
/// positioned at the body.
async fn open_get(
    addr: std::net::SocketAddr,
    path: &str,
) -> (usize, tokio::io::BufReader<tokio::net::TcpStream>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut reader = tokio::io::BufReader::with_capacity(4096, stream);
    let mut length = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = Some(value.trim().parse().unwrap());
        }
        if line.starts_with("HTTP/1.1") {
            assert!(line.starts_with("HTTP/1.1 200"), "{line}");
        }
    }
    (length.expect("a Content-Length"), reader)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slow_clients_and_disconnects_hold_at_most_the_budget() {
    use tokio::io::AsyncReadExt;
    let fixture = large("sockets");
    let budget = NativeBufferBudget::new(256 * 1024, Duration::from_secs(10));
    let app = app_on(&fixture.dir, Some(budget.clone())).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(nextrs_server::serve(
        listener,
        app,
        nextrs_server::ServeOptions::default(),
        async {
            let _ = stopped.await;
        },
    ));
    let contents = Arc::new(large_contents());
    let (peak, ()) = peak_during(&budget, async {
        let mut tasks = Vec::new();
        // Slow readers: 32 KiB every 10 ms (about 3 MiB/s).
        for _ in 0..8 {
            let contents = contents.clone();
            tasks.push(tokio::spawn(async move {
                let (length, mut reader) = open_get(addr, LARGE).await;
                assert_eq!(length, LARGE_LEN);
                let mut body = Vec::with_capacity(length);
                let mut buf = vec![0; 32 * 1024];
                while body.len() < length {
                    let n = reader.read(&mut buf).await.unwrap();
                    assert!(n > 0, "closed early at {}", body.len());
                    body.extend_from_slice(&buf[..n]);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert!(body == *contents);
            }));
        }
        // Clients that read 100 KiB and hang up.
        for _ in 0..8 {
            tasks.push(tokio::spawn(async move {
                let (_, mut reader) = open_get(addr, LARGE).await;
                let mut buf = vec![0; 100 * 1024];
                reader.read_exact(&mut buf).await.unwrap();
            }));
        }
        // Clients that never read their body at all, then hang up.
        for _ in 0..4 {
            tasks.push(tokio::spawn(async move {
                let (_, reader) = open_get(addr, PUBLIC).await;
                tokio::time::sleep(Duration::from_millis(300)).await;
                drop(reader);
            }));
        }
        // Meanwhile a fast client is served in full.
        let contents = contents.clone();
        tasks.push(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let (length, mut reader) = open_get(addr, LARGE).await;
            let mut body = vec![0; length];
            reader.read_exact(&mut body).await.unwrap();
            assert!(body == *contents);
        }));
        for task in tasks {
            task.await.unwrap();
        }
    })
    .await;
    assert!(peak <= 256 * 1024, "peak {peak}");
    // Hung-up connections are noticed on their next write; give them time
    // (bounded), then everything must be back.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while budget.reserved_bytes() != 0 && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(budget.reserved_bytes(), 0);
    let _ = stop.send(());
    server.await.unwrap().unwrap();
}
