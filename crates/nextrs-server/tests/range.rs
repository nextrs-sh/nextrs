//! Byte-range requests for static and public files, on real Next.js build
//! output, through the application router. Behaviour recorded from next
//! start 16.4.6 (`send`) is noted where NextRS matches it; the intentional
//! differences (RFC 9110 where `send` behind Next answers 500 or a
//! compressed partial body) are in `src/range.rs` and the RC2 range report.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, Response, StatusCode};
use nextrs_server::{CompressionCache, ServerOptions};
use tower::ServiceExt;

const CHUNK: &str = "3wbm-994r45kr.js";

fn snapshot() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/next-16.4.3-default")
}

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

/// A copy of the snapshot with an empty chunk, a 6 MiB chunk and public
/// files (made before the app starts: next start lists `public/` then).
struct Fixture {
    dir: PathBuf,
    app: Router,
    cache: Arc<CompressionCache>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn large_contents() -> Vec<u8> {
    // Compressible, with a position-dependent pattern so a wrong offset
    // shows.
    (0..6 * 1024 * 1024u32)
        .map(|i| b"0123456789abcdef"[((i / 7) % 16) as usize])
        .collect()
}

async fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("nextrs-range-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot(), &dir);
    let chunks = dir.join(".next/static/chunks");
    std::fs::write(chunks.join("empty.js"), b"").unwrap();
    std::fs::write(chunks.join("large.js"), large_contents()).unwrap();
    std::fs::create_dir_all(dir.join("public/docs")).unwrap();
    std::fs::write(dir.join("public/docs/notes.txt"), notes()).unwrap();
    // Old enough for Last-Modified to be a strong validator (If-Range).
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let info = nextrs_adapter::inspect(&dir).unwrap();
    let cache = Arc::new(CompressionCache::new(16 << 20, 4 << 20));
    let options = ServerOptions {
        allow_unsupported: true,
        compression_cache: cache.clone(),
        ..ServerOptions::default()
    };
    let app = nextrs_server::app(&info, &options).await.unwrap();
    Fixture { dir, app, cache }
}

fn notes() -> Vec<u8> {
    (0..5000u32).map(|i| b'a' + (i % 26) as u8).collect()
}

fn chunk() -> Vec<u8> {
    std::fs::read(snapshot().join(".next/static/chunks").join(CHUNK)).unwrap()
}

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
) -> (Response<Body>, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap().to_vec();
    (Response::from_parts(parts, Body::empty()), bytes)
}

fn header<'a>(response: &'a Response<Body>, name: &str) -> Option<&'a str> {
    response.headers().get(name).map(|v| v.to_str().unwrap())
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .expect("a complete gzip stream");
    out
}

fn cache_stat(cache: &CompressionCache, name: &str) -> u64 {
    cache
        .snapshot()
        .into_iter()
        .find(|(n, _)| *n == name)
        .unwrap()
        .1
}

/// A 206 with a Content-Range and Content-Length naming exactly `bytes`.
fn assert_partial(
    response: &Response<Body>,
    body: &[u8],
    file: &[u8],
    start: usize,
    end: usize,
    what: &str,
) {
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT, "{what}");
    assert_eq!(
        header(response, "content-range"),
        Some(format!("bytes {start}-{end}/{}", file.len()).as_str()),
        "{what}"
    );
    assert_eq!(
        header(response, "content-length"),
        Some((end - start + 1).to_string().as_str()),
        "{what}"
    );
    assert_eq!(header(response, "accept-ranges"), Some("bytes"), "{what}");
    assert_eq!(header(response, "content-encoding"), None, "{what}");
    assert_eq!(body, &file[start..=end], "{what}");
}

/// next start: 206 for each of these, the same Content-Range.
#[tokio::test]
async fn single_ranges_are_partial_content() {
    let f = fixture("single").await;
    let file = chunk();
    let n = file.len();
    let path = format!("/_next/static/chunks/{CHUNK}");
    for (range, start, end) in [
        ("bytes=0-0".to_owned(), 0, 0),
        (format!("bytes={}-{}", n - 1, n - 1), n - 1, n - 1),
        ("bytes=1000-1999".to_owned(), 1000, 1999),
        (format!("bytes=0-{}", n - 1), 0, n - 1),
        ("bytes=0-".to_owned(), 0, n - 1),
        ("bytes=-10".to_owned(), n - 10, n - 1),
        (format!("bytes=-{}", n + 50), 0, n - 1),
        ("bytes=200000-".to_owned(), 200_000, n - 1),
        ("bytes=5-99999999999999999999".to_owned(), 5, n - 1),
    ] {
        let (response, body) = request(&f.app, Method::GET, &path, &[("range", &range)]).await;
        assert_partial(&response, &body, &file, start, end, &range);
        assert_eq!(
            header(&response, "cache-control"),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(
            header(&response, "content-type"),
            Some("application/javascript; charset=UTF-8")
        );
        assert!(header(&response, "etag").unwrap().starts_with("W/\""));
        assert!(header(&response, "last-modified").is_some());
        assert_eq!(header(&response, "vary"), Some("Accept-Encoding"));
    }
    // The full response advertises ranges now.
    let (response, body) = request(&f.app, Method::GET, &path, &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "accept-ranges"), Some("bytes"));
    assert_eq!(body, file);
}

/// next start answers these with a 500 page carrying `Content-Range:
/// bytes */<size>`; NextRS sends the 416 that Content-Range belongs to.
#[tokio::test]
async fn unsatisfiable_ranges_are_416() {
    let f = fixture("unsat").await;
    let n = chunk().len();
    let path = format!("/_next/static/chunks/{CHUNK}");
    for range in [
        format!("bytes={n}-"),
        format!("bytes={}-{}", n + 10, n + 20),
        "bytes=-0".to_owned(),
    ] {
        for accept in ["identity", "gzip"] {
            let (response, body) = request(
                &f.app,
                Method::GET,
                &path,
                &[("range", &range), ("accept-encoding", accept)],
            )
            .await;
            assert_eq!(
                response.status(),
                StatusCode::RANGE_NOT_SATISFIABLE,
                "{range}"
            );
            assert_eq!(
                header(&response, "content-range"),
                Some(format!("bytes */{n}").as_str()),
                "{range}"
            );
            // Not cacheable as the file is.
            assert_eq!(header(&response, "cache-control"), None, "{range}");
            assert_eq!(header(&response, "content-encoding"), None, "{range}");
            assert_eq!(body, b"Range Not Satisfiable");
        }
    }
}

/// Headers that select nothing usable: the full 200 (next start: the same
/// for several ranges and unknown units, a 500 for malformed ones).
#[tokio::test]
async fn ignored_ranges_get_the_full_response() {
    let f = fixture("ignored").await;
    let file = chunk();
    let path = format!("/_next/static/chunks/{CHUNK}");
    for range in [
        "bytes=abc",
        "bytes=5-2",
        "bytes=",
        "items=0-5",
        "bytes=0-0,5-9",
        "bytes=0-1,0-1,0-1",
    ] {
        let (response, body) = request(&f.app, Method::GET, &path, &[("range", range)]).await;
        assert_eq!(response.status(), StatusCode::OK, "{range}");
        assert_eq!(header(&response, "content-range"), None, "{range}");
        assert_eq!(header(&response, "accept-ranges"), Some("bytes"));
        assert_eq!(body, file, "{range}");
    }
    // Two Range fields are not one ranges-specifier.
    let mut builder = Request::builder().uri(path.as_str());
    builder = builder
        .header("range", "bytes=0-0")
        .header("range", "bytes=1-1");
    let response = f
        .app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn empty_and_missing_files() {
    let f = fixture("empty").await;
    let path = "/_next/static/chunks/empty.js";
    let (response, body) = request(&f.app, Method::GET, path, &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-length"), Some("0"));
    assert!(body.is_empty());
    for range in ["bytes=0-0", "bytes=0-", "bytes=-0"] {
        let (response, _) = request(&f.app, Method::GET, path, &[("range", range)]).await;
        assert_eq!(
            response.status(),
            StatusCode::RANGE_NOT_SATISFIABLE,
            "{range}"
        );
        assert_eq!(header(&response, "content-range"), Some("bytes */0"));
    }
    // A suffix of nothing: the (empty) full response.
    let (response, body) = request(&f.app, Method::GET, path, &[("range", "bytes=-5")]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.is_empty());
    let (response, _) = request(
        &f.app,
        Method::GET,
        "/_next/static/chunks/missing.js",
        &[("range", "bytes=0-9")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(header(&response, "content-range"), None);
}

/// A range from a large file is read alone and lands at the right offset;
/// the full gzip response of a file above the cache's entry limit is still
/// compressed (per request, not held).
#[tokio::test]
async fn large_files() {
    let f = fixture("large").await;
    let file = large_contents();
    let n = file.len();
    let path = "/_next/static/chunks/large.js";
    for (range, start, end) in [
        (format!("bytes={}-{}", n - 4096, n - 1), n - 4096, n - 1),
        ("bytes=3145721-3145800".to_owned(), 3_145_721, 3_145_800),
        ("bytes=-1".to_owned(), n - 1, n - 1),
    ] {
        let (response, body) = request(&f.app, Method::GET, path, &[("range", &range)]).await;
        assert_partial(&response, &body, &file, start, end, &range);
    }
    let (response, body) = request(&f.app, Method::GET, path, &[("accept-encoding", "gzip")]).await;
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(gunzip(&body), file);
    assert_eq!(cache_stat(&f.cache, "entries"), 0);
    assert_eq!(cache_stat(&f.cache, "bypassed"), 1);
}

/// RFC 9110 section 14.2: GET is the only method with ranges. HEAD
/// describes the full response (next start: 206 headers).
#[tokio::test]
async fn head_ignores_range() {
    let f = fixture("head").await;
    let n = chunk().len();
    let path = format!("/_next/static/chunks/{CHUNK}");
    let (response, body) = request(&f.app, Method::HEAD, &path, &[("range", "bytes=0-9")]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-range"), None);
    assert_eq!(
        header(&response, "content-length"),
        Some(n.to_string().as_str())
    );
    assert_eq!(header(&response, "accept-ranges"), Some("bytes"));
    assert!(body.is_empty());
    let (response, _) = request(&f.app, Method::POST, &path, &[("range", "bytes=0-9")]).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

/// If-Range compares strongly: the file's own weak ETag does not match
/// (next start: it does), its Last-Modified does when exactly equal.
/// If-None-Match is evaluated first: a 304 whatever the range.
#[tokio::test]
async fn conditional_range_requests() {
    let f = fixture("conditional").await;
    let file = chunk();
    let path = format!("/_next/static/chunks/{CHUNK}");
    let (full, _) = request(&f.app, Method::GET, &path, &[]).await;
    let etag = header(&full, "etag").unwrap().to_owned();
    let modified = header(&full, "last-modified").unwrap().to_owned();
    let later = httpdate::fmt_http_date(
        httpdate::parse_http_date(&modified).unwrap() + std::time::Duration::from_secs(1),
    );
    for (if_range, partial) in [
        (modified.as_str(), true),
        (later.as_str(), false),
        (etag.as_str(), false),
        (etag.trim_start_matches("W/"), false),
        ("\"other\"", false),
        ("not a date", false),
    ] {
        let (response, body) = request(
            &f.app,
            Method::GET,
            &path,
            &[("range", "bytes=10-19"), ("if-range", if_range)],
        )
        .await;
        if partial {
            assert_partial(&response, &body, &file, 10, 19, if_range);
        } else {
            assert_eq!(response.status(), StatusCode::OK, "{if_range}");
            assert_eq!(body, file, "{if_range}");
        }
    }
    // If-None-Match / If-Modified-Since win over Range.
    for (name, value) in [
        ("if-none-match", etag.as_str()),
        ("if-modified-since", modified.as_str()),
    ] {
        let (response, body) = request(
            &f.app,
            Method::GET,
            &path,
            &[("range", "bytes=0-9"), (name, value)],
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED, "{name}");
        assert!(body.is_empty());
    }
    let (response, body) = request(
        &f.app,
        Method::GET,
        &path,
        &[("range", "bytes=0-9"), ("if-none-match", "\"x\"")],
    )
    .await;
    assert_partial(&response, &body, &file, 0, 9, "if-none-match mismatch");
}

/// With gzip accepted, a range is served from the identity bytes (never a
/// compressed partial body: next start compresses it, so its Content-Range
/// describes other bytes than it sends); the cached gzip copy never
/// answers a range.
#[tokio::test]
async fn ranges_with_gzip_use_the_identity_bytes() {
    let f = fixture("gzip").await;
    let file = chunk();
    let path = format!("/_next/static/chunks/{CHUNK}");
    let gzip_full = || async {
        let (response, body) =
            request(&f.app, Method::GET, &path, &[("accept-encoding", "gzip")]).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header(&response, "content-encoding"), Some("gzip"));
        assert_eq!(header(&response, "accept-ranges"), None);
        assert_eq!(gunzip(&body), file);
        body
    };
    let range = || async {
        let (response, body) = request(
            &f.app,
            Method::GET,
            &path,
            &[
                ("accept-encoding", "gzip, deflate, br"),
                ("range", "bytes=100-2099"),
            ],
        )
        .await;
        assert_partial(&response, &body, &file, 100, 2099, "gzip range");
        assert_eq!(header(&response, "vary"), Some("Accept-Encoding"));
    };
    range().await;
    let first = gzip_full().await;
    // After warm-up: the range still gets identity bytes, the full response
    // the cached copy.
    range().await;
    assert_eq!(gzip_full().await, first);
    assert_eq!(cache_stat(&f.cache, "misses"), 1);
    assert_eq!(cache_stat(&f.cache, "hits"), 1);
    // A resumed download it may have received compressed: If-Range is not
    // trusted, the full gzip response again.
    let (full, _) = request(&f.app, Method::GET, &path, &[]).await;
    let modified = header(&full, "last-modified").unwrap().to_owned();
    let (response, body) = request(
        &f.app,
        Method::GET,
        &path,
        &[
            ("accept-encoding", "gzip"),
            ("range", "bytes=0-9"),
            ("if-range", &modified),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(gunzip(&body), file);
    // Revalidating with the ETag of either representation: 304.
    let (response, _) = request(
        &f.app,
        Method::GET,
        &path,
        &[
            ("accept-encoding", "gzip"),
            ("range", "bytes=0-9"),
            ("if-none-match", header(&full, "etag").unwrap()),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
}

/// Public files (next start: `send` too) take ranges; they are never put
/// in the compression cache.
#[tokio::test]
async fn public_files() {
    let f = fixture("public").await;
    let file = notes();
    let (response, body) = request(
        &f.app,
        Method::GET,
        "/docs/notes.txt",
        &[("range", "bytes=26-51")],
    )
    .await;
    assert_partial(&response, &body, &file, 26, 51, "public");
    assert_eq!(
        header(&response, "cache-control"),
        Some("public, max-age=0")
    );
    let (response, body) = request(
        &f.app,
        Method::GET,
        "/docs/notes.txt",
        &[("accept-encoding", "gzip")],
    )
    .await;
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(gunzip(&body), file);
    assert_eq!(cache_stat(&f.cache, "entries"), 0);
}

/// Ranges, identity and gzip requests for one file at once: each answer is
/// complete and from the right bytes; the file is compressed once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_range_and_full_requests() {
    let f = fixture("concurrent").await;
    let file = Arc::new(chunk());
    let path = format!("/_next/static/chunks/{CHUNK}");
    let mut tasks = Vec::new();
    for i in 0..96usize {
        let app = f.app.clone();
        let path = path.clone();
        let file = file.clone();
        tasks.push(tokio::spawn(async move {
            match i % 3 {
                0 => {
                    let start = (i * 997) % (file.len() - 100);
                    let range = format!("bytes={start}-{}", start + 99);
                    let (response, body) = request(
                        &app,
                        Method::GET,
                        &path,
                        &[("range", &range), ("accept-encoding", "gzip")],
                    )
                    .await;
                    assert_partial(&response, &body, &file, start, start + 99, &range);
                }
                1 => {
                    let (response, body) =
                        request(&app, Method::GET, &path, &[("accept-encoding", "gzip")]).await;
                    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
                    assert_eq!(gunzip(&body), *file);
                }
                _ => {
                    let (response, body) = request(&app, Method::GET, &path, &[]).await;
                    assert_eq!(response.status(), StatusCode::OK);
                    assert_eq!(body, *file);
                }
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    assert_eq!(
        cache_stat(&f.cache, "misses"),
        1,
        "{:?}",
        f.cache.snapshot()
    );
    assert_eq!(
        cache_stat(&f.cache, "hits") + cache_stat(&f.cache, "coalesced"),
        31
    );
}

/// Prerendered pages do not take ranges (next start: the full 200).
#[tokio::test]
async fn pages_ignore_range() {
    let f = fixture("pages").await;
    let html = std::fs::read(snapshot().join(".next/server/app/index.html")).unwrap();
    let (response, body) = request(&f.app, Method::GET, "/", &[("range", "bytes=0-9")]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-range"), None);
    assert_eq!(header(&response, "accept-ranges"), None);
    assert_eq!(body, html);
}
