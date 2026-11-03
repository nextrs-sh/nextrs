//! Streaming request bodies: JavaScript pulls chunks on demand, so an
//! unread body is never buffered, a slow upload is consumed as it arrives,
//! and errors (aborted upload, size limit) reach the handler as Node-style
//! stream errors.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use common::*;
use futures_util::StreamExt;
use nextrs_runtime::{BodyError, RenderRequest, RequestBody};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn post(body: RequestBody, headers: &[(&str, &str)]) -> RenderRequest {
    let mut request = request("echo.js", headers);
    request.method = "POST".into();
    request.body = body;
    request
}

fn payload(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn chunked(data: Vec<u8>, size: usize, delay: Option<Duration>) -> RequestBody {
    let chunks: Vec<Bytes> = data.chunks(size).map(Bytes::copy_from_slice).collect();
    RequestBody::Stream(Box::pin(futures_util::stream::iter(chunks).then(
        move |chunk| async move {
            if let Some(delay) = delay {
                tokio::time::sleep(delay).await;
            }
            Ok(chunk)
        },
    )))
}

fn sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn echo(executor: &nextrs_runtime::Executor, request: RenderRequest) -> (u16, Value) {
    let response = collect(executor, request).await;
    (
        response.status,
        serde_json::from_slice(&response.body).unwrap_or(Value::Null),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn small_buffered_body() {
    let executor = executor(small_limits());
    let (status, result) = echo(
        &executor,
        post(RequestBody::Full(Bytes::from_static(b"hello")), &[]),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(result["bytes"], 5);
    assert_eq!(result["sha256"], sha256(b"hello"));
    assert_eq!(result["complete"], true);
}

#[tokio::test(flavor = "multi_thread")]
async fn large_streamed_body_arrives_intact_in_many_reads() {
    let executor = executor(small_limits());
    let data = payload(6 * 1024 * 1024, 7);
    let digest = sha256(&data);
    let (status, result) = echo(&executor, post(chunked(data, 64 * 1024, None), &[])).await;
    assert_eq!(status, 200);
    assert_eq!(result["bytes"], 6 * 1024 * 1024);
    assert_eq!(result["sha256"], digest);
    assert!(result["reads"].as_u64().unwrap() >= 96, "{result}");
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_uploads_are_consumed_as_they_arrive() {
    let executor = executor(small_limits());
    let data = payload(64 * 1024, 3);
    let digest = sha256(&data);
    let (status, result) = echo(
        &executor,
        post(chunked(data, 4096, Some(Duration::from_millis(5))), &[]),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(result["sha256"], digest);
    assert_eq!(result["reads"], 16);
}

/// A handler that never reads its body pulls nothing from the producer.
#[tokio::test(flavor = "multi_thread")]
async fn an_unread_body_is_not_pulled() {
    let executor = executor(small_limits());
    let pulled = Arc::new(AtomicUsize::new(0));
    let counter = pulled.clone();
    let body = futures_util::stream::iter(0..10_000).map(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(Bytes::from(vec![0u8; 64 * 1024]))
    });
    let response = collect(
        &executor,
        post(RequestBody::Stream(Box::pin(body)), &[("x-skip-body", "1")]),
    )
    .await;
    assert_eq!(response.text(), "ignored body");
    assert_eq!(pulled.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_aborted_upload_reaches_the_handler_as_econnreset() {
    let executor = executor(small_limits());
    let body = futures_util::stream::iter(vec![
        Ok(Bytes::from_static(b"partial")),
        Err(BodyError::Aborted("connection reset".into())),
    ]);
    let (status, result) = echo(&executor, post(RequestBody::Stream(Box::pin(body)), &[])).await;
    assert_eq!(status, 400);
    assert_eq!(result["error"], "ECONNRESET");
    assert_eq!(result["message"], "aborted");
    assert_eq!(result["aborted"], true);
    // As in Node, destroy() discards data not yet consumed, so whether the
    // handler saw the 7 bytes before the error depends on timing.
    assert!(result["bytes"].as_u64().unwrap() <= 7);
    assert_eq!(executor.metrics().request_body_errors, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_body_over_the_limit_fails_with_413_semantics() {
    let executor = executor(small_limits());
    let body = futures_util::stream::iter(vec![
        Ok(Bytes::from(vec![1u8; 1000])),
        Err(BodyError::TooLarge { limit: 1000 }),
    ]);
    let (status, result) = echo(&executor, post(RequestBody::Stream(Box::pin(body)), &[])).await;
    assert_eq!(status, 413);
    assert_eq!(result["error"], "ERR_NEXTRS_BODY_TOO_LARGE");
}

#[tokio::test(flavor = "multi_thread")]
async fn simultaneous_uploads_stay_separate() {
    let executor = executor(nextrs_runtime::Limits {
        isolates: 2,
        ..small_limits()
    });
    let tasks: Vec<_> = (0..32u8)
        .map(|i| {
            let executor = executor.clone();
            tokio::spawn(async move {
                let data = payload(200_000 + i as usize * 1000, i);
                let digest = sha256(&data);
                let delay = (i % 3 == 0).then(|| Duration::from_millis(1));
                let (status, result) = echo(&executor, post(chunked(data, 8192, delay), &[])).await;
                (status, result["sha256"].as_str().map(str::to_owned), digest)
            })
        })
        .collect();
    for task in tasks {
        let (status, got, expected) = task.await.unwrap();
        assert_eq!(status, 200);
        assert_eq!(got.as_deref(), Some(expected.as_str()));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_streamed_body_can_be_streamed_back() {
    let executor = executor(small_limits());
    let data = payload(3 * 1024 * 1024, 9);
    let response = collect(
        &executor,
        post(
            chunked(data.clone(), 32 * 1024, None),
            &[("x-stream-back", "1")],
        ),
    )
    .await;
    assert!(response.ended);
    assert_eq!(response.body.len(), data.len());
    assert!(response.body == data, "echoed body differs");
    assert!(response.chunks >= 90);
}
