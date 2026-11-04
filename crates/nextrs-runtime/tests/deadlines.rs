//! Request deadlines, queue deadlines and cancellation. These are per
//! request; CPU limits are per isolate (supervision.rs).

mod common;

use std::time::{Duration, Instant};

use common::*;
use nextrs_runtime::{Limits, RenderError};

fn limits(timeout_ms: u64) -> Limits {
    Limits {
        request_timeout: Some(Duration::from_millis(timeout_ms)),
        ..small_limits()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_promise_that_never_settles_gets_a_504_at_the_deadline() {
    let executor = executor(limits(200));
    let started = Instant::now();
    let response = collect(&executor, request("faults.js", &[("x-mode", "never")])).await;
    let elapsed = started.elapsed();
    assert_eq!(response.status, 504);
    assert_eq!(response.header("x-nextrs-error"), Some("request-timeout"));
    assert!(response.ended);
    assert!(
        elapsed >= Duration::from_millis(200) && elapsed < Duration::from_secs(2),
        "{elapsed:?}"
    );
    assert!(
        eventually(Duration::from_secs(1), async || executor.metrics().active()
            == 0)
        .await
    );
    assert_eq!(executor.metrics().request_timeouts, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_started_response_is_aborted_at_the_deadline() {
    let executor = executor(limits(200));
    let response = collect(
        &executor,
        request("faults.js", &[("x-mode", "never-after-head")]),
    )
    .await;
    assert_eq!(response.status, 200);
    assert_eq!(response.text(), "started");
    assert_eq!(response.error.as_deref(), Some("request deadline exceeded"));
}

/// The deadline covers waitUntil/after() work; past it the work is
/// abandoned and the slot released. Work that finishes in time runs.
#[tokio::test(flavor = "multi_thread")]
async fn wait_until_work_is_bounded_by_the_deadline() {
    let executor = executor(limits(300));
    let response = collect(&executor, request("wait-until.js", &[("x-mode", "never")])).await;
    assert_eq!(response.text(), "responded");
    assert_eq!(executor.metrics().active(), 1, "waitUntil keeps the slot");
    assert!(
        eventually(Duration::from_secs(2), async || executor.metrics().active()
            == 0)
        .await
    );
    assert_eq!(executor.metrics().wait_until_timeouts, 1);

    collect(
        &executor,
        request("wait-until.js", &[("x-id", "kept"), ("x-delay", "20")]),
    )
    .await;
    assert!(
        eventually(Duration::from_secs(2), async || executor.metrics().active()
            == 0)
        .await
    );
    let report = collect(&executor, request("wait-until.js", &[("x-mode", "report")])).await;
    assert_eq!(report.text(), r#"["kept"]"#);
}

#[tokio::test(flavor = "multi_thread")]
async fn timeouts_do_not_disturb_other_requests_in_the_isolate() {
    let executor = executor(limits(300));
    let hanging: Vec<_> = (0..10)
        .map(|_| {
            let executor = executor.clone();
            tokio::spawn(async move {
                collect(&executor, request("faults.js", &[("x-mode", "never")])).await
            })
        })
        .collect();
    for i in 0..50 {
        let id = format!("ok-{i}");
        let response = collect(&executor, request("context.js", &[("x-id", &id)])).await;
        assert_eq!(response.status, 200);
        assert!(response.text().contains(&format!("sync={id}")));
    }
    for task in hanging {
        assert_eq!(task.await.unwrap().status, 504);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_while_queued_never_executes_the_request() {
    let executor = executor(Limits {
        max_active_per_isolate: 1,
        request_timeout: None,
        ..small_limits()
    });
    let blocker = executor
        .render(request(
            "faults.js",
            &[("x-mode", "slow"), ("x-delay", "300")],
        ))
        .await
        .unwrap();
    // Queued behind the blocker, then abandoned (the client went away).
    let queued = tokio::time::timeout(
        Duration::from_millis(50),
        executor.render(request("flood.js", &[("x-id", "never-run")])),
    )
    .await;
    assert!(queued.is_err(), "should still be queued");
    assert_eq!(
        executor.metrics().queued,
        0,
        "abandoned waiter left the queue"
    );
    drain(blocker).await;
    let report = collect(
        &executor,
        request("flood-progress.js", &[("x-id", "never-run")]),
    )
    .await;
    assert_eq!(report.text(), "null");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_during_execution_releases_the_slot() {
    let executor = executor(Limits {
        request_timeout: None,
        ..small_limits()
    });
    let stream = executor
        .render(request("faults.js", &[("x-mode", "never")]))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(executor.metrics().active(), 1);
    drop(stream);
    assert!(
        eventually(Duration::from_secs(1), async || executor.metrics().active()
            == 0)
        .await
    );
    assert_eq!(executor.metrics().responses_cancelled, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_deadline_and_queue_bound_are_enforced() {
    let executor = executor(Limits {
        max_active_per_isolate: 2,
        max_queued: 2,
        queue_timeout: Duration::from_millis(200),
        request_timeout: None,
        ..small_limits()
    });
    let mut held = Vec::new();
    for _ in 0..2 {
        held.push(
            executor
                .render(request("faults.js", &[("x-mode", "never")]))
                .await
                .unwrap(),
        );
    }
    let waiting: Vec<_> = (0..2)
        .map(|_| {
            let executor = executor.clone();
            tokio::spawn(async move {
                let started = Instant::now();
                let result = executor.render(request("faults.js", &[])).await.map(|_| ());
                (result, started.elapsed())
            })
        })
        .collect();
    assert!(
        eventually(Duration::from_secs(1), async || executor.metrics().queued
            == 2)
        .await
    );
    assert_eq!(
        executor
            .render(request("faults.js", &[]))
            .await
            .unwrap_err(),
        RenderError::Overloaded
    );
    for task in waiting {
        let (result, waited) = task.await.unwrap();
        assert_eq!(result.unwrap_err(), RenderError::QueueTimeout);
        assert!(waited >= Duration::from_millis(200));
    }
    let metrics = executor.metrics();
    assert_eq!(metrics.rejected_overloaded, 1);
    assert_eq!(metrics.rejected_queue_timeout, 2);
    drop(held);
    assert!(
        eventually(Duration::from_secs(1), async || executor.metrics().active()
            == 0)
        .await
    );
    assert_eq!(
        collect(&executor, request("faults.js", &[])).await.text(),
        "ok"
    );
}
