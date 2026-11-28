//! Nested requests (Next.js fetching its own server for an admitted
//! request) when their parents hold every isolate slot: they take an
//! overdraft slot from a separate, bounded allowance instead of queueing
//! behind other parents until `queue_timeout` (RE-4.1D §3, fixed in RE-5).
//! External requests never get one; recursion stays bounded by the
//! internal-request permits; memory admission still records nested
//! requests and never gates them; nothing leaks on cancellation or
//! shutdown.

mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use common::*;
use futures_util::StreamExt;
use nextrs_runtime::{
    Executor, InternalDispatch, InternalRequest, InternalResponse, Limits, MemoryAdmission,
    MemoryProbe, MemoryReading, RenderRequest, ResponseEvent,
};

const MIB: u64 = 1024 * 1024;

/// Serves internal requests with the same executor, as the HTTP layer
/// does for a dynamic page.
#[derive(Default)]
struct Dispatch(OnceLock<Weak<Executor>>);

impl InternalDispatch for Dispatch {
    fn dispatch(
        &self,
        internal: InternalRequest,
    ) -> Pin<Box<dyn Future<Output = InternalResponse> + Send + 'static>> {
        let executor = self.0.get().and_then(Weak::upgrade);
        Box::pin(async move {
            let failed = |status: u16| InternalResponse {
                status,
                headers: Vec::new(),
                body: futures_util::stream::empty().boxed(),
            };
            let Some(executor) = executor else {
                return failed(503);
            };
            let mut child = request("nested.js", &[]);
            child.url = internal.path_and_query;
            child.headers = internal.headers;
            let Ok(mut events) = executor.render(child).await else {
                return failed(503);
            };
            let (status, headers) = match events.recv().await {
                Some(ResponseEvent::Head { status, headers }) => (status, headers),
                _ => return failed(502),
            };
            let body = futures_util::stream::unfold(events, |mut events| async move {
                match events.recv().await? {
                    ResponseEvent::Chunk(chunk) => Some((Ok(chunk), events)),
                    ResponseEvent::Error(error) => Some((Err(error), events)),
                    ResponseEvent::End | ResponseEvent::Head { .. } => None,
                }
            })
            .boxed();
            InternalResponse {
                status,
                headers,
                body,
            }
        })
    }
}

fn start(limits: Limits) -> Arc<Executor> {
    let executor = executor(Limits {
        request_timeout: None,
        ..limits
    });
    let dispatch = Arc::new(Dispatch::default());
    let _ = dispatch.0.set(Arc::downgrade(&executor));
    executor.set_internal_dispatch(dispatch);
    executor
}

fn limits(isolates: usize, slots: usize) -> Limits {
    Limits {
        isolates,
        max_active_per_isolate: slots,
        queue_timeout: Duration::from_secs(3),
        ..small_limits()
    }
}

fn get(url: &str, headers: &[(&str, &str)]) -> RenderRequest {
    let mut request = request("nested.js", headers);
    request.url = url.to_owned();
    request
}

async fn report(executor: &Executor) -> (u64, u64) {
    let text = collect(executor, get("/report", &[])).await.text();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    (
        value["maxParents"].as_u64().unwrap(),
        value["maxChildren"].as_u64().unwrap(),
    )
}

fn active(executor: &Executor) -> usize {
    executor.metrics().executors.iter().map(|e| e.active).sum()
}

async fn idle(executor: &Executor) -> bool {
    eventually(Duration::from_secs(5), async || executor.is_idle()).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_nested_request_does_not_wait_for_its_parents_slot() {
    // One isolate, one slot, held by the parent: before the fix the child
    // waited out the 3 s queue timeout and failed.
    let executor = start(limits(1, 1));
    let started = Instant::now();
    let reply = collect(&executor, get("/parent?children=1", &[])).await;
    let elapsed = started.elapsed();
    assert_eq!(reply.status, 200);
    assert_eq!(reply.text(), "200:child");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    let metrics = executor.metrics();
    assert_eq!(metrics.nested_overdraft, 1);
    assert_eq!(metrics.rejected_queue_timeout, 0);
    assert!(idle(&executor).await);
    assert_eq!(active(&executor), 0);
    executor.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn external_requests_never_use_the_allowance() {
    // Two slots, eight parents at once, each with two slow children: the
    // parents run at most two at a time (the rest queue as before), every
    // child gets a slot, and every parent completes with its payload.
    let executor = start(Limits {
        queue_timeout: Duration::from_secs(20),
        ..limits(1, 2)
    });
    let started = Instant::now();
    let parents: Vec<_> = (0..8)
        .map(|_| {
            let executor = executor.clone();
            tokio::spawn(async move {
                collect(&executor, get("/parent?children=2", &[("x-delay", "100")])).await
            })
        })
        .collect();
    for parent in parents {
        let reply = parent.await.unwrap();
        assert_eq!(reply.status, 200);
        assert_eq!(reply.text(), "200:child,200:child");
    }
    let elapsed = started.elapsed();
    let (max_parents, max_children) = report(&executor).await;
    println!(
        "8 parents × 2 children, 2 slots: {elapsed:?}, max parents {max_parents}, max children {max_children}"
    );
    assert!(
        max_parents <= 2,
        "parents above the slot limit: {max_parents}"
    );
    assert!(max_children >= 2);
    let metrics = executor.metrics();
    assert_eq!(metrics.rejected_queue_timeout, 0);
    assert!(metrics.nested_overdraft >= 2);
    assert!(idle(&executor).await);
    assert_eq!(active(&executor), 0);
    executor.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn overdraft_is_bounded_per_isolate_and_recursion_ends() {
    // One slot: the parent holds it and its child takes the isolate's one
    // overdraft slot (at most `max_active_per_isolate` more). A grandchild
    // queues for a slot, as before RE-5, and fails at the queue timeout:
    // a self-fetching route 100 deep ends instead of growing.
    let executor = start(limits(1, 1));
    let reply = collect(&executor, get("/recurse?depth=1", &[])).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(reply.text(), "1>bottom");
    let started = Instant::now();
    let reply = collect(&executor, get("/recurse?depth=100", &[])).await;
    let elapsed = started.elapsed();
    assert_ne!(reply.status, 200, "{}", reply.text());
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
    assert!(executor.metrics().rejected_queue_timeout >= 1);
    // With room for two levels of overdraft (two slots), depth 2 is fine.
    let executor2 = start(limits(1, 2));
    let reply = collect(&executor2, get("/recurse?depth=3", &[])).await;
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert!(idle(&executor).await && idle(&executor2).await);
    assert_eq!(active(&executor), 0);
    executor.shutdown();
    executor2.shutdown();
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelled_parents_and_shutdown_leave_no_slot_behind() {
    let executor = start(limits(1, 1));
    // Clients gone while children run in overdraft slots.
    for _ in 0..4 {
        let stream = executor
            .render(get("/parent?children=3", &[("x-delay", "200")]))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(stream);
    }
    assert!(idle(&executor).await, "cancelled parents finished");
    assert_eq!(active(&executor), 0);
    // The ordinary limit still holds afterwards: one slot, so a second
    // external request waits for the first (no leaked overdraft count lets
    // it in, no leaked active count blocks it).
    let first = executor
        .render(get("/parent?children=1", &[("x-delay", "300")]))
        .await
        .unwrap();
    let second = executor.render(get("/parent?children=1", &[]));
    let (first, second) = tokio::join!(drain(first), async { drain(second.await.unwrap()).await });
    assert_eq!(first.text(), "200:child");
    assert_eq!(second.text(), "200:child");
    // Shutdown with a parent in flight, its child in the isolate's one
    // overdraft slot: both finish. (A second concurrent child would queue
    // and, shutdown having begun, be refused: the allowance is one slot per
    // ordinary slot.)
    let running = executor
        .render(get("/parent?children=1", &[("x-delay", "200")]))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let shutdown = {
        let executor = executor.clone();
        tokio::task::spawn_blocking(move || executor.shutdown())
    };
    let reply = drain(running).await;
    shutdown.await.unwrap();
    assert!(reply.ended, "{reply:?}");
    assert_eq!(reply.text(), "200:child");
}

/// A memory probe fixed at `usage` of a 1000 MiB limit.
struct Probe(Mutex<u64>);

impl MemoryProbe for Probe {
    fn read(&self, _full: bool) -> Option<MemoryReading> {
        Some(MemoryReading {
            limit: Some(1000 * MIB),
            usage: *self.0.lock().unwrap() * MIB,
            inactive_file: Some(0),
        })
    }

    fn describe(&self) -> String {
        "test probe".into()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn at_critical_memory_children_are_recorded_and_never_gated() {
    // 99 % used: a parent is admitted only by the progress rule, its
    // children take overdraft slots without being refused, their estimates
    // are commitments while they run, and every commitment ends.
    let probe = Arc::new(Probe(Mutex::new(990)));
    let executor = start(Limits {
        memory_admission: Some(MemoryAdmission {
            sample_interval: Duration::ZERO,
            stat_interval: Duration::ZERO,
            cooldown: Duration::ZERO,
            probe: Some(probe.clone()),
            ..MemoryAdmission::default()
        }),
        ..limits(1, 1)
    });
    let reply = collect(&executor, get("/parent?children=3", &[("x-delay", "100")])).await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.text(), "200:child,200:child,200:child");
    let metrics = executor.metrics();
    assert_eq!(metrics.rejected_memory_pressure, 0);
    assert_eq!(metrics.nested_overdraft, 3);
    let settled = eventually(Duration::from_secs(5), async || {
        executor
            .metrics()
            .admission
            .is_some_and(|a| a.commitments == 0)
    })
    .await;
    assert!(
        settled,
        "{:?}",
        executor.metrics().admission.map(|a| a.commitments)
    );
    executor.shutdown();
}
