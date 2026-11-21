//! Response compression on real Next.js build output (the 16.4.3 default
//! and 16.4.6 deployment-ID snapshots), through the application router.
//! Expected values marked "next start" were recorded from next start 16.4.6
//! (the `compression` 1.7 middleware) serving fixtures/next-app.

use std::io::Read;
use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, Response, StatusCode};
use std::sync::Arc;

use nextrs_server::{CompressionCache, ServerOptions};
use tower::ServiceExt;

const RSC_VARY: &str =
    "rsc, next-router-state-tree, next-router-prefetch, next-router-segment-prefetch";

fn snapshot(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data")
        .join(name)
}

fn options() -> ServerOptions {
    ServerOptions {
        allow_unsupported: true,
        ..ServerOptions::default()
    }
}

async fn app_for(dir: &Path) -> Router {
    let info = nextrs_adapter::inspect(dir).unwrap();
    nextrs_server::app(&info, &options()).await.unwrap()
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

fn read(snapshot_name: &str, rel: &str) -> Vec<u8> {
    std::fs::read(snapshot(snapshot_name).join(".next").join(rel)).unwrap()
}

const DEFAULT: &str = "next-16.4.3-default";
const DPL: &str = "next-16.4.6-deployment-id";

/// next start: `Accept-Encoding: gzip` on a prerendered page → gzip, Vary
/// keeps the RSC fields and adds Accept-Encoding. NextRS also weakens the
/// strong ETag (next start keeps it as is), and sends the Content-Length of
/// its cached gzip copy (next start streams it chunked).
#[tokio::test]
async fn prerendered_html_is_gzipped_for_gzip_clients() {
    let app = app_for(&snapshot(DEFAULT)).await;
    let html = read(DEFAULT, "server/app/index.html");
    let (response, body) = request(
        &app,
        Method::GET,
        "/",
        &[("accept-encoding", "gzip, deflate, br, zstd")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(
        header(&response, "content-length"),
        Some(body.len().to_string().as_str())
    );
    assert_eq!(
        header(&response, "vary"),
        Some(format!("{RSC_VARY}, Accept-Encoding").as_str())
    );
    assert_eq!(header(&response, "etag"), Some("W/\"i77w8p2mwj4lr\""));
    assert_eq!(
        header(&response, "cache-control"),
        Some("s-maxage=31536000")
    );
    assert_eq!(
        header(&response, "content-type"),
        Some("text/html; charset=utf-8")
    );
    assert!(body.len() < html.len());
    assert_eq!(gunzip(&body), html);
}

/// next start: identity, no Accept-Encoding, `br` alone, `gzip;q=0` → the
/// identity response with its Content-Length and strong ETag, and Vary
/// with Accept-Encoding all the same.
#[tokio::test]
async fn other_clients_get_identity_with_vary() {
    let app = app_for(&snapshot(DEFAULT)).await;
    let html = read(DEFAULT, "server/app/index.html");
    for accept in [
        None,
        Some("identity"),
        Some("br"),
        Some("gzip;q=0"),
        Some("deflate"),
        Some(""),
    ] {
        let headers: Vec<(&str, &str)> = accept.iter().map(|a| ("accept-encoding", *a)).collect();
        let (response, body) = request(&app, Method::GET, "/", &headers).await;
        assert_eq!(response.status(), StatusCode::OK, "{accept:?}");
        assert_eq!(header(&response, "content-encoding"), None, "{accept:?}");
        assert_eq!(
            header(&response, "content-length"),
            Some("5967"),
            "{accept:?}"
        );
        assert_eq!(
            header(&response, "etag"),
            Some("\"i77w8p2mwj4lr\""),
            "{accept:?}"
        );
        assert_eq!(
            header(&response, "vary"),
            Some(format!("{RSC_VARY}, Accept-Encoding").as_str()),
            "{accept:?}"
        );
        assert_eq!(body, html, "{accept:?}");
    }
}

/// next start: HEAD is never compressed (Content-Length of the identity
/// body, no body).
#[tokio::test]
async fn head_is_not_compressed() {
    let app = app_for(&snapshot(DEFAULT)).await;
    let (response, body) = request(&app, Method::HEAD, "/", &[("accept-encoding", "gzip")]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-encoding"), None);
    assert_eq!(header(&response, "content-length"), Some("5967"));
    assert_eq!(header(&response, "etag"), Some("\"i77w8p2mwj4lr\""));
    assert!(body.is_empty());
}

/// Revalidation: the weak ETag of the gzip response and the strong one of
/// the identity response both get a 304 (If-None-Match compares weakly),
/// with no body and no Content-Encoding.
#[tokio::test]
async fn conditional_requests_match_either_representation() {
    let app = app_for(&snapshot(DEFAULT)).await;
    for tag in ["W/\"i77w8p2mwj4lr\"", "\"i77w8p2mwj4lr\""] {
        for accept in ["gzip", "identity"] {
            let (response, body) = request(
                &app,
                Method::GET,
                "/",
                &[("accept-encoding", accept), ("if-none-match", tag)],
            )
            .await;
            assert_eq!(
                response.status(),
                StatusCode::NOT_MODIFIED,
                "{tag} {accept}"
            );
            assert_eq!(
                header(&response, "content-encoding"),
                None,
                "{tag} {accept}"
            );
            assert!(body.is_empty(), "{tag} {accept}");
        }
    }
    let (response, _) = request(
        &app,
        Method::GET,
        "/",
        &[("accept-encoding", "gzip"), ("if-none-match", "\"other\"")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
}

/// RSC payloads (text/x-component) are compressed and keep the RSC Vary
/// fields; the deployment ID header is untouched.
#[tokio::test]
async fn rsc_payloads_are_compressed() {
    let app = app_for(&snapshot(DPL)).await;
    let rsc = read(DPL, "server/app/index.rsc");
    let (response, body) = request(
        &app,
        Method::GET,
        "/?_rsc",
        &[("rsc", "1"), ("accept-encoding", "gzip")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-type"), Some("text/x-component"));
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(
        header(&response, "x-nextjs-deployment-id"),
        Some("dpl-rc2-test")
    );
    assert_eq!(
        header(&response, "vary"),
        Some(format!("{RSC_VARY}, Accept-Encoding").as_str())
    );
    assert_eq!(gunzip(&body), rsc);
}

/// Static JavaScript is compressed whole. NextRS does not implement Range
/// requests (it answers the full 200 and advertises no Accept-Ranges; next
/// start answers 206, a difference that predates compression): that full
/// response is compressed too. Partial responses themselves are never
/// compressed (compress.rs unit tests).
#[tokio::test]
async fn static_assets_and_ranges() {
    let app = app_for(&snapshot(DPL)).await;
    let path = "/_next/static/chunks/310vm2bl3xxpt.js";
    let file = read(DPL, "static/chunks/310vm2bl3xxpt.js");
    let (response, body) = request(&app, Method::GET, path, &[("accept-encoding", "gzip")]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(header(&response, "accept-ranges"), None);
    assert_eq!(header(&response, "vary"), Some("Accept-Encoding"));
    let etag = header(&response, "etag").unwrap().to_owned();
    assert!(etag.starts_with("W/"), "{etag}");
    assert_eq!(gunzip(&body), file);

    let (response, body) = request(
        &app,
        Method::GET,
        path,
        &[("accept-encoding", "gzip"), ("range", "bytes=0-2047")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-range"), None);
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(gunzip(&body), file);

    // Revalidating the compressed copy with its weak ETag.
    let (response, _) = request(
        &app,
        Method::GET,
        path,
        &[("accept-encoding", "gzip"), ("if-none-match", &etag)],
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
}

/// next start: a body of known size under 1 KiB is sent as is (Vary all
/// the same).
#[tokio::test]
async fn small_bodies_are_not_compressed() {
    let app = app_for(&snapshot(DPL)).await;
    let build = String::from_utf8(read(DPL, "BUILD_ID")).unwrap();
    let path = format!("/_next/static/{build}/_ssgManifest.js");
    let file = read(DPL, &format!("static/{build}/_ssgManifest.js"));
    assert!(file.len() < 1024);
    let (response, body) = request(&app, Method::GET, &path, &[("accept-encoding", "gzip")]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-encoding"), None);
    assert_eq!(header(&response, "vary"), Some("Accept-Encoding"));
    assert_eq!(body, file);
}

/// A 404 page is compressed like any HTML (next start: 404, gzip).
#[tokio::test]
async fn not_found_pages_are_compressed() {
    let app = app_for(&snapshot(DPL)).await;
    let (response, body) = request(
        &app,
        Method::GET,
        "/no-such-page",
        &[("accept-encoding", "gzip")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(gunzip(&body), read(DPL, "server/app/_not-found.html"));
}

/// `compress: false` in next.config: no compression and no Vary added, as
/// next start does without its middleware.
#[tokio::test]
async fn compress_false_turns_it_off() {
    let dir = std::env::temp_dir().join(format!("nextrs-server-nocompress-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot(DEFAULT), &dir);
    let path = dir.join(".next/required-server-files.json");
    let mut files: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    files["config"]["compress"] = serde_json::Value::Bool(false);
    std::fs::write(&path, serde_json::to_vec(&files).unwrap()).unwrap();
    let info = nextrs_adapter::inspect(&dir).unwrap();
    assert!(!info.config.compress);
    let app = nextrs_server::app(&info, &options()).await.unwrap();
    let (response, body) = request(&app, Method::GET, "/", &[("accept-encoding", "gzip")]).await;
    assert_eq!(header(&response, "content-encoding"), None);
    assert_eq!(header(&response, "vary"), Some(RSC_VARY));
    assert_eq!(header(&response, "content-length"), Some("5967"));
    assert_eq!(body, read(DEFAULT, "server/app/index.html"));
    assert!(
        nextrs_adapter::inspect(&snapshot(DEFAULT))
            .unwrap()
            .config
            .compress
    );
    std::fs::remove_dir_all(&dir).unwrap();
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

// ---------------------------------------------------------------------
// The gzip cache of immutable responses.

fn cache_stat(cache: &CompressionCache, name: &str) -> u64 {
    cache
        .snapshot()
        .into_iter()
        .find(|(n, _)| *n == name)
        .unwrap()
        .1
}

async fn app_with_cache(dir: &Path, cache: &Arc<CompressionCache>) -> Router {
    let info = nextrs_adapter::inspect(dir).unwrap();
    let options = ServerOptions {
        compression_cache: cache.clone(),
        ..options()
    };
    nextrs_server::app(&info, &options).await.unwrap()
}

/// Prerendered HTML, its RSC payload, the 404 page and a static chunk are
/// each compressed once; later gzip requests get the same bytes from the
/// cache. Identity and HEAD requests never touch it.
#[tokio::test]
async fn immutable_responses_are_compressed_once() {
    let cache = Arc::new(CompressionCache::new(1 << 20, 1 << 20));
    let app = app_with_cache(&snapshot(DPL), &cache).await;
    let chunk = "/_next/static/chunks/310vm2bl3xxpt.js";
    type Case<'a> = (&'a str, &'a [(&'a str, &'a str)], Vec<u8>);
    let cases: [Case; 4] = [
        ("/", &[], read(DPL, "server/app/index.html")),
        ("/?_rsc", &[("rsc", "1")], read(DPL, "server/app/index.rsc")),
        ("/missing", &[], read(DPL, "server/app/_not-found.html")),
        (chunk, &[], read(DPL, "static/chunks/310vm2bl3xxpt.js")),
    ];
    for (path, extra, expected) in &cases {
        let mut first = None;
        for _ in 0..3 {
            let mut headers = extra.to_vec();
            headers.push(("accept-encoding", "gzip"));
            let (response, body) = request(&app, Method::GET, path, &headers).await;
            assert_eq!(
                header(&response, "content-encoding"),
                Some("gzip"),
                "{path}"
            );
            assert_eq!(
                header(&response, "content-length"),
                Some(body.len().to_string().as_str()),
                "{path}"
            );
            assert_eq!(&gunzip(&body), expected, "{path}");
            match &first {
                None => first = Some(body),
                Some(first) => assert_eq!(first, &body, "{path}: the cached copy"),
            }
            // Identity in between: the file's own bytes, not the cache's.
            let (response, body) = request(&app, Method::GET, path, extra).await;
            assert_eq!(header(&response, "content-encoding"), None, "{path}");
            assert_eq!(&body, expected, "{path}");
            let (response, body) = request(
                &app,
                Method::HEAD,
                path,
                &[extra, &[("accept-encoding", "gzip")][..]].concat(),
            )
            .await;
            assert_eq!(header(&response, "content-encoding"), None, "{path}");
            assert!(body.is_empty());
        }
    }
    assert_eq!(cache_stat(&cache, "misses"), 4);
    assert_eq!(cache_stat(&cache, "hits"), 8);
    assert_eq!(cache_stat(&cache, "entries"), 4);
    assert!(cache_stat(&cache, "held_bytes") > 0);
}

/// A static file rewritten in place, even with the same size and its old
/// modification time put back, is compressed again: never a stale copy.
#[tokio::test]
async fn a_rewritten_static_file_is_compressed_again() {
    let dir = std::env::temp_dir().join(format!("nextrs-server-ccfile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot(DPL), &dir);
    let cache = Arc::new(CompressionCache::new(1 << 20, 1 << 20));
    let app = app_with_cache(&dir, &cache).await;
    let path = "/_next/static/chunks/310vm2bl3xxpt.js";
    let file = dir.join(".next/static/chunks/310vm2bl3xxpt.js");
    let original = std::fs::read(&file).unwrap();
    let get = || async {
        let (response, body) =
            request(&app, Method::GET, path, &[("accept-encoding", "gzip")]).await;
        assert_eq!(header(&response, "content-encoding"), Some("gzip"));
        gunzip(&body)
    };
    assert_eq!(get().await, original);
    assert_eq!(get().await, original);
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    let mut changed = original.clone();
    changed[..12].copy_from_slice(b"/*changed!*/");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file, &changed).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(
        std::fs::metadata(&file).unwrap().len(),
        original.len() as u64
    );
    assert_eq!(get().await, changed);
    assert_eq!(cache_stat(&cache, "misses"), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A cache smaller than the build's output stays within its capacity,
/// evicting, and every response is still correct.
#[tokio::test]
async fn the_capacity_bounds_real_output() {
    // Holds a few of the eight chunks; the 229 KB one compresses to more
    // than the whole capacity and is served without being held.
    let capacity = 64 * 1024;
    let cache = Arc::new(CompressionCache::new(capacity, 1 << 20));
    let app = app_with_cache(&snapshot(DEFAULT), &cache).await;
    let dir = snapshot(DEFAULT).join(".next/static/chunks");
    let mut chunks: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|f| f.ends_with(".js"))
        .collect();
    chunks.sort();
    let mut compressed = 0;
    for round in 0..3 {
        for chunk in &chunks {
            let file = read(DEFAULT, &format!("static/chunks/{chunk}"));
            let (response, body) = request(
                &app,
                Method::GET,
                &format!("/_next/static/chunks/{chunk}"),
                &[("accept-encoding", "gzip")],
            )
            .await;
            if header(&response, "content-encoding") == Some("gzip") {
                assert_eq!(gunzip(&body), file, "{chunk}");
                compressed += usize::from(round == 0);
            } else {
                assert_eq!(body, file, "{chunk}");
            }
            assert!(cache_stat(&cache, "held_bytes") <= capacity as u64);
        }
    }
    assert!(compressed >= 4, "{compressed} chunks compressed");
    assert!(
        cache_stat(&cache, "evictions") > 0,
        "{:?}",
        cache.snapshot()
    );
    assert!(cache_stat(&cache, "bypassed") > 0, "{:?}", cache.snapshot());
}

/// `--compression-cache 0`: compressed per request (chunked), as rc.2's
/// first candidate did.
#[tokio::test]
async fn a_disabled_cache_compresses_per_request() {
    let cache = Arc::new(CompressionCache::disabled());
    let app = app_with_cache(&snapshot(DEFAULT), &cache).await;
    let (response, body) = request(&app, Method::GET, "/", &[("accept-encoding", "gzip")]).await;
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
    assert_eq!(header(&response, "content-length"), None);
    assert_eq!(gunzip(&body), read(DEFAULT, "server/app/index.html"));
    assert_eq!(cache_stat(&cache, "entries"), 0);
}
