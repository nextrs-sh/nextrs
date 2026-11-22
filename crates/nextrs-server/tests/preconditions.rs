//! `If-Match` and `If-Unmodified-Since` on static and public files (RFC
//! 9110 sections 13.1.1, 13.1.4 and 13.2.2), through the application router
//! on real Next.js build output, with the existing `If-None-Match`,
//! `If-Modified-Since` and Range behaviour around them.
//!
//! next start 16.4.6 (`send`) evaluates both headers too, but compares
//! `If-Match` weakly and answers every failed precondition with a 500 error
//! page; NextRS answers 412 and uses the strong comparison. Static files
//! carry a weak ETag, so a listed entity tag never satisfies `If-Match` and
//! only `*` does. Pages are answered as next start answers them: their
//! freshness check (`fresh`) ignores both headers.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, Response, StatusCode};
use nextrs_server::{CompressionCache, ServerOptions};
use tower::ServiceExt;

const CHUNK: &str = "3wbm-994r45kr.js";
const PAST: &str = "Sat, 01 Jan 2000 00:00:00 GMT";
const FUTURE: &str = "Fri, 01 Jan 2100 00:00:00 GMT";

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

async fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("nextrs-precond-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot(), &dir);
    std::fs::create_dir_all(dir.join("public/docs")).unwrap();
    std::fs::write(
        dir.join("public/docs/notes.txt"),
        "public file contents\n".repeat(200),
    )
    .unwrap();
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

/// The file's validators, from a plain GET.
async fn validators(app: &Router, uri: &str) -> (String, String) {
    let (response, _) = request(app, Method::GET, uri, &[]).await;
    assert_eq!(response.status(), StatusCode::OK, "{uri}");
    let etag = header(&response, "etag").unwrap().to_owned();
    assert!(
        etag.starts_with("W/\""),
        "static files have weak ETags: {etag}"
    );
    (etag, header(&response, "last-modified").unwrap().to_owned())
}

fn assert_412(response: &Response<Body>, body: &[u8], method: &Method, what: &str) {
    assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED, "{what}");
    // The error is not the file: no immutable caching, no range, no body of
    // the file.
    assert_eq!(header(response, "cache-control"), None, "{what}");
    assert_eq!(header(response, "content-range"), None, "{what}");
    assert_eq!(header(response, "content-encoding"), None, "{what}");
    assert_eq!(
        header(response, "content-type"),
        Some("text/plain; charset=utf-8"),
        "{what}"
    );
    if *method == Method::GET {
        assert_eq!(body, b"Precondition Failed", "{what}");
    } else {
        assert!(body.is_empty(), "{what}");
    }
}

/// Every case, for one file, with GET and HEAD.
async fn check_file(f: &Fixture, uri: &str, cache_control: &str) {
    let (etag, last_modified) = validators(&f.app, uri).await;
    let strong_spelling = etag.trim_start_matches("W/").to_owned();
    let list = format!("\"nope\", {etag}");
    for method in [Method::GET, Method::HEAD] {
        let get = |headers: Vec<(&'static str, String)>| {
            let method = method.clone();
            let app = f.app.clone();
            async move {
                let pairs: Vec<(&str, &str)> =
                    headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
                request(&app, method, uri, &pairs).await
            }
        };
        let label = |case: &str| format!("{method} {uri} {case}");

        // 412: If-Match with any entity tag (static ETags are weak, and
        // If-Match compares strongly), weak or strong spelling, or a list.
        for (case, value) in [
            ("If-Match: the weak ETag itself", etag.clone()),
            ("If-Match: its strong spelling", strong_spelling.clone()),
            ("If-Match: another tag", "\"nope\"".to_owned()),
            ("If-Match: another weak tag", "W/\"nope\"".to_owned()),
            ("If-Match: a list", list.clone()),
        ] {
            let (response, body) = get(vec![("if-match", value)]).await;
            assert_412(&response, &body, &method, &label(case));
            assert_eq!(header(&response, "etag"), Some(etag.as_str()));
        }
        // 200: `*` holds for an existing file.
        let (response, _) = get(vec![("if-match", "*".into())]).await;
        assert_eq!(response.status(), StatusCode::OK, "{}", label("*"));
        assert_eq!(header(&response, "cache-control"), Some(cache_control));

        // If-Unmodified-Since: the file's own date and later hold, earlier
        // fails; something that is not an HTTP-date is ignored.
        for (date, status) in [
            (last_modified.as_str(), StatusCode::OK),
            (FUTURE, StatusCode::OK),
            ("not a date", StatusCode::OK),
            ("2000-01-01", StatusCode::OK),
        ] {
            let (response, _) = get(vec![("if-unmodified-since", date.into())]).await;
            assert_eq!(response.status(), status, "{}", label(date));
        }
        let (response, body) = get(vec![("if-unmodified-since", PAST.into())]).await;
        assert_412(
            &response,
            &body,
            &method,
            &label("If-Unmodified-Since past"),
        );

        // Precedence: If-Match over If-Unmodified-Since, in both directions.
        let (response, _) = get(vec![
            ("if-match", "*".into()),
            ("if-unmodified-since", PAST.into()),
        ])
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{}", label("*, past"));
        let (response, body) = get(vec![
            ("if-match", "\"nope\"".into()),
            ("if-unmodified-since", FUTURE.into()),
        ])
        .await;
        assert_412(&response, &body, &method, &label("nope, future"));

        // A failed precondition is never a 304, a 206 or a compressed body.
        for (other, value) in [
            ("if-none-match", etag.clone()),
            ("if-modified-since", FUTURE.to_owned()),
            ("range", "bytes=0-9".to_owned()),
            ("accept-encoding", "gzip".to_owned()),
            ("cache-control", "no-cache".to_owned()),
        ] {
            let (response, body) = get(vec![("if-match", "\"nope\"".into()), (other, value)]).await;
            assert_412(&response, &body, &method, &label(other));
            let (response, body) = get(vec![
                ("if-unmodified-since", PAST.into()),
                (other, "x".into()),
            ])
            .await;
            assert_412(&response, &body, &method, &label(other));
        }

        // Passing preconditions leave the existing behaviour: 304 from
        // If-None-Match / If-Modified-Since, 206 from Range (GET only).
        let (response, body) = get(vec![
            ("if-match", "*".into()),
            ("if-none-match", etag.clone()),
        ])
        .await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_MODIFIED,
            "{}",
            label("*, inm")
        );
        assert!(body.is_empty());
        let (response, _) = get(vec![
            ("if-unmodified-since", FUTURE.into()),
            ("if-modified-since", FUTURE.into()),
        ])
        .await;
        assert_eq!(
            response.status(),
            StatusCode::NOT_MODIFIED,
            "{}",
            label("ius, ims")
        );
        let (response, body) = get(vec![
            ("if-unmodified-since", FUTURE.into()),
            ("range", "bytes=0-9".into()),
        ])
        .await;
        if method == Method::GET {
            assert_eq!(
                response.status(),
                StatusCode::PARTIAL_CONTENT,
                "{}",
                label("range")
            );
            assert_eq!(body.len(), 10);
        } else {
            assert_eq!(response.status(), StatusCode::OK, "{}", label("range"));
        }
        // No conditional header at all: as before.
        let (response, _) = get(vec![]).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(header(&response, "etag"), Some(etag.as_str()));
        assert_eq!(
            header(&response, "last-modified"),
            Some(last_modified.as_str())
        );
    }
}

#[tokio::test]
async fn next_static_files() {
    let f = fixture("static").await;
    check_file(
        &f,
        &format!("/_next/static/chunks/{CHUNK}"),
        "public, max-age=31536000, immutable",
    )
    .await;
    // A 412 is never stored in the compression cache.
    let uri = format!("/_next/static/chunks/{CHUNK}");
    let (response, _) = request(
        &f.app,
        Method::GET,
        &uri,
        &[("if-match", "\"nope\""), ("accept-encoding", "gzip")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    let entries = |cache: &CompressionCache| {
        cache
            .snapshot()
            .into_iter()
            .find(|(n, _)| *n == "entries")
            .unwrap()
            .1
    };
    assert_eq!(entries(&f.cache), 0);
    let (response, _) = request(&f.app, Method::GET, &uri, &[("accept-encoding", "gzip")]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-encoding"), Some("gzip"));
}

#[tokio::test]
async fn public_files() {
    let f = fixture("public").await;
    check_file(&f, "/docs/notes.txt", "public, max-age=0").await;
}

#[tokio::test]
async fn missing_files_are_404_whatever_the_preconditions() {
    let f = fixture("missing").await;
    for (name, value) in [("if-match", "*"), ("if-unmodified-since", PAST)] {
        let (response, _) = request(
            &f.app,
            Method::GET,
            "/_next/static/chunks/missing.js",
            &[(name, value)],
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{name}");
    }
}

/// Pages keep next start's behaviour: `fresh` ignores both headers.
#[tokio::test]
async fn pages_ignore_if_match_and_if_unmodified_since() {
    let f = fixture("pages").await;
    for (name, value) in [("if-match", "\"nope\""), ("if-unmodified-since", PAST)] {
        let (response, body) = request(&f.app, Method::GET, "/", &[(name, value)]).await;
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        assert!(!body.is_empty());
    }
}
