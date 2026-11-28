//! Memory-aware admission (RE-4B) with real isolates and a scripted memory
//! probe: refusal while others run, the progress guarantee, deferral and
//! FIFO order, queued requests refused when memory turns critical,
//! recovery, the queue byte bound, degraded readings, cancellation, Server
//! Actions never executed when refused (and never twice), and isolates
//! near their heap limit passed over.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use nextrs_runtime::{
    Executor, Limits, MemoryAdmission, MemoryProbe, MemoryReading, RenderError, RenderRequest,
};

const MIB: u64 = 1024 * 1024;
const BUDGET: u64 = 1000 * MIB;

/// A memory probe the test sets: usage in MiB of a 1000 MiB limit, or
/// failing reads.
struct Probe(Mutex<Option<u64>>);

impl Probe {
    fn at(usage_mib: u64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Some(usage_mib * MIB))))
    }

    fn set(&self, usage_mib: u64) {
        *self.0.lock().unwrap() = Some(usage_mib * MIB);
    }

    fn fail(&self) {
        *self.0.lock().unwrap() = None;
    }
}

impl MemoryProbe for Probe {
    fn read(&self, _full: bool) -> Option<MemoryReading> {
        let usage = (*self.0.lock().unwrap())?;
        Some(MemoryReading {
            limit: Some(BUDGET),
            usage,
            inactive_file: Some(0),
        })
    }

    fn describe(&self) -> String {
        "test probe".into()
    }
}

fn admission(probe: &Arc<Probe>) -> MemoryAdmission {
    MemoryAdmission {
        sample_interval: Duration::ZERO,
        stat_interval: Duration::ZERO,
        cooldown: Duration::ZERO,
        probe: Some(probe.clone()),
        ..MemoryAdmission::default()
    }
}

fn start(
    probe: &Arc<Probe>,
    tune: impl FnOnce(&mut MemoryAdmission, &mut Limits),
) -> Arc<Executor> {
    let mut gate = admission(probe);
    let mut limits = Limits {
        request_timeout: None,
        ..small_limits()
    };
    tune(&mut gate, &mut limits);
    limits.memory_admission = Some(gate);
    executor(limits)
}

fn job(id: &str, delay_ms: u64) -> RenderRequest {
    let delay = delay_ms.to_string();
    request(
        "admission.js",
        &[("x-id", id), ("x-delay", &delay), ("x-log", "1")],
    )
}

async fn runs(executor: &Executor, id: &str) -> u64 {
    collect(
        executor,
        request("admission.js", &[("x-id", id), ("x-report", "1")]),
    )
    .await
    .text()
    .parse()
    .unwrap()
}

fn rejected(executor: &Executor, class: &str) -> u64 {
    let admission = executor.metrics().admission.expect("admission on");
    admission
        .rejected
        .iter()
        .find(|(c, _)| *c == class)
        .map_or(0, |(_, n)| *n)
}

#[tokio::test(flavor = "multi_thread")]
async fn off_by_default() {
    let executor = executor(small_limits());
    assert!(executor.metrics().admission.is_none());
    assert_eq!(collect(&executor, job("a", 0)).await.text(), "ok");
}

#[tokio::test(flavor = "multi_thread")]
async fn critical_refuses_while_others_run_and_always_admits_one() {
    let probe = Probe::at(990);
    let executor = start(&probe, |_, _| {});
    assert_eq!(executor.metrics().admission.unwrap().state, "critical");
    // Nothing runs: admitted anyway (progress).
    let first = executor
        .render(job("first", 300))
        .await
        .expect("progress admission");
    // Something runs: refused at once, before JavaScript.
    let refused = executor.render(job("second", 0)).await;
    assert_eq!(refused.err(), Some(RenderError::MemoryPressure));
    assert_eq!(drain(first).await.text(), "ok");
    assert_eq!(rejected(&executor, "render"), 1);
    assert!(executor.metrics().admission.unwrap().progress_admissions >= 1);
    assert_eq!(executor.metrics().rejected_memory_pressure, 1);
    probe.set(100);
    assert_eq!(
        runs(&executor, "second").await,
        0,
        "refused request never ran"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_server_action_never_runs_and_an_admitted_one_runs_once() {
    let probe = Probe::at(990);
    let executor = start(&probe, |_, _| {});
    let blocker = executor.render(job("blocker", 300)).await.unwrap();
    let mut action = request(
        "admission.js",
        &[
            ("x-id", "action"),
            ("next-action", "7f00"),
            ("content-length", "2"),
        ],
    );
    action.method = "POST".into();
    action.body = nextrs_runtime::RequestBody::Full(bytes::Bytes::from_static(b"{}"));
    let refused = executor.render(action).await;
    assert_eq!(refused.err(), Some(RenderError::MemoryPressure));
    assert_eq!(rejected(&executor, "server_action"), 1);
    drain(blocker).await;
    // Memory recovers; the client retries once; it runs exactly once.
    probe.set(100);
    assert_eq!(runs(&executor, "action").await, 0);
    let mut retry = request(
        "admission.js",
        &[("x-id", "action"), ("next-action", "7f00")],
    );
    retry.method = "POST".into();
    assert_eq!(collect(&executor, retry).await.text(), "ok");
    assert_eq!(runs(&executor, "action").await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn elevated_caps_concurrency_and_the_rest_run_in_arrival_order() {
    let probe = Probe::at(880);
    let executor = start(&probe, |gate, limits| {
        gate.pressure_max_active = Some(1);
        limits.max_active_per_isolate = 8;
    });
    let a = executor.render(job("a", 200)).await.unwrap();
    // Enqueued one after another (spawned tasks may start in any order),
    // so arrival order is b, c, d.
    let mut pending = Vec::new();
    for (n, id) in ["b", "c", "d"].into_iter().enumerate() {
        let task = {
            let executor = executor.clone();
            tokio::spawn(async move { collect(&executor, job(id, 0)).await.text() })
        };
        assert!(
            eventually(Duration::from_secs(2), async || executor.metrics().queued
                == n + 1)
            .await,
            "{id} waits behind the cap"
        );
        pending.push(task);
    }
    assert_eq!(drain(a).await.text(), "ok");
    for task in pending {
        assert_eq!(task.await.unwrap(), "ok");
    }
    let order = collect(
        &executor,
        request("admission.js", &[("x-report-order", "1")]),
    )
    .await
    .text();
    assert_eq!(order, r#"["a","b","c","d"]"#);
    let snapshot = executor.metrics().admission.unwrap();
    assert!(snapshot.deferred >= 3, "{snapshot:?}");
    assert_eq!(snapshot.queued_bytes, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn queued_requests_behind_a_running_one_are_refused_at_critical() {
    let probe = Probe::at(880);
    let executor = start(&probe, |gate, limits| {
        gate.pressure_max_active = Some(2);
        limits.max_active_per_isolate = 8;
    });
    let long = executor.render(job("long", 2000)).await.unwrap();
    let short = executor.render(job("short", 500)).await.unwrap();
    let queued = {
        let executor = executor.clone();
        tokio::spawn(async move { executor.render(job("waiting", 0)).await.err() })
    };
    assert!(
        eventually(Duration::from_secs(2), async || executor.metrics().queued
            == 1)
        .await
    );
    probe.set(990);
    // `short` ends while `long` still runs: the waiter is re-checked and
    // refused, not admitted.
    drain(short).await;
    assert_eq!(queued.await.unwrap(), Some(RenderError::MemoryPressure));
    drain(long).await;
    probe.set(100);
    assert_eq!(runs(&executor, "waiting").await, 0);
    assert_eq!(executor.metrics().queued, 0);
    assert_eq!(executor.metrics().admission.unwrap().queued_bytes, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn admission_recovers_when_memory_falls() {
    let probe = Probe::at(990);
    let executor = start(&probe, |_, _| {});
    let blocker = executor.render(job("blocker", 300)).await.unwrap();
    assert_eq!(
        executor.render(job("x", 0)).await.err(),
        Some(RenderError::MemoryPressure)
    );
    probe.set(200);
    assert_eq!(collect(&executor, job("y", 0)).await.text(), "ok");
    assert_eq!(executor.metrics().admission.unwrap().state, "normal");
    drain(blocker).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_queue_is_bounded_in_bytes() {
    let probe = Probe::at(880);
    let executor = start(&probe, |gate, limits| {
        gate.pressure_max_active = Some(1);
        gate.max_queued_bytes = 4096;
        limits.max_active_per_isolate = 8;
    });
    let blocker = executor.render(job("blocker", 300)).await.unwrap();
    let big = "v".repeat(3000);
    let first = {
        let executor = executor.clone();
        let big = big.clone();
        tokio::spawn(async move {
            executor
                .render(request("admission.js", &[("x-id", "q1"), ("x-pad", &big)]))
                .await
                .map(|_| ())
        })
    };
    assert!(
        eventually(Duration::from_secs(2), async || executor.metrics().queued
            == 1)
        .await
    );
    let second = executor
        .render(request("admission.js", &[("x-id", "q2"), ("x-pad", &big)]))
        .await;
    assert_eq!(second.err(), Some(RenderError::Overloaded));
    drain(blocker).await;
    assert!(first.await.unwrap().is_ok());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_waiter_leaves_the_queue_and_frees_its_bytes() {
    let probe = Probe::at(880);
    let executor = start(&probe, |gate, limits| {
        gate.pressure_max_active = Some(1);
        limits.max_active_per_isolate = 8;
    });
    let blocker = executor.render(job("blocker", 300)).await.unwrap();
    let waiting =
        tokio::time::timeout(Duration::from_millis(100), executor.render(job("gone", 0))).await;
    assert!(waiting.is_err(), "still queued when abandoned");
    let snapshot = executor.metrics();
    assert_eq!(snapshot.queued, 0);
    assert_eq!(snapshot.admission.unwrap().queued_bytes, 0);
    drain(blocker).await;
    assert_eq!(runs(&executor, "gone").await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn failed_readings_reduce_concurrency_but_never_stop_traffic() {
    let probe = Probe::at(100);
    let executor = start(&probe, |gate, limits| {
        gate.stale_after = Duration::from_millis(100);
        gate.pressure_max_active = Some(1);
        limits.max_active_per_isolate = 8;
    });
    probe.fail();
    tokio::time::sleep(Duration::from_millis(150)).await;
    let tasks: Vec<_> = (0..4)
        .map(|i| {
            let executor = executor.clone();
            tokio::spawn(async move { collect(&executor, job(&format!("d{i}"), 50)).await.text() })
        })
        .collect();
    for task in tasks {
        assert_eq!(task.await.unwrap(), "ok");
    }
    let snapshot = executor.metrics().admission.unwrap();
    assert_eq!(snapshot.state, "degraded");
    assert!(snapshot.sample_failures > 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_isolate_near_its_heap_limit_takes_no_more_requests_while_busy() {
    let probe = Probe::at(100);
    let executor = start(&probe, |gate, limits| {
        // 24 MiB retained against a 128 MiB limit is past 15 %.
        gate.heap_percent = Some(15.0);
        limits.heap_limit_bytes = Some(128 * MIB as usize);
        limits.stall_timeout = None;
        limits.max_active_per_isolate = 8;
    });
    let holder = executor
        .render(request(
            "admission.js",
            &[
                ("x-id", "holder"),
                ("x-retain-mib", "24"),
                ("x-delay", "4000"),
                ("x-log", "1"),
            ],
        ))
        .await
        .unwrap();
    // The heap is sampled once a second.
    assert!(
        eventually(Duration::from_secs(3), async || {
            executor.metrics().executors[0].heap_used_bytes > (20 * MIB) as usize
        })
        .await
    );
    let next = {
        let executor = executor.clone();
        tokio::spawn(async move { collect(&executor, job("next", 0)).await.text() })
    };
    assert!(
        eventually(Duration::from_secs(1), async || executor.metrics().queued
            == 1)
        .await,
        "{:?}",
        executor.metrics().executors[0]
    );
    assert_eq!(drain(holder).await.text(), "ok");
    assert_eq!(next.await.unwrap(), "ok");
    let order = collect(
        &executor,
        request("admission.js", &[("x-report-order", "1")]),
    )
    .await
    .text();
    assert_eq!(order, r#"["holder","next"]"#);
    assert!(executor.metrics().admission.unwrap().heap_skips >= 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_with_deferred_waiters_does_not_hang() {
    let probe = Probe::at(880);
    let executor = start(&probe, |gate, limits| {
        gate.pressure_max_active = Some(1);
        limits.max_active_per_isolate = 8;
    });
    let blocker = executor.render(job("blocker", 200)).await.unwrap();
    let waiting = {
        let executor = executor.clone();
        tokio::spawn(async move { executor.render(job("w", 0)).await.err() })
    };
    assert!(
        eventually(Duration::from_secs(2), async || executor.metrics().queued
            == 1)
        .await
    );
    let stopper = {
        let executor = executor.clone();
        tokio::task::spawn_blocking(move || executor.shutdown())
    };
    assert_eq!(waiting.await.unwrap(), Some(RenderError::ShuttingDown));
    drain(blocker).await;
    tokio::time::timeout(Duration::from_secs(10), stopper)
        .await
        .expect("shutdown finishes")
        .unwrap();
}

// ---- Commitment lifecycle (RE-4.1B) -------------------------------------

/// Live commitments: count and bytes.
fn commitments(executor: &Executor) -> (u64, u64) {
    let admission = executor.metrics().admission.expect("admission on");
    (admission.commitments, admission.committed_bytes)
}

async fn settles(executor: &Executor) -> bool {
    eventually(Duration::from_secs(5), async || {
        commitments(executor) == (0, 0)
    })
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn commitments_end_with_their_requests_on_every_path() {
    let probe = Probe::at(100);
    let executor = start(&probe, |_, limits| {
        limits.request_timeout = Some(Duration::from_millis(300));
        limits.max_active_per_isolate = 8;
    });
    // Completed.
    let running = executor.render(job("done", 200)).await.unwrap();
    assert_eq!(commitments(&executor), (1, 2 * MIB), "held while it runs");
    drain(running).await;
    assert!(
        settles(&executor).await,
        "completed: {:?}",
        commitments(&executor)
    );
    // Client gone: the stream is dropped while JavaScript still runs; the
    // commitment lasts as long as the request does, then goes.
    drop(executor.render(job("cancelled", 200)).await.unwrap());
    assert!(
        settles(&executor).await,
        "cancelled: {:?}",
        commitments(&executor)
    );
    // Request timeout.
    let timed_out = collect(&executor, job("slow", 5000)).await;
    assert!(timed_out.error.is_some() || timed_out.status == 504);
    assert!(
        settles(&executor).await,
        "timed out: {:?}",
        commitments(&executor)
    );
    // Isolate replaced by the stall watchdog (500 ms in small_limits).
    let looping = collect(&executor, request("faults.js", &[("x-mode", "loop")])).await;
    assert!(looping.error.is_some() || looping.status != 200);
    assert!(
        settles(&executor).await,
        "replaced: {:?}",
        commitments(&executor)
    );
    assert!(!whoami(&executor).await.is_empty(), "serving again");
    assert!(settles(&executor).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_waiter_that_times_out_leaves_no_commitment() {
    let probe = Probe::at(880);
    let executor = start(&probe, |gate, limits| {
        gate.pressure_max_active = Some(1);
        limits.max_active_per_isolate = 8;
        limits.queue_timeout = Duration::from_millis(150);
    });
    let blocker = executor.render(job("blocker", 400)).await.unwrap();
    let waiter = executor.render(job("late", 0)).await;
    assert_eq!(waiter.err(), Some(RenderError::QueueTimeout));
    assert_eq!(commitments(&executor), (1, 2 * MIB), "only the blocker's");
    drain(blocker).await;
    assert!(settles(&executor).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn shutdown_releases_every_commitment() {
    let probe = Probe::at(100);
    let executor = start(&probe, |_, limits| limits.max_active_per_isolate = 8);
    let streams: Vec<_> =
        futures_join((0..4).map(|i| executor.render(job(&format!("s{i}"), 300)))).await;
    assert_eq!(commitments(&executor).0, 4);
    let stopper = {
        let executor = executor.clone();
        tokio::task::spawn_blocking(move || executor.shutdown())
    };
    for stream in streams {
        drain(stream.unwrap()).await;
    }
    tokio::time::timeout(Duration::from_secs(10), stopper)
        .await
        .expect("shutdown finishes")
        .unwrap();
    assert_eq!(commitments(&executor), (0, 0));
}

async fn futures_join<F: std::future::Future>(futures: impl Iterator<Item = F>) -> Vec<F::Output> {
    let mut out = Vec::new();
    for future in futures {
        out.push(future.await);
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn committed_accounting_holds_back_requests_whose_estimates_are_outstanding() {
    // 900 MiB of a 1000 MiB budget, 2 MiB estimates, critical at 950:
    // with sampled accounting a fresh reading forgets the estimates of the
    // requests still waiting (here: on a timer, as for a backend) and
    // admits more; with committed accounting the estimates stay.
    for (accounting, admitted) in [
        (nextrs_runtime::AdmissionAccounting::Sampled, 40),
        (nextrs_runtime::AdmissionAccounting::Committed, 25),
    ] {
        let probe = Probe::at(900);
        let executor = start(&probe, |gate, limits| {
            gate.accounting = accounting;
            gate.pressure_percent = 95.0;
            gate.critical_percent = 95.0;
            gate.hysteresis_percent = 1.0;
            gate.pressure_max_active = Some(1000);
            limits.max_active_per_isolate = 64;
            limits.isolates = 1;
            limits.queue_timeout = Duration::from_millis(100);
        });
        let mut streams = Vec::new();
        let mut refused = 0;
        for i in 0..40 {
            match executor.render(job(&format!("w{i}"), 1500)).await {
                Ok(stream) => streams.push(stream),
                Err(_) => refused += 1,
            }
        }
        assert_eq!(streams.len(), admitted, "{accounting:?}: {refused} refused");
        for stream in streams {
            drain(stream).await;
        }
        assert!(settles(&executor).await);
    }
}
