//! Several isolates: dispatch, isolation across isolates, saturation, and
//! one isolate failing while the others keep serving.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use nextrs_runtime::{Executor, Limits, RenderError};

fn pool(isolates: usize) -> Arc<Executor> {
    executor(Limits {
        isolates,
        ..small_limits()
    })
}

fn isolate_of(whoami: &str) -> String {
    whoami.split(':').next().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_requests_spread_over_every_isolate() {
    let executor = pool(3);
    assert_eq!(executor.metrics().ready_executors(), 3);
    let tasks: Vec<_> = (0..60)
        .map(|_| {
            let executor = executor.clone();
            tokio::spawn(async move {
                collect(
                    &executor,
                    request("faults.js", &[("x-mode", "whoami"), ("x-delay", "30")]),
                )
                .await
                .text()
            })
        })
        .collect();
    let mut per_isolate: BTreeMap<String, usize> = BTreeMap::new();
    for task in tasks {
        *per_isolate
            .entry(isolate_of(&task.await.unwrap()))
            .or_default() += 1;
    }
    assert_eq!(per_isolate.len(), 3, "{per_isolate:?}");
    // Least-outstanding dispatch keeps them close to even.
    assert!(per_isolate.values().all(|n| *n >= 15), "{per_isolate:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn async_context_stays_isolated_within_and_across_isolates() {
    let executor = pool(3);
    for round in 0..2 {
        let tasks: Vec<_> = (0..300)
            .map(|i| {
                let executor = executor.clone();
                tokio::spawn(async move {
                    let id = format!("p{round}-{i}");
                    let delay = ((i * 7919) % 23).to_string();
                    let response = collect(
                        &executor,
                        request("context.js", &[("x-id", &id), ("x-delay", &delay)]),
                    )
                    .await;
                    (id, response.text())
                })
            })
            .collect();
        for task in tasks {
            let (id, body) = task.await.unwrap();
            for line in body.lines() {
                if let Some((label, value)) = line.split_once('=') {
                    match label {
                        "outside-resource" | "exit" => assert_eq!(value, "undefined"),
                        _ => assert_eq!(value, id, "{id} saw another context: {line}"),
                    }
                }
            }
            assert!(body.contains(&format!("sync={id}")));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn module_state_is_per_isolate() {
    let executor = pool(2);
    let mut isolates = BTreeSet::new();
    let tasks: Vec<_> = (0..20)
        .map(|_| {
            let executor = executor.clone();
            tokio::spawn(async move {
                collect(
                    &executor,
                    request("faults.js", &[("x-mode", "whoami"), ("x-delay", "20")]),
                )
                .await
                .text()
            })
        })
        .collect();
    for task in tasks {
        isolates.insert(isolate_of(&task.await.unwrap()));
    }
    assert_eq!(isolates.len(), 2, "each isolate has its own globalThis");
}

#[tokio::test(flavor = "multi_thread")]
async fn saturated_pools_queue_then_refuse() {
    let executor = executor(Limits {
        isolates: 2,
        max_active_per_isolate: 3,
        max_queued: 4,
        queue_timeout: Duration::from_millis(300),
        request_timeout: None,
        ..small_limits()
    });
    let mut held = Vec::new();
    for _ in 0..6 {
        held.push(
            executor
                .render(request("faults.js", &[("x-mode", "never")]))
                .await
                .unwrap(),
        );
    }
    let metrics = executor.metrics();
    assert_eq!(metrics.active(), 6);
    assert!(metrics.executors.iter().all(|e| e.active == 3));
    let queued: Vec<_> = (0..4)
        .map(|_| {
            let executor = executor.clone();
            // Each holds its slot longer than the queue timeout once served.
            tokio::spawn(async move {
                executor
                    .render(request(
                        "faults.js",
                        &[("x-mode", "slow"), ("x-delay", "600")],
                    ))
                    .await
                    .map(drain)
            })
        })
        .collect();
    assert!(
        eventually(Duration::from_secs(1), async || executor.metrics().queued
            == 4)
        .await
    );
    assert_eq!(
        executor
            .render(request("faults.js", &[]))
            .await
            .unwrap_err(),
        RenderError::Overloaded
    );
    // Freeing two slots serves the two oldest waiters.
    held.truncate(4);
    let mut served = 0;
    let mut timed_out = 0;
    for task in queued {
        match task.await.unwrap() {
            Ok(response) => {
                assert_eq!(response.await.text(), "slow done");
                served += 1;
            }
            Err(RenderError::QueueTimeout) => timed_out += 1,
            Err(other) => panic!("{other}"),
        }
    }
    assert_eq!((served, timed_out), (2, 2));
}

/// One isolate keeps failing; the other keeps answering. Requests to the
/// failing one fail or are re-queued, never hang, and the pool recovers.
#[tokio::test(flavor = "multi_thread")]
async fn a_failing_isolate_does_not_take_the_pool_down() {
    let executor = pool(2);
    let traffic: Vec<_> = (0..200)
        .map(|i| {
            let executor = executor.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis((i % 50) * 4)).await;
                let id = format!("t{i}");
                let mode = if i % 40 == 0 { "uncaught" } else { "whoami" };
                match executor
                    .render(request("faults.js", &[("x-mode", mode), ("x-id", &id)]))
                    .await
                {
                    Ok(stream) => (mode, drain(stream).await),
                    Err(error) => panic!("refused: {error}"),
                }
            })
        })
        .collect();
    // Every request completes one of three ways; none hangs (each task
    // is awaited). A request still in a failing isolate's inbox was never
    // delivered: the HTTP layer answers it with a retry-safe 503, and
    // with a 502 if JavaScript had received it.
    let (mut ok, mut failed_delivered, mut failed_undelivered) = (0, 0, 0);
    for task in traffic {
        let (mode, response) = task.await.unwrap();
        if mode != "whoami" {
            continue;
        }
        match (response.ended, response.delivered) {
            (true, _) => ok += 1,
            (false, true) => failed_delivered += 1,
            (false, false) => failed_undelivered += 1,
        }
    }
    // How many requests share a failing isolate depends on timing and
    // machine load (5 failures among 200 requests); most must succeed.
    assert!(
        ok >= 100,
        "only {ok} succeeded ({failed_delivered} failed in JavaScript, \
         {failed_undelivered} before delivery)"
    );
    assert!(
        eventually(Duration::from_secs(5), async || executor
            .metrics()
            .ready_executors()
            == 2)
        .await
    );
    let metrics = executor.metrics();
    let restarts: u64 = metrics.executors.iter().map(|e| e.restarts).sum();
    assert!(restarts >= 1, "{metrics:?}");
    assert!(whoami(&executor).await.contains(':'));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stalled_isolate_only_fails_its_own_requests() {
    let executor = pool(2);
    // Occupy each isolate with one slow request so the loop lands on a
    // known isolate alongside it.
    let slow: Vec<_> = futures_util::future::join_all((0..2).map(|_| {
        executor.render(request(
            "faults.js",
            &[("x-mode", "slow"), ("x-delay", "1500")],
        ))
    }))
    .await
    .into_iter()
    .map(Result::unwrap)
    .collect();
    collect(&executor, request("faults.js", &[("x-mode", "loop")])).await;
    let outcomes: Vec<bool> = futures_util::future::join_all(slow.into_iter().map(drain))
        .await
        .into_iter()
        .map(|r| r.ended)
        .collect();
    // Exactly one slow request shared the stalled isolate.
    assert_eq!(
        outcomes.iter().filter(|ended| **ended).count(),
        1,
        "{outcomes:?}"
    );
    assert!(
        eventually(Duration::from_secs(5), async || executor
            .metrics()
            .ready_executors()
            == 2)
        .await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_stops_every_isolate() {
    let executor = pool(3);
    whoami(&executor).await;
    let stopper = {
        let executor = executor.clone();
        tokio::task::spawn_blocking(move || executor.shutdown())
    };
    stopper.await.unwrap();
    assert_eq!(executor.metrics().ready_executors(), 0);
    assert_eq!(
        executor
            .render(request("faults.js", &[]))
            .await
            .unwrap_err(),
        RenderError::ShuttingDown
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn metrics_report_heap_statistics_per_isolate() {
    let executor = pool(2);
    for i in 0..20 {
        collect(
            &executor,
            request("context.js", &[("x-id", &format!("h{i}"))]),
        )
        .await;
    }
    assert!(
        eventually(Duration::from_secs(3), async || executor
            .metrics()
            .executors
            .iter()
            .all(
                |e| e.heap_used_bytes > 0 && e.heap_total_bytes >= e.heap_used_bytes
            ))
        .await,
        "{:#?}",
        executor.metrics().executors
    );
    let text = executor.metrics().to_prometheus();
    assert!(text.contains("nextrs_executor_heap_used_bytes{slot=\"1\"}"));
    assert!(text.contains("# TYPE nextrs_executor_external_bytes gauge"));
}
