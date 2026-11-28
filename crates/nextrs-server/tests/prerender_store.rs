//! RE-5B: prerendered output served from its build files through the
//! bounded prerender cache (`--prerender-cache`) answers exactly as the
//! default server, which reads it all at startup: status, every header
//! and every byte, for each prerendered page of the committed snapshot
//! builds (HTML, RSC payloads, segment prefetches, Pages Router pages,
//! the 404 page), cold and warm, for GET, HEAD, conditional requests, gzip
//! clients and other methods, with no cache, a cache smaller than one
//! page, and a large one. A build file changed after startup is refused,
//! never served.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use nextrs_server::{PrerenderStore, ServerOptions};
use tower::ServiceExt;

fn snapshot(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data")
        .join(name)
}

const SNAPSHOTS: &[&str] = &[
    "next-16.4.3-default",
    "next-16.4.3-adapter",
    "next-16.4.6-deployment-id",
    "next-16.4.6-pages-optimized-dynamic",
    "next-16.4.6-static-params",
];

async fn app(dir: &Path, store: Option<Arc<PrerenderStore>>) -> Router {
    let info = nextrs_adapter::inspect(dir).unwrap();
    nextrs_server::app(
        &info,
        &ServerOptions {
            allow_unsupported: true,
            prerender_store: store,
            ..ServerOptions::default()
        },
    )
    .await
    .unwrap()
}

#[derive(Debug, PartialEq)]
struct Answer {
    status: StatusCode,
    headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
}

async fn ask(app: &Router, method: &Method, uri: &str, headers: &[(String, String)]) -> Answer {
    let mut builder = Request::builder().method(method.clone()).uri(uri);
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let mut headers: Vec<_> = parts
        .headers
        .iter()
        .map(|(n, v)| (n.as_str().to_owned(), v.as_bytes().to_vec()))
        .collect();
    headers.sort();
    Answer {
        status: parts.status,
        headers,
        body: to_bytes(body, usize::MAX).await.unwrap().to_vec(),
    }
}

/// The requests a client sends for a build's prerendered output.
/// Method, URI and headers of one request.
type Ask = (Method, String, Vec<(String, String)>);

fn requests(dir: &Path) -> Vec<Ask> {
    let info = nextrs_adapter::inspect(dir).unwrap();
    let h = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    };
    let mut out = Vec::new();
    for prerender in &info.prerenders {
        let path = prerender.pathname.clone();
        for method in [Method::GET, Method::HEAD, Method::POST] {
            out.push((method, path.clone(), h(&[])));
        }
        out.push((Method::GET, path.clone(), h(&[("accept-encoding", "gzip")])));
        out.push((Method::GET, path.clone(), h(&[("rsc", "1")])));
        out.push((
            Method::GET,
            path.clone(),
            h(&[("rsc", "1"), ("accept-encoding", "gzip")]),
        ));
        for (segment, _) in &prerender.segments {
            out.push((
                Method::GET,
                path.clone(),
                h(&[
                    ("rsc", "1"),
                    ("next-router-prefetch", "1"),
                    ("next-router-segment-prefetch", segment),
                ]),
            ));
        }
    }
    out.push((Method::GET, "/no-such-page".into(), h(&[])));
    out.push((Method::HEAD, "/no-such-page".into(), h(&[])));
    out
}

/// Asks, following one redirect to the URL Next's router validation
/// sends RSC requests to (`_rsc`), so the payload itself is compared.
async fn exchange(
    app: &Router,
    method: &Method,
    uri: &str,
    headers: &[(String, String)],
) -> Vec<Answer> {
    let first = ask(app, method, uri, headers).await;
    let location = first
        .headers
        .iter()
        .find(|(n, _)| n == "location")
        .map(|(_, v)| String::from_utf8_lossy(v).into_owned())
        .filter(|l| l.starts_with('/') && l.contains("_rsc="));
    let mut answers = vec![first];
    if let Some(location) = location {
        answers.push(ask(app, method, &location, headers).await);
    }
    answers
}

#[tokio::test]
async fn file_backed_prerenders_answer_exactly_as_resident_ones() {
    let mut compared = 0;
    let mut bodies = 0;
    for name in SNAPSHOTS {
        let dir = snapshot(name);
        let eager = app(&dir, None).await;
        let stores = [0u64, 4096, 64 << 20].map(PrerenderStore::new);
        let mut lazy = Vec::new();
        for store in &stores {
            lazy.push(app(&dir, Some(store.clone())).await);
        }
        for (method, uri, headers) in requests(&dir) {
            let expected = exchange(&eager, &method, &uri, &headers).await;
            // A conditional request with the ETag the page was served with.
            let etag = expected[expected.len() - 1]
                .headers
                .iter()
                .find(|(n, _)| n == "etag")
                .map(|(_, v)| String::from_utf8_lossy(v).into_owned());
            for (i, app) in lazy.iter().enumerate() {
                // Cold, then warm (from the cache where it keeps it).
                for round in ["cold", "warm"] {
                    let actual = exchange(app, &method, &uri, &headers).await;
                    assert_eq!(
                        actual, expected,
                        "{name} {method} {uri} {headers:?}: store {i} ({round})"
                    );
                    compared += 1;
                }
                if let Some(etag) = &etag {
                    let mut conditional = headers.clone();
                    conditional.push(("if-none-match".into(), etag.clone()));
                    let actual = exchange(app, &method, &uri, &conditional).await;
                    let expected = exchange(&eager, &method, &uri, &conditional).await;
                    assert_eq!(actual, expected, "{name} {method} {uri}: If-None-Match");
                    compared += 1;
                }
            }
            bodies += expected.iter().filter(|a| !a.body.is_empty()).count();
        }
        for store in &stores {
            assert!(store.cached_bytes() <= store.capacity());
        }
        assert!(
            stores[2].cached_entries() > 0,
            "{name}: the large cache keeps pages"
        );
        assert_eq!(stores[0].cached_entries(), 0);
    }
    println!("{compared} comparisons, {bodies} non-empty reference bodies");
    assert!(bodies > 50, "the snapshots exercise prerendered bodies");
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[tokio::test]
async fn a_prerender_changed_after_startup_is_refused_not_served() {
    let dir = std::env::temp_dir().join(format!("nextrs-re5b-changed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot("next-16.4.3-default"), &dir);
    let store = PrerenderStore::new(64 << 20);
    let lazy = app(&dir, Some(store.clone())).await;
    let before = ask(&lazy, &Method::GET, "/", &[]).await;
    assert_eq!(before.status, StatusCode::OK);
    // Served from the cache from now on: a cached body is the startup one.
    let index = dir.join(".next/server/app/index.html");
    std::fs::write(&index, b"<html>tampered</html>").unwrap();
    let cached = ask(&lazy, &Method::GET, "/", &[]).await;
    assert_eq!(cached, before);
    // A fresh server reads it lazily: changed after its startup → 500.
    let store = PrerenderStore::new(0);
    let fresh = app(&dir, Some(store)).await;
    let original_len = std::fs::metadata(&index).unwrap().len();
    std::fs::write(&index, vec![b'x'; original_len as usize + 1]).unwrap();
    let refused = ask(&fresh, &Method::GET, "/", &[]).await;
    assert_eq!(refused.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!refused.body.windows(3).any(|w| w == b"xxx"));
    let _ = std::fs::remove_dir_all(&dir);
}
