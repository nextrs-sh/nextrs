//! Memory reclamation and heap sizing (RE-2/RE-3). A reclamation must reach
//! every ready isolate, must not restart one (module state survives), and
//! must leave the isolates serving; heap sizing from a memory amount must
//! change V8's limit the way Node.js's cgroup-based sizing does.

mod common;

use std::time::Duration;

use common::*;
use nextrs_runtime::{Limits, ReclaimLevel};

fn limits(isolates: usize) -> Limits {
    Limits {
        isolates,
        ..small_limits()
    }
}

async fn heap_limit(executor: &nextrs_runtime::Executor) -> usize {
    // Sampled by the isolate at most once a second, and on reclamation.
    assert!(
        eventually(Duration::from_secs(5), async || executor
            .metrics()
            .executors[0]
            .heap_limit_bytes
            > 0)
        .await
    );
    executor.metrics().executors[0].heap_limit_bytes
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_reclamation_reaches_every_isolate_and_keeps_its_state() {
    let executor = executor(limits(2));
    let before = whoami(&executor).await;
    // The slot is released when JavaScript reports the request done,
    // shortly after the response ended.
    assert!(eventually(Duration::from_secs(5), async || executor.is_idle()).await);
    let report = executor.reclaim_memory(ReclaimLevel::Idle).await;
    assert_eq!(report.isolates, 2, "{report:?}");
    let metrics = executor.metrics();
    assert_eq!(metrics.memory_reclaims_idle, 1);
    assert_eq!(metrics.memory_reclaims_pressure, 0);
    assert!(metrics.memory_reclaim_seconds_total > 0.0);
    // Same isolate, module state kept: a collection is not a restart.
    let after = whoami(&executor).await;
    let (old_id, _) = before.split_once(':').unwrap();
    let (new_id, served) = after.split_once(':').unwrap();
    if old_id == new_id {
        assert_eq!(served, "2", "module state lost: {before} -> {after}");
    }
    assert!(metrics.executors.iter().all(|e| e.restarts == 0));
    // Low-memory collections are full collections.
    assert!(metrics.executors.iter().all(|e| e.full_gcs > 0));
    let text = metrics.to_prometheus();
    assert!(text.contains("nextrs_executor_memory_reclaims_total{trigger=\"idle\"} 1"));
    assert!(text.contains("nextrs_executor_heap_limit_bytes{slot=\"0\"}"));
}

#[tokio::test(flavor = "multi_thread")]
async fn pressure_reclamation_works_and_repeats() {
    let executor = executor(limits(1));
    for _ in 0..3 {
        let report = executor.reclaim_memory(ReclaimLevel::Pressure).await;
        assert_eq!(report.isolates, 1);
    }
    assert_eq!(executor.metrics().memory_reclaims_pressure, 3);
    let response = collect(&executor, request("faults.js", &[("x-mode", "whoami")])).await;
    assert!(response.ended, "{response:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn reclamation_while_requests_wait_does_not_lose_them() {
    let executor = executor(limits(1));
    let mut pending = Vec::new();
    for _ in 0..20 {
        pending.push(executor.render(request("faults.js", &[("x-mode", "whoami")])));
    }
    let reclaim = executor.reclaim_memory(ReclaimLevel::Pressure);
    let (report, responses) = tokio::join!(reclaim, futures_util::future::join_all(pending));
    assert_eq!(report.isolates, 1);
    for response in responses {
        let collected = drain(response.expect("admitted")).await;
        assert!(collected.ended && collected.status == 200, "{collected:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn heap_sized_from_memory_follows_v8s_defaults_for_that_memory() {
    let v8_default = heap_limit(&executor(limits(1))).await;
    let small = heap_limit(&executor(Limits {
        heap_memory_bytes: Some(256 * 1024 * 1024),
        ..limits(1)
    }))
    .await;
    let large = heap_limit(&executor(Limits {
        heap_memory_bytes: Some(4 * 1024 * 1024 * 1024),
        ..limits(1)
    }))
    .await;
    assert!(small < large, "256 MiB: {small}, 4 GiB: {large}");
    // V8 never sizes a heap below its minimum; 256 MiB of memory gives a
    // limit near it (Node.js reports 259 MiB in a 256 MiB container).
    assert!(small <= 512 * 1024 * 1024, "{small}");
    // An explicit limit wins over sizing from memory.
    let explicit = heap_limit(&executor(Limits {
        heap_limit_bytes: Some(96 * 1024 * 1024),
        heap_memory_bytes: Some(4 * 1024 * 1024 * 1024),
        ..limits(1)
    }))
    .await;
    assert!(explicit < small.max(v8_default), "{explicit}");
}
