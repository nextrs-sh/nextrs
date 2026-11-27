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
    // The slot is released when JavaScript reports the request done,
    // shortly after the response ended.
    assert!(eventually(Duration::from_secs(5), async || executor.is_idle()).await);
    let gcs_before: Vec<u64> = executor
        .metrics()
        .executors
        .iter()
        .map(|e| e.full_gcs)
        .collect();
    let report = executor.reclaim_memory(ReclaimLevel::Idle).await;
    assert_eq!(report.isolates, 2, "{report:?}");
    let metrics = executor.metrics();
    assert_eq!(metrics.memory_reclaims_idle, 1);
    assert_eq!(metrics.memory_reclaims_pressure, 0);
    assert!(metrics.memory_reclaim_seconds_total > 0.0);
    assert!(metrics.executors.iter().all(|e| e.restarts == 0));
    // Low-memory collections are full collections, in every isolate.
    for (e, before) in metrics.executors.iter().zip(&gcs_before) {
        assert!(
            e.full_gcs > *before,
            "slot {}: {} -> {}",
            e.slot,
            before,
            e.full_gcs
        );
    }
    let text = metrics.to_prometheus();
    assert!(text.contains("nextrs_executor_memory_reclaims_total{trigger=\"idle\"} 1"));
    assert!(text.contains("nextrs_executor_heap_limit_bytes{slot=\"0\"}"));
}

#[tokio::test(flavor = "multi_thread")]
async fn reclamation_keeps_module_state() {
    // One isolate, so both requests are served by the same one.
    let executor = executor(limits(1));
    let before = whoami(&executor).await;
    assert!(eventually(Duration::from_secs(5), async || executor.is_idle()).await);
    for level in [ReclaimLevel::Idle, ReclaimLevel::Pressure] {
        assert_eq!(executor.reclaim_memory(level).await.isolates, 1);
    }
    let after = whoami(&executor).await;
    let (old_id, _) = before.split_once(':').unwrap();
    let (new_id, served) = after.split_once(':').unwrap();
    assert_eq!(old_id, new_id, "{before} -> {after}");
    assert_eq!(served, "2", "module state lost: {before} -> {after}");
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
    const MIB: usize = 1024 * 1024;
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
    // V8 never sizes an old generation below 256 MiB from memory alone:
    // 256 MiB of memory gives a heap limit above the memory itself (Node.js
    // reports 259 MiB in a 256 MiB container). This is why `auto` caps it.
    assert!((256 * MIB + 1..=300 * MIB).contains(&small), "{small}");
    // A cap lowers the limit V8 derives from the memory...
    let budget = nextrs_runtime::heap_budget(256 * 1024 * 1024, 1, 32 * 1024 * 1024);
    let capped = heap_limit(&executor(Limits {
        heap_limit_bytes: Some(usize::try_from(budget.heap_limit).unwrap()),
        heap_memory_bytes: Some(budget.sizing_memory),
        ..limits(1)
    }))
    .await;
    assert!(
        (100 * MIB..=125 * MIB).contains(&capped),
        "budget {budget:?}: {capped}"
    );
    // ...and never raises it.
    let uncapped = heap_limit(&executor(Limits {
        heap_limit_bytes: Some(8 * 1024 * MIB),
        heap_memory_bytes: Some(256 * 1024 * 1024),
        ..limits(1)
    }))
    .await;
    assert_eq!(uncapped, small);
    // A cap below the young generation V8 would pick for the memory keeps
    // an old generation (the young one shrinks to a quarter of the cap).
    let tight = heap_limit(&executor(Limits {
        heap_limit_bytes: Some(96 * MIB),
        heap_memory_bytes: Some(4 * 1024 * 1024 * 1024),
        ..limits(1)
    }))
    .await;
    assert!(tight <= 110 * MIB, "{tight}");
    // An explicit limit alone keeps its meaning.
    let explicit = heap_limit(&executor(Limits {
        heap_limit_bytes: Some(96 * MIB),
        ..limits(1)
    }))
    .await;
    assert!(explicit <= 110 * MIB, "{explicit}");
}
