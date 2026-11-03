//! Response backpressure: a consumer that reads slowly or not at all must
//! hold the JavaScript producer back, so memory stays bounded by the
//! configured budget per response instead of by the response size.

mod common;

use std::time::Duration;

use common::*;
use nextrs_runtime::{Limits, ResponseEvent};
use serde_json::Value;

const CHUNK: usize = 16 * 1024;
const BUDGET: usize = 64 * 1024;
/// Writable's default highWaterMark in readable-stream.
const JS_HIGH_WATER_MARK: usize = 16 * 1024;

fn limits() -> Limits {
    Limits {
        response_buffer_bytes: BUDGET,
        ..small_limits()
    }
}

fn flood(id: &str, total: usize, extra: &[(&str, &str)]) -> nextrs_runtime::RenderRequest {
    let total = total.to_string();
    let chunk = CHUNK.to_string();
    let mut headers = vec![("x-id", id), ("x-total", &total), ("x-chunk", &chunk)];
    headers.extend_from_slice(extra);
    request("flood.js", &headers)
}

async fn progress(executor: &nextrs_runtime::Executor, id: &str) -> Value {
    let report = collect(executor, request("flood-progress.js", &[("x-id", id)])).await;
    serde_json::from_slice(&report.body).expect("progress json")
}

/// Every chunk-sized block must carry its index, in order.
fn assert_ordered(body: &[u8], total: usize) {
    assert_eq!(body.len(), total, "body length");
    for (index, block) in body.chunks(CHUNK).enumerate() {
        let seen = u32::from_be_bytes(block[..4].try_into().unwrap()) as usize;
        assert_eq!(seen, index, "block {index} out of order");
        assert!(
            block[4..].iter().all(|b| *b as usize == index % 251),
            "block {index} corrupted"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_that_stops_reading_stops_the_producer() {
    let executor = executor(limits());
    let total = 8 * 1024 * 1024;
    let mut stream = executor.render(flood("paused", total, &[])).await.unwrap();
    assert!(matches!(
        stream.recv().await,
        Some(ResponseEvent::Head { status: 200, .. })
    ));
    // Not reading: chunks pile up in the channel, still charged.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let state = progress(&executor, "paused").await;
    let produced = state["produced"].as_u64().unwrap() as usize;
    let bound = BUDGET + JS_HIGH_WATER_MARK + 2 * CHUNK;
    assert!(
        produced <= bound,
        "produced {produced} bytes while the consumer was paused (bound {bound})"
    );
    assert!(stream.buffered_bytes() <= BUDGET + CHUNK);
    assert!(state["drains"].as_u64().unwrap() >= 1);

    // Resuming delivers everything, in order.
    let mut body = Vec::new();
    while let Some(event) = stream.recv().await {
        match event {
            ResponseEvent::Chunk(chunk) => body.extend_from_slice(&chunk),
            ResponseEvent::End => break,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_ordered(&body, total);
    assert_eq!(stream.buffered_bytes(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_consumer_gets_a_complete_ordered_body() {
    let executor = executor(limits());
    let total = 2 * 1024 * 1024;
    let mut stream = executor.render(flood("slow", total, &[])).await.unwrap();
    let mut body = Vec::new();
    let mut peak = 0;
    while let Some(event) = stream.recv().await {
        peak = peak.max(stream.buffered_bytes());
        match event {
            ResponseEvent::Head { .. } => {}
            ResponseEvent::Chunk(chunk) => {
                body.extend_from_slice(&chunk);
                if body.len() % (256 * 1024) == 0 {
                    tokio::time::sleep(Duration::from_millis(15)).await;
                }
            }
            ResponseEvent::End => break,
            ResponseEvent::Error(error) => panic!("{error}"),
        }
    }
    assert_ordered(&body, total);
    assert!(peak <= BUDGET + CHUNK, "peak buffered {peak}");
}

/// Large single writes are accepted one at a time.
#[tokio::test(flavor = "multi_thread")]
async fn chunks_larger_than_the_budget_still_flow() {
    let executor = executor(Limits {
        response_buffer_bytes: 4 * 1024,
        ..limits()
    });
    let total = 1024 * 1024;
    let response = collect(&executor, flood("big", total, &[])).await;
    assert!(response.ended);
    assert_ordered(&response.body, total);
}

/// A producer that ignores write()'s return value buffers in its own
/// Writable (in the V8 heap, as it would in Node), but Rust still holds no
/// more than the budget plus one chunk.
#[tokio::test(flavor = "multi_thread")]
async fn ignoring_backpressure_cannot_grow_host_buffers() {
    let executor = executor(limits());
    let total = 4 * 1024 * 1024;
    let mut stream = executor
        .render(flood("ignorant", total, &[("x-ignore-backpressure", "1")]))
        .await
        .unwrap();
    assert!(matches!(
        stream.recv().await,
        Some(ResponseEvent::Head { .. })
    ));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(stream.buffered_bytes() <= BUDGET + CHUNK);
    let state = progress(&executor, "ignorant").await;
    assert_eq!(state["produced"].as_u64().unwrap() as usize, total);
    let response = drain(stream).await;
    assert!(response.ended);
    assert_ordered(&response.body, total);
}

#[tokio::test(flavor = "multi_thread")]
async fn disconnecting_while_blocked_on_capacity_releases_the_request() {
    let executor = executor(limits());
    let mut stream = executor
        .render(flood("gone", 64 * 1024 * 1024, &[]))
        .await
        .unwrap();
    assert!(matches!(
        stream.recv().await,
        Some(ResponseEvent::Head { .. })
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(executor.metrics().active(), 1);
    drop(stream);
    assert!(
        eventually(Duration::from_secs(2), async || executor.metrics().active()
            == 0)
        .await,
        "slot not released after disconnect"
    );
    let state = progress(&executor, "gone").await;
    assert_eq!(state["closed"], true);
    assert_eq!(state["finished"], false);
    assert!(state["produced"].as_u64().unwrap() < 1024 * 1024);
    let metrics = executor.metrics();
    assert_eq!(metrics.responses_cancelled, 1);
    assert!(metrics.backpressure_waits >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn many_paused_clients_stay_within_their_budgets() {
    let executor = executor(limits());
    let clients = 24;
    let mut streams = Vec::new();
    for i in 0..clients {
        let mut stream = executor
            .render(flood(&format!("many-{i}"), 16 * 1024 * 1024, &[]))
            .await
            .unwrap();
        assert!(matches!(
            stream.recv().await,
            Some(ResponseEvent::Head { .. })
        ));
        streams.push(stream);
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let held: usize = streams.iter().map(|s| s.buffered_bytes()).sum();
    assert!(
        held <= clients * (BUDGET + CHUNK),
        "{held} bytes held for {clients} paused clients"
    );
    for i in 0..clients {
        let state = progress(&executor, &format!("many-{i}")).await;
        assert!(
            state["produced"].as_u64().unwrap() as usize <= BUDGET + JS_HIGH_WATER_MARK + 2 * CHUNK
        );
    }
    drop(streams);
    assert!(
        eventually(Duration::from_secs(2), async || executor.metrics().active()
            == 0)
        .await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exception_mid_stream_aborts_the_response() {
    let executor = executor(limits());
    let response = collect(
        &executor,
        flood("throws", 4 * 1024 * 1024, &[("x-throw-after", "20")]),
    )
    .await;
    assert_eq!(response.status, 200);
    assert_eq!(response.error.as_deref(), Some("boom while streaming"));
    assert!(!response.ended);
    assert_eq!(response.body.len(), 20 * CHUNK);
    // The isolate is fine.
    assert_eq!(
        collect(&executor, request("errors.js", &[])).await.text(),
        "ok"
    );
    assert_eq!(executor.metrics().handler_errors, 1);
}
