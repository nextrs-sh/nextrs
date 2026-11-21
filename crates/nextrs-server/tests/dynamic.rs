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
            // The fixture has Server Actions, which only the executor runs.
            allow_unsupported: !with_executor,
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

/// With next start's compression middleware's Accept-Encoding.
const VARY: &str = "rsc, next-router-state-tree, next-router-prefetch, \
                    next-router-segment-prefetch, Accept-Encoding";

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
async fn server_action_requests_reach_next_action_handler() {
    // Without the executor, the build is refused at startup (it has
    // Server Actions); with --allow-unsupported, requests get a 501.
    let info = nextrs_adapter::inspect(&fixture()).expect("fixture is built");
    let refused = nextrs_server::app(&info, &ServerOptions::default()).await;
    assert!(
        matches!(refused, Err(nextrs_server::StartError::Unsupported(ref d)) if d.iter().any(|d| d.code == "server-actions"))
    );
    let static_only = nextrs_server::app(
        &info,
        &ServerOptions {
            allow_unsupported: true,
            ..ServerOptions::default()
        },
    )
    .await
    .unwrap();
    let request = Request::post("/actions/simple")
        .header("next-action", "40ffffffffffffffffffffffffffffffffffffffff")
        .body(Body::empty())
        .unwrap();
    let (status, headers, _) = get(&static_only, request).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(headers["x-nextrs-unsupported"], "server-actions");

    // With it, Next's own action handler answers, on prerendered and
    // dynamic pages alike: an ID it does not know is "unavailable" (409),
    // a malformed one is "invalid" (400), as next start does.
    let app = app(true).await;
    for path in ["/actions/simple", "/dynamic"] {
        for (id, expected) in [
            (
                "40ffffffffffffffffffffffffffffffffffffffff",
                StatusCode::CONFLICT,
            ),
            ("not-an-action", StatusCode::BAD_REQUEST),
        ] {
            let request = Request::post(path)
                .header("next-action", id)
                .header("content-type", "text/plain;charset=UTF-8")
                .body(Body::from("[]"))
                .unwrap();
            let (status, headers, _) = get(&app, request).await;
            assert_eq!(status, expected, "{path} {id}");
            assert_eq!(headers["x-nextjs-action-not-found"], "1");
            assert_eq!(
                headers["cache-control"],
                "no-cache, no-store, max-age=0, must-revalidate"
            );
        }
    }

    // A POST that cannot be an action still gets 405 on a prerendered page.
    let request = Request::post("/actions/simple")
        .header("content-type", "text/plain")
        .body(Body::from("x"))
        .unwrap();
    let (status, _, _) = get(&app, request).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    // GET with a Next-Action header is an ordinary page request.
    let request = Request::get("/actions/simple")
        .header("next-action", "40ffffffffffffffffffffffffffffffffffffffff")
        .body(Body::empty())
        .unwrap();
    let (status, headers, _) = get(&app, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["x-nextjs-cache"], "HIT");
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

/// What next start 16.4.6 answered for each request (fixtures/next-app):
/// an RSC request (`rsc: 1`) to an App Router route handler is validated
/// like one to a page, for every method: without the `_rsc` value its
/// router headers hash to, a 307 to the same URL with it (the other query
/// pairs kept, an empty body); with it, the handler's own response.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn rsc_requests_to_route_handlers_are_validated() {
    let app = app(true).await;
    let send = |method: &str, uri: &str, headers: &[(&str, &str)], body: &'static str| {
        let mut request = Request::builder().method(method).uri(uri);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        get(&app, request.body(Body::from(body)).unwrap())
    };
    let rsc = [("rsc", "1")];
    for (method, uri, location) in [
        ("GET", "/api/health", "/api/health?_rsc"),
        ("GET", "/api/health?x=1&y=2", "/api/health?x=1&y=2&_rsc"),
        ("GET", "/api/health?_rsc=abc", "/api/health?_rsc"),
        ("HEAD", "/api/health", "/api/health?_rsc"),
        ("OPTIONS", "/api/health", "/api/health?_rsc"),
        ("POST", "/api/echo", "/api/echo?_rsc"),
        (
            "POST",
            "/api/echo?mode=stream",
            "/api/echo?mode=stream&_rsc",
        ),
    ] {
        let (status, headers, body) = send(method, uri, &rsc, "hello").await;
        assert_eq!(status, StatusCode::TEMPORARY_REDIRECT, "{method} {uri}");
        assert_eq!(headers["location"], location, "{method} {uri}");
        assert!(body.is_empty(), "{method} {uri}");
        // Following the redirect reaches the handler: no loop.
        let (status, _, _) = send(method, location, &rsc, "hello").await;
        assert!(status.is_success(), "{method} {location}: {status}");
    }
    // The router's headers are part of the hash (next start's value).
    let prefetch = [("rsc", "1"), ("next-router-prefetch", "1")];
    let (status, headers, _) = send("GET", "/api/health", &prefetch, "").await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(headers["location"], "/api/health?_rsc=HEAvJdHAQJrUAe2K");
    let (status, _, body) = send("GET", "/api/health?_rsc=HEAvJdHAQJrUAe2K", &prefetch, "").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"status\":\"ok\""), "{body}");
    // A matching `_rsc`: the handler's response, POST body intact.
    let (status, _, body) = send("POST", "/api/echo?_rsc", &rsc, "hello").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("\"bytes\":5"), "{body}");
    // Not RSC requests (next start: 200): no header, another value, the
    // header repeated (Node joins it to "1, 1").
    for headers in [
        &[][..],
        &[("rsc", "0")][..],
        &[("rsc", "true")][..],
        &[("rsc", "1"), ("rsc", "1")][..],
    ] {
        let (status, _, body) = send("GET", "/api/health", headers, "").await;
        assert_eq!(status, StatusCode::OK, "{headers:?}");
        assert!(body.contains("\"status\":\"ok\""), "{headers:?}");
        let (status, _, body) = send("POST", "/api/echo", headers, "hello").await;
        assert_eq!(status, StatusCode::OK, "{headers:?}");
        assert!(body.contains("\"bytes\":5"), "{headers:?}");
    }
    // Pages keep their validation; a repeated header is no RSC request
    // there either (next start renders the HTML document).
    let (status, headers, _) = send("GET", "/dynamic", &rsc, "").await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(headers["location"], "/dynamic?_rsc");
    let (status, headers, body) = send("GET", "/dynamic", &[("rsc", "1"), ("rsc", "1")], "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "text/html; charset=utf-8");
    assert!(body.contains("Noli Me Tangere"));
}

/// Only immutable output enters the compression cache: SSR, streamed
/// HTML, RSC renders and route handler responses are compressed per
/// request; the prerendered page is compressed once.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn dynamic_output_is_never_cached() {
    let info = nextrs_adapter::inspect(&fixture()).expect("fixture is built");
    let executor = Executor::start(ExecutorConfig::new(
        info.project_dir.clone(),
        info.dist_dir.clone(),
    ))
    .expect("executor starts");
    let cache = std::sync::Arc::new(nextrs_server::CompressionCache::new(8 << 20, 4 << 20));
    let app = nextrs_server::app(
        &info,
        &ServerOptions {
            executor: Some(executor),
            compression_cache: cache.clone(),
            ..ServerOptions::default()
        },
    )
    .await
    .unwrap();
    let entries = || {
        cache
            .snapshot()
            .into_iter()
            .find(|(n, _)| *n == "entries")
            .unwrap()
            .1
    };
    for _ in 0..2 {
        for (uri, rsc) in [
            ("/dynamic", false),
            ("/books/42", false),
            ("/books/42?_rsc", true),
            ("/request-info", false),
            ("/api/health", false),
        ] {
            let mut request = Request::get(uri).header("accept-encoding", "gzip");
            if rsc {
                request = request.header("rsc", "1");
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert!(
                response.status().is_success(),
                "{uri}: {}",
                response.status()
            );
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap_or_else(|e| panic!("{uri}: {e}"));
            assert_eq!(entries(), 0, "{uri} was cached");
        }
    }
    let response = app
        .clone()
        .oneshot(
            Request::get("/")
                .header("accept-encoding", "gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["content-encoding"], "gzip");
    assert_eq!(entries(), 1);
}
