//! Dynamic routes rendered by the embedded V8 executor through the HTTP
//! layer. Needs the real fixture build (`.next` and `node_modules` are not
//! committed), so the tests are ignored by default:
//!
//!   (cd fixtures/next-app && npm ci && npx next build)
//!   cargo test -p nextrs-server --test dynamic -- --ignored

use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use nextrs_runtime::{Executor, ExecutorConfig};
use nextrs_server::ServerOptions;
use tower::ServiceExt;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/next-app")
}

async fn app(with_executor: bool) -> Router {
    let info = nextrs_adapter::inspect(&fixture()).expect("fixture is built");
    let executor = with_executor.then(|| {
        Executor::start(ExecutorConfig::new(
            info.project_dir.clone(),
            info.dist_dir.clone(),
        ))
        .expect("executor starts")
    });
    nextrs_server::app(
        &info,
        &ServerOptions {
            executor,
            ..ServerOptions::default()
        },
    )
    .await
    .unwrap()
}

async fn get(app: &Router, request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap();
    (
        parts.status,
        parts.headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

const VARY: &str =
    "rsc, next-router-state-tree, next-router-prefetch, next-router-segment-prefetch";

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn renders_dynamic_routes_with_next_semantics() {
    let app = app(true).await;

    let (status, headers, body) =
        get(&app, Request::get("/dynamic").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "text/html; charset=utf-8");
    assert_eq!(headers["vary"], VARY);
    assert!(body.contains("Noli Me Tangere"));
    assert!(body.contains("Streamed after Suspense boundary"));

    let (status, _, body) = get(&app, Request::get("/books/42").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("id=<!-- -->42"), "{body}");

    let request = Request::get("/request-info")
        .header("x-nextrs-probe", "from-test")
        .header("cookie", "theme=solarized")
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = get(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("x-nextrs-probe=<!-- -->from-test"), "{body}");
    assert!(body.contains("theme=<!-- -->solarized"), "{body}");

    let (status, headers, body) = get(
        &app,
        Request::get("/api/health").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/json");
    assert_eq!(headers["vary"], VARY);
    assert_eq!(body, r#"{"status":"ok","service":"nextrs-fixture"}"#);

    // Static serving is unchanged next to the executor.
    let (status, headers, _) = get(&app, Request::get("/").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["x-nextjs-cache"], "HIT");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn dynamic_rsc_requests_keep_the_cache_busting_protocol() {
    let app = app(true).await;
    let request = Request::get("/books/42")
        .header("rsc", "1")
        .body(Body::empty())
        .unwrap();
    let (status, headers, _) = get(&app, request).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(headers["location"], "/books/42?_rsc");

    let request = Request::get("/books/42?_rsc")
        .header("rsc", "1")
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = get(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "text/x-component");
    assert!(body.contains(r#"["Book ","42"]"#), "{body}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn without_an_executor_dynamic_routes_are_refused() {
    let app = app(false).await;
    let (status, headers, _) =
        get(&app, Request::get("/dynamic").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(headers["x-nextrs-unsupported"], "javascript-required");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn server_actions_are_refused_explicitly() {
    let app = app(true).await;
    let request = Request::post("/dynamic")
        .header("next-action", "abc")
        .body(Body::empty())
        .unwrap();
    let (status, headers, _) = get(&app, request).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(headers["x-nextrs-unsupported"], "server-actions");
}

/// HTTP/1.0 cannot carry a chunked body: like Node, close the connection
/// to delimit a streamed render instead of advertising keep-alive.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn http_1_0_streamed_responses_close_the_connection() {
    let app = app(true).await;
    let request = Request::get("/books/42")
        .version(axum::http::Version::HTTP_10)
        .header("connection", "keep-alive")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.version(), axum::http::Version::HTTP_10);
    assert_eq!(response.headers()["connection"], "close");
}
