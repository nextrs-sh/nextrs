//! The native buffer budget (RE-4C) on static files, against a real
//! Next.js build snapshot: a response holds its file's bytes until its body
//! is dropped; a request that finds no room waits, then gets a 503 with
//! Retry-After; HEAD and 304 reserve nothing; a range reserves only itself;
//! a file larger than the whole budget is served alone; nothing leaks.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
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
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.len(), CHUNK_LEN);
    assert_eq!(budget.reserved_bytes(), 0, "released once the body is gone");
}

#[tokio::test]
async fn no_room_waits_then_answers_503_and_room_again_serves() {
    let budget = NativeBufferBudget::new(12 * 1024, Duration::from_millis(50));
    let app = app(&budget).await;
    let held = call(&app, get(CHUNK)).await;
    assert_eq!(held.status(), StatusCode::OK);
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
    drop(held);
    assert_eq!(waiting.await.unwrap(), StatusCode::OK);
    assert_eq!(budget.reserved_bytes(), 0);
    assert!(
        budget
            .to_prometheus()
            .contains("nextrs_native_buffer_refused_total{kind=\"static_file\"} 1")
    );
}

#[tokio::test]
async fn head_and_not_modified_reserve_nothing() {
    let budget = NativeBufferBudget::new(12 * 1024, Duration::from_millis(20));
    let app = app(&budget).await;
    let held = call(&app, get(CHUNK)).await;
    let etag = held.headers()["etag"].clone();
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
    let budget = NativeBufferBudget::new(12 * 1024, Duration::from_millis(20));
    let app = app(&budget).await;
    let held = call(&app, get(CHUNK)).await;
    // 10 KiB held, 2 KiB left: a 1000-byte range fits.
    let partial = call(
        &app,
        Request::get(CHUNK)
            .header("range", "bytes=0-999")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
    let body = to_bytes(partial.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.len(), 1000);
    drop(held);
    assert_eq!(budget.reserved_bytes(), 0);
}

#[tokio::test]
async fn a_file_larger_than_the_budget_is_served_alone() {
    let budget = NativeBufferBudget::new(4 * 1024, Duration::from_millis(20));
    let app = app(&budget).await;
    let response = call(&app, get(CHUNK)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(budget.reserved_bytes(), 4 * 1024, "takes the whole budget");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.len(), CHUNK_LEN);
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
                let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
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
