//! RE-23B scratch: stall margin of the large streamed body test.
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
async fn re23b_large_streamed_body_stall_margin() {
    let ms: u64 = std::env::var("RE23B_STALL_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(500);
    let limits = nextrs_runtime::Limits { stall_timeout: Some(Duration::from_millis(ms)), ..small_limits() };
    let executor = executor(limits);
    let bytes: usize = std::env::var("RE23B_BYTES").ok().and_then(|v| v.parse().ok()).unwrap_or(6 * 1024 * 1024);
    let data = payload(bytes, 7);
    let digest = sha256(&data);
    let (status, result) = echo(&executor, post(chunked(data, 64 * 1024, None), &[])).await;
    eventually(Duration::from_secs(5), async || executor.metrics().executors[0].restarts > 0 || status == 200).await;
    let m = executor.metrics();
    let fails: Vec<_> = m.executors[0].failures.iter().filter(|(_, n)| *n > 0).cloned().collect();
    println!("RE23B failures={fails:?}");
    println!("RE23B bytes={bytes} stall_ms={ms} status={status} reads={} sha_ok={} restarts={}", result["reads"], result["sha256"] == digest, m.executors.iter().map(|e| e.restarts).sum::<u64>());
    assert_eq!(status, 200);
    assert_eq!(result["sha256"], digest);
}
