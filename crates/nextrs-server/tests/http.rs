//! HTTP behaviour against snapshots of real Next.js 16.4.3 builds. Expected
//! values marked "next start" were recorded from `next start` serving the
//! same build (tests/compat/compare.mjs records the comparison).

use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, Response, StatusCode};
use nextrs_server::{ServerOptions, StartError};
use tower::ServiceExt;

fn snapshot(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data")
        .join(name)
}

async fn app_for(name: &str) -> Router {
    let info = nextrs_adapter::inspect(&snapshot(name)).unwrap();
    nextrs_server::app(&info, &ServerOptions::default())
        .await
        .unwrap()
}

async fn send(app: &Router, request: Request<Body>) -> (Response<Body>, Vec<u8>) {
    let response = app.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap().to_vec();
    (Response::from_parts(parts, Body::empty()), bytes)
}

async fn get(app: &Router, uri: &str) -> (Response<Body>, Vec<u8>) {
    send(app, Request::get(uri).body(Body::empty()).unwrap()).await
}

fn header<'a>(response: &'a Response<Body>, name: &str) -> Option<&'a str> {
    response.headers().get(name).and_then(|v| v.to_str().ok())
}

fn read(name: &str, rel: &str) -> Vec<u8> {
    std::fs::read(snapshot(name).join(".next").join(rel)).unwrap()
}

#[tokio::test]
async fn serves_prerendered_html_like_next_start() {
    let app = app_for("next-16.4.3-default").await;
    let (response, body) = get(&app, "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body, read("next-16.4.3-default", "server/app/index.html"));
    // next start, default build: ETag "i77w8p2mwj4lr", Content-Length 5967.
    assert_eq!(header(&response, "etag"), Some("\"i77w8p2mwj4lr\""));
    assert_eq!(header(&response, "content-length"), Some("5967"));
    assert_eq!(
        header(&response, "content-type"),
        Some("text/html; charset=utf-8")
    );
    assert_eq!(
        header(&response, "cache-control"),
        Some("s-maxage=31536000")
    );
    assert_eq!(header(&response, "x-nextjs-cache"), Some("HIT"));
    assert_eq!(header(&response, "x-nextjs-stale-time"), Some("300"));
    assert_eq!(header(&response, "x-powered-by"), Some("Next.js"));
    assert_eq!(
        header(&response, "vary"),
        Some("rsc, next-router-state-tree, next-router-prefetch, next-router-segment-prefetch")
    );
    assert_eq!(
        header(&response, "x-next-cache-tags"),
        None,
        "internal header leaked"
    );
}

#[tokio::test]
async fn adapter_layout_serves_the_same_way() {
    let app = app_for("next-16.4.3-adapter").await;
    let (response, body) = get(&app, "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body.len(), 5967);
    // next start, adapter build: ETag "y33y3w3rco4lr".
    assert_eq!(header(&response, "etag"), Some("\"y33y3w3rco4lr\""));
}

#[tokio::test]
async fn head_and_conditional_requests() {
    let app = app_for("next-16.4.3-default").await;
    let (response, body) = send(&app, Request::head("/").body(Body::empty()).unwrap()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(body.is_empty());
    assert_eq!(header(&response, "content-length"), Some("5967"));

    let request = Request::get("/")
        .header("if-none-match", "\"i77w8p2mwj4lr\"")
        .body(Body::empty())
        .unwrap();
    let (response, body) = send(&app, request).await;
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert!(body.is_empty());
    assert_eq!(header(&response, "content-type"), None);
}

#[tokio::test]
async fn serves_static_chunks_with_immutable_caching() {
    let app = app_for("next-16.4.3-default").await;
    let (response, body) = get(&app, "/_next/static/chunks/turbopack-3-ry682tkyiqk.js").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body,
        read(
            "next-16.4.3-default",
            "static/chunks/turbopack-3-ry682tkyiqk.js"
        )
    );
    assert_eq!(
        header(&response, "content-type"),
        Some("application/javascript; charset=UTF-8")
    );
    assert_eq!(
        header(&response, "cache-control"),
        Some("public, max-age=31536000, immutable")
    );
    let etag = header(&response, "etag").unwrap().to_owned();
    assert!(etag.starts_with("W/\"272e-"), "{etag}");

    let request = Request::get("/_next/static/chunks/turbopack-3-ry682tkyiqk.js")
        .header("if-none-match", etag)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&app, request).await.0.status(),
        StatusCode::NOT_MODIFIED
    );

    let (response, _) = get(&app, "/_next/static/chunks/missing.js").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn never_serves_files_outside_static() {
    let app = app_for("next-16.4.3-default").await;
    let secrets = [
        read("next-16.4.3-default", "prerender-manifest.json"),
        read("next-16.4.3-default", "BUILD_ID"),
        read("next-16.4.3-default", "required-server-files.json"),
    ];
    for uri in [
        "/_next/static/../../BUILD_ID",
        "/_next/static/%2e%2e/%2e%2e/prerender-manifest.json",
        "/_next/static/..%2f..%2fprerender-manifest.json",
        "/_next/static/chunks/..%5c..%5c..%5cBUILD_ID",
        "/_next/static/%2E%2E/required-server-files.json",
        "/_next/prerender-manifest.json",
        "/prerender-manifest.json",
        "/.next/BUILD_ID",
        "/_next/static/chunks/%00",
    ] {
        let (response, body) = get(&app, uri).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        assert!(!secrets.contains(&body), "{uri} leaked a build file");
    }
}

#[tokio::test]
async fn trailing_slash_redirect_matches_next_start() {
    let app = app_for("next-16.4.3-default").await;
    let (response, body) = get(&app, "/books/42/?x=1").await;
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert_eq!(header(&response, "location"), Some("/books/42?x=1"));
    assert_eq!(header(&response, "refresh"), Some("0;url=/books/42?x=1"));
    assert_eq!(body, b"/books/42?x=1");
}

#[tokio::test]
async fn rsc_requests_use_cache_busting_redirects() {
    let app = app_for("next-16.4.3-default").await;
    let rsc = |uri: &str| {
        Request::get(uri)
            .header("rsc", "1")
            .body(Body::empty())
            .unwrap()
    };

    let (response, body) = send(&app, rsc("/")).await;
    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(header(&response, "location"), Some("/?_rsc"));
    assert!(body.is_empty());

    let (response, body) = send(&app, rsc("/?_rsc")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, "content-type"), Some("text/x-component"));
    assert_eq!(body, read("next-16.4.3-default", "server/app/index.rsc"));
    assert_eq!(header(&response, "etag"), None);

    let segment = |uri: &str| {
        Request::get(uri)
            .header("rsc", "1")
            .header("next-router-prefetch", "1")
            .header("next-router-segment-prefetch", "/_tree")
            .body(Body::empty())
            .unwrap()
    };
    // next start redirected `/?a=1` to `/?a=1&_rsc=_i_aeImnuN6u1u1r`.
    let (response, _) = send(&app, segment("/?a=1")).await;
    assert_eq!(
        header(&response, "location"),
        Some("/?a=1&_rsc=_i_aeImnuN6u1u1r")
    );
    let (response, body) = send(&app, segment("/?a=1&_rsc=_i_aeImnuN6u1u1r")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body,
        read(
            "next-16.4.3-default",
            "server/app/index.segments/_tree.segment.rsc"
        )
    );
    assert_eq!(header(&response, "x-nextjs-postponed"), Some("2"));
    assert_eq!(header(&response, "x-nextjs-stale-time"), None);
}

#[tokio::test]
async fn dynamic_routes_are_refused_explicitly() {
    let app = app_for("next-16.4.3-default").await;
    for uri in [
        "/dynamic",
        "/books/42",
        "/api/health",
        "/request-info",
        "/dynamic/",
    ] {
        let (response, body) = get(&app, uri).await;
        if uri.ends_with('/') {
            assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
            continue;
        }
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{uri}");
        assert_eq!(
            header(&response, "x-nextrs-unsupported"),
            Some("javascript-required")
        );
        assert_eq!(header(&response, "cache-control"), Some("no-store"));
        assert!(
            String::from_utf8(body)
                .unwrap()
                .contains("requires JavaScript")
        );
    }
    // A dynamic RSC request must not fall back to anything static either.
    let request = Request::get("/dynamic?_rsc")
        .header("rsc", "1")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        send(&app, request).await.0.status(),
        StatusCode::NOT_IMPLEMENTED
    );
}

#[tokio::test]
async fn unknown_paths_get_the_prerendered_not_found_page() {
    let app = app_for("next-16.4.3-default").await;
    let (response, body) = get(&app, "/does-not-exist").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        read("next-16.4.3-default", "server/app/_not-found.html")
    );
    assert_eq!(
        header(&response, "cache-control"),
        Some("private, no-cache, no-store, max-age=0, must-revalidate")
    );
    // next start: ETag "ehms0u4scp5me" for this build's not-found page.
    assert_eq!(header(&response, "etag"), Some("\"ehms0u4scp5me\""));
    // The framework route itself is not directly routable as a page.
    let (response, _) = get(&app, "/_not-found").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn non_get_methods() {
    let app = app_for("next-16.4.3-default").await;
    let (response, _) = send(&app, Request::post("/").body(Body::empty()).unwrap()).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(header(&response, "allow"), Some("GET, HEAD"));

    let action = Request::post("/")
        .header("next-action", "abc")
        .body(Body::empty())
        .unwrap();
    let (response, _) = send(&app, action).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    assert_eq!(
        header(&response, "x-nextrs-unsupported"),
        Some("server-actions")
    );

    let (response, _) = send(
        &app,
        Request::post("/api/health").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}

#[tokio::test]
async fn image_optimizer_is_refused() {
    let app = app_for("next-16.4.3-default").await;
    let (response, _) = get(&app, "/_next/image?url=%2Fa.png&w=64&q=75").await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    assert_eq!(
        header(&response, "x-nextrs-unsupported"),
        Some("image-optimizer")
    );
}

#[tokio::test]
async fn refuses_to_start_on_unsupported_features() {
    let dir = std::env::temp_dir().join(format!("nextrs-server-mw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot("next-16.4.3-default"), &dir);
    let manifest = dir.join(".next/server/middleware-manifest.json");
    std::fs::write(
        &manifest,
        r#"{"version":3,"middleware":{"/":{"files":[],"name":"middleware","page":"/"}},"sortedMiddleware":["/"],"functions":{}}"#,
    )
    .unwrap();
    let info = nextrs_adapter::inspect(&dir).unwrap();
    match nextrs_server::app(&info, &ServerOptions::default()).await {
        Err(StartError::Unsupported(diagnostics)) => {
            assert!(diagnostics.iter().any(|d| d.code == "middleware"));
        }
        Err(other) => panic!("unexpected error {other}"),
        Ok(_) => panic!("server started although middleware cannot run"),
    }
    let allowed = ServerOptions {
        allow_unsupported: true,
        ..ServerOptions::default()
    };
    assert!(nextrs_server::app(&info, &allowed).await.is_ok());
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
