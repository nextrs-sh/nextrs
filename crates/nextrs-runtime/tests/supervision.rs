//! Isolate failure and recovery. Each failure kind must take the isolate
//! out of service, fail its in-flight requests predictably, and bring a
//! fresh isolate (new heap, reloaded modules) back without a restart loop.
//! Ordinary handler exceptions must not restart anything.

mod common;

use std::time::{Duration, Instant};

use common::*;
use nextrs_runtime::{Executor, ExecutorError, FailureReason, Limits, RenderError};

fn failures(executor: &Executor, reason: FailureReason) -> u64 {
    executor.metrics().executors[0]
        .failures
        .iter()
        .find(|(name, _)| *name == reason.as_str())
        .map_or(0, |(_, n)| *n)
}

/// Fails the isolate with `mode`, then checks that it comes back as a
/// fresh isolate.
async fn fails_and_recovers(mode: &str, reason: FailureReason, limits: Limits) {
    let executor = executor(limits);
    let before = whoami(&executor).await;
    let failing = collect(&executor, request("faults.js", &[("x-mode", mode)])).await;
    assert!(
        !failing.ended || failing.status == 200,
        "{mode}: unexpected {failing:?}"
    );
    assert!(
        eventually(Duration::from_secs(5), async || failures(&executor, reason)
            == 1)
        .await,
        "{mode}: failure not recorded: {:?}",
        executor.metrics().executors
    );
    assert!(
        eventually(Duration::from_secs(5), async || executor
            .metrics()
            .ready_executors()
            == 1)
        .await,
        "{mode}: isolate not restarted"
    );
    let after = whoami(&executor).await;
    let (old_id, _) = before.split_once(':').unwrap();
    let (new_id, served) = after.split_once(':').unwrap();
    assert_ne!(old_id, new_id, "{mode}: same isolate after restart");
    assert_eq!(served, "1", "{mode}: module state survived the restart");
    let metrics = executor.metrics();
    assert_eq!(metrics.executors[0].restarts, 1);
    assert_eq!(metrics.executors[0].generation, 2);
    assert!(
        metrics.executors[0]
            .last_failure
            .as_deref()
            .is_some_and(|f| f.starts_with(reason.as_str())),
        "{:?}",
        metrics.executors[0].last_failure
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_uncaught_exception_restarts_the_isolate() {
    fails_and_recovers("uncaught", FailureReason::Uncaught, small_limits()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unhandled_rejection_restarts_the_isolate() {
    fails_and_recovers("rejection", FailureReason::Uncaught, small_limits()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cpu_bound_loop_is_terminated_and_replaced() {
    fails_and_recovers("loop", FailureReason::CpuStall, small_limits()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_endless_microtask_chain_is_terminated_and_replaced() {
    fails_and_recovers("microtask-loop", FailureReason::CpuStall, small_limits()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reaching_the_heap_limit_replaces_the_isolate_instead_of_aborting() {
    // Allocating up to the limit is one long CPU-bound turn (with GC
    // churn); under load it outlasted the 500 ms stall limit, and the
    // isolate was then (correctly) failed as a stall, not at its heap
    // limit. Only the heap limit is under test here.
    let limits = Limits {
        heap_limit_bytes: Some(64 * 1024 * 1024),
        stall_timeout: None,
        ..small_limits()
    };
    fails_and_recovers("oom", FailureReason::HeapLimit, limits).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_executor_thread_panic_is_recovered() {
    fails_and_recovers("panic", FailureReason::Panic, small_limits()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn handler_exceptions_do_not_restart_the_isolate() {
    let executor = executor(small_limits());
    let before = whoami(&executor).await;
    for mode in ["before-head", "after-head", "unsupported", "fetch"] {
        collect(&executor, request("errors.js", &[("x-mode", mode)])).await;
    }
    let after = whoami(&executor).await;
    assert_eq!(before.split(':').next(), after.split(':').next());
    assert_eq!(executor.metrics().executors[0].restarts, 0);
    assert_eq!(executor.metrics().handler_errors, 2);
}

/// Requests in the failing isolate end without `End`, and say they were
/// delivered (so the HTTP layer answers 502, not a retryable 503).
#[tokio::test(flavor = "multi_thread")]
async fn in_flight_requests_fail_with_their_isolate() {
    let executor = executor(small_limits());
    let mut in_flight = Vec::new();
    for _ in 0..5 {
        in_flight.push(
            executor
                .render(request(
                    "faults.js",
                    &[("x-mode", "slow"), ("x-delay", "5000")],
                ))
                .await
                .unwrap(),
        );
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    collect(&executor, request("faults.js", &[("x-mode", "loop")])).await;
    for stream in in_flight {
        assert!(stream.was_delivered());
        let response = drain(stream).await;
        assert!(!response.ended && response.error.is_none(), "{response:?}");
    }
    assert!(
        eventually(Duration::from_secs(5), async || executor
            .metrics()
            .ready_executors()
            == 1)
        .await
    );
    assert_eq!(
        collect(&executor, request("faults.js", &[])).await.text(),
        "ok"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_response_channels_end_streams_without_completion() {
    let executor = executor(small_limits());
    let response = collect(
        &executor,
        request("faults.js", &[("x-mode", "drop-responses")]),
    )
    .await;
    assert_eq!(response.text(), "before");
    assert!(!response.ended && response.error.is_none());
    // The isolate itself is healthy and keeps serving.
    assert_eq!(
        collect(&executor, request("faults.js", &[])).await.text(),
        "ok"
    );
    assert_eq!(executor.metrics().executors[0].restarts, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn bootstrap_failure_fails_startup() {
    let mut config = config(small_limits());
    config.bootstrap = Some(fixture().join("dist/throwing-bootstrap.js"));
    match Executor::start(config) {
        Err(ExecutorError::Bootstrap(error)) => {
            assert!(error.contains("bootstrap exploded"), "{error}")
        }
        other => panic!("expected a bootstrap error, got {other:?}"),
    }
}

/// A bootstrap that runs longer than the stall limit is not a stall: it
/// has its own deadline (the Phase 1E finding: healthy bootstraps under
/// contention were killed by the 2-3 s stall limit).
#[tokio::test(flavor = "multi_thread")]
async fn a_slow_bootstrap_is_not_judged_by_the_stall_limit() {
    let mut config = config(Limits {
        stall_timeout: Some(Duration::from_millis(300)),
        bootstrap_timeout: Some(Duration::from_secs(20)),
        ..small_limits()
    });
    config.bootstrap = Some(fixture().join("dist/slow-bootstrap.js"));
    config
        .env
        .insert("NEXTRS_TEST_BOOT_MS".into(), "1200".into());
    let executor = Executor::start(config).expect("a slow bootstrap starts");
    assert_eq!(
        collect(&executor, request("faults.js", &[])).await.text(),
        "ok"
    );
    assert_eq!(executor.metrics().executors[0].restarts, 0);
}

/// A bootstrap that never finishes fails startup at its deadline instead
/// of leaving the executor unavailable.
#[tokio::test(flavor = "multi_thread")]
async fn a_bootstrap_that_never_finishes_fails_at_its_deadline() {
    let mut config = config(Limits {
        bootstrap_timeout: Some(Duration::from_millis(800)),
        ..small_limits()
    });
    config.bootstrap = Some(fixture().join("dist/slow-bootstrap.js"));
    config.env.insert("NEXTRS_TEST_BOOT_MS".into(), "-1".into());
    let started = Instant::now();
    match Executor::start(config) {
        Err(ExecutorError::Bootstrap(error)) => {
            assert!(error.contains("did not finish its bootstrap"), "{error}")
        }
        other => panic!("expected a bootstrap deadline error, got {other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(10));
}

/// Repeated failures back off exponentially (20, 40, 80, 160, 200 ms
/// here) instead of restarting in a tight loop.
#[tokio::test(flavor = "multi_thread")]
async fn repeated_failures_back_off() {
    let executor = executor(small_limits());
    let started = Instant::now();
    for round in 1..=5u64 {
        assert!(
            eventually(Duration::from_secs(5), async || executor
                .metrics()
                .ready_executors()
                == 1)
            .await,
            "round {round}: not ready"
        );
        collect(&executor, request("faults.js", &[("x-mode", "uncaught")])).await;
        assert!(
            eventually(Duration::from_secs(5), async || failures(
                &executor,
                FailureReason::Uncaught
            ) == round)
            .await
        );
    }
    assert!(
        eventually(Duration::from_secs(5), async || executor
            .metrics()
            .ready_executors()
            == 1)
        .await
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(20 + 40 + 80 + 160),
        "restarts too fast: {elapsed:?}"
    );
    assert_eq!(executor.metrics().executors[0].restarts, 5);
}

/// While the only isolate restarts, requests wait in the queue and are
/// served by the new isolate.
#[tokio::test(flavor = "multi_thread")]
async fn requests_during_a_restart_wait_for_the_new_isolate() {
    let executor = executor(Limits {
        restart_backoff_initial: Duration::from_millis(300),
        ..small_limits()
    });
    collect(&executor, request("faults.js", &[("x-mode", "uncaught")])).await;
    assert!(
        eventually(Duration::from_secs(2), async || executor
            .metrics()
            .ready_executors()
            == 0)
        .await
    );
    let response = collect(&executor, request("faults.js", &[])).await;
    assert_eq!(response.text(), "ok");
    assert!(executor.metrics().queue_waits >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn graceful_shutdown_finishes_in_flight_requests() {
    let executor = executor(small_limits());
    let slow = executor
        .render(request(
            "faults.js",
            &[("x-mode", "slow"), ("x-delay", "300")],
        ))
        .await
        .unwrap();
    let stopper = {
        let executor = executor.clone();
        tokio::task::spawn_blocking(move || executor.shutdown())
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        executor
            .render(request("faults.js", &[]))
            .await
            .unwrap_err(),
        RenderError::ShuttingDown
    );
    let response = drain(slow).await;
    assert!(response.ended);
    assert_eq!(response.text(), "slow done");
    stopper.await.unwrap();
    assert_eq!(executor.metrics().executors[0].state, "restarting");
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_terminates_requests_that_outlive_the_grace_period() {
    let executor = executor(Limits {
        shutdown_grace: Duration::from_millis(200),
        request_timeout: None,
        ..small_limits()
    });
    let stuck = executor
        .render(request("faults.js", &[("x-mode", "never")]))
        .await
        .unwrap();
    let started = Instant::now();
    let stopper = {
        let executor = executor.clone();
        tokio::task::spawn_blocking(move || executor.shutdown())
    };
    let response = drain(stuck).await;
    assert!(!response.ended);
    stopper.await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    // A deliberate shutdown is not a failure.
    assert!(
        executor.metrics().executors[0]
            .failures
            .iter()
            .all(|(_, n)| *n == 0)
    );
}
