//! One isolate thread: a current-thread Tokio runtime driving a `JsRuntime`
//! until the supervisor stops it or JavaScript fails.

use std::ffi::c_void;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use deno_core::{JsRuntime, PollEventLoopOptions, RuntimeOptions, v8};
use tokio::sync::mpsc;

use crate::pool::{Job, ReclaimLevel, ReclaimRequest, Reclaimed, WorkerExit, WorkerShared};
use crate::{ExecutorConfig, Limits, REPORTED_NODE_VERSION, ops};

/// How often the isolate's event loop proves it is not stuck in JavaScript.
const HEARTBEAT: Duration = Duration::from_millis(25);
/// How often the event loop records V8 heap statistics.
const HEAP_SAMPLE: Duration = Duration::from_secs(1);

/// Extra heap granted when the near-limit callback fires, so the
/// termination can unwind instead of V8 aborting the process. Not smaller
/// for small heaps: on its last-resort path V8 calls the callback once and
/// retries the failed allocation once, so a single object larger than the
/// grant would abort the whole process instead of replacing one isolate
/// (v8/src/heap/heap-allocator.cc `RetryCustomAllocate`). The grant is a
/// permission, not memory: V8 uses only what it allocates before the
/// termination takes effect.
const HEAP_HEADROOM: usize = 64 * 1024 * 1024;

const SCRIPTS: &[(&str, &str)] = &[
    (
        "ext:nextrs/00_globals.js",
        include_str!("../js/00_globals.js"),
    ),
    ("ext:nextrs/01_fetch.js", include_str!("../js/01_fetch.js")),
    (
        "ext:nextrs/node-polyfills.js",
        include_str!("../js/node-polyfills.js"),
    ),
    (
        "ext:nextrs/02_webcrypto.js",
        include_str!("../js/02_webcrypto.js"),
    ),
    ("ext:nextrs/02_node.js", include_str!("../js/02_node.js")),
    (
        "ext:nextrs/03_server.js",
        include_str!("../js/03_server.js"),
    ),
];

pub(crate) fn spawn(
    config: ExecutorConfig,
    worker: Arc<WorkerShared>,
    services: ops::HostServices,
    external: Option<Arc<crate::external::ExternalBudget>>,
    jobs: mpsc::UnboundedReceiver<Box<Job>>,
    ready: Sender<Result<(), String>>,
    exit: Sender<WorkerExit>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(format!("nextrs-v8-{}", worker.slot))
        .spawn(move || {
            let outcome = run(
                config,
                worker.clone(),
                services,
                external,
                jobs,
                ready.clone(),
            );
            worker.mark_exited();
            if let Err(error) = &outcome {
                // Only reaches the supervisor if bootstrap had not reported.
                let _ = ready.send(Err(error.clone()));
            }
            let _ = exit.send(outcome);
        })
}

fn run(
    config: ExecutorConfig,
    worker: Arc<WorkerShared>,
    services: ops::HostServices,
    external: Option<Arc<crate::external::ExternalBudget>>,
    jobs: mpsc::UnboundedReceiver<Box<Job>>,
    ready: Sender<Result<(), String>>,
) -> WorkerExit {
    let tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    tokio.block_on(async move {
        let boot = ops::BootConfig {
            project_dir: config.project_dir.to_string_lossy().into_owned(),
            dist_dir: config.dist_dir.to_string_lossy().into_owned(),
            env: config.env.clone(),
            bootstrap: config
                .bootstrap
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            trace_node_api: config.trace_node_api,
            node_version: REPORTED_NODE_VERSION,
            nextrs_version: env!("CARGO_PKG_VERSION"),
            fault_injection: cfg!(feature = "fault-injection"),
            network: services.net.is_some(),
            cache: services.cache.is_some(),
            cache_reuse_bytes: services
                .cache
                .as_ref()
                .map_or(0, |cache| cache.reuse_budget()),
            build_route_cache: config
                .build_route_cache
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        };
        let mut create_params = heap_params(&config.limits);
        if let Some(budget) = &external {
            create_params = Some(
                create_params
                    .unwrap_or_default()
                    .array_buffer_allocator(budget.allocator()),
            );
        }
        let mut web = deno_web::deno_web::init(
            Arc::new(deno_web::BlobStore::default()),
            None,
            false,
            deno_web::InMemoryBroadcastChannel::default(),
        );
        crate::embedded_js::embed(&mut web, crate::embedded_js::DENO_WEB);
        crate::web_patches::apply(&mut web);
        let mut webidl = deno_webidl::deno_webidl::init();
        crate::embedded_js::embed(&mut webidl, crate::embedded_js::DENO_WEBIDL);
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![
                webidl,
                web,
                ops::nextrs_runtime::init(
                    boot,
                    config.fs_policy(),
                    ops::Inbox(Rc::new(tokio::sync::Mutex::new(jobs))),
                    ops::ReadySignal(Some(ready)),
                    worker.clone(),
                    services,
                ),
            ],
            create_params,
            ..Default::default()
        });
        // Test hook: a slow isolate creation (a cold machine), so the
        // supervisor's deadline can expire before the handle exists.
        #[cfg(feature = "fault-injection")]
        if let Some(ms) = config
            .env
            .get("NEXTRS_FAULT_ISOLATE_CREATE_MS")
            .and_then(|v| v.parse::<u64>().ok())
        {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        let _ = worker
            .isolate
            .set(runtime.v8_isolate().thread_safe_handle());
        // A termination requested while the isolate was being created had
        // no handle to interrupt: apply it now, before any script runs.
        worker.interrupt();
        {
            let worker = worker.clone();
            runtime.add_near_heap_limit_callback(move |current, initial| {
                worker.on_near_heap_limit(current, initial, HEAP_HEADROOM)
            });
        }
        if config.limits.heap_limit_bytes.is_some() || config.limits.heap_memory_bytes.is_some() {
            // What V8 applied, which can be below the budget (V8's own
            // default for the memory) or overridden by --v8-flags.
            let stats = runtime.v8_isolate().get_heap_statistics();
            tracing::info!(
                target: "nextrs::executor",
                slot = worker.slot,
                heap_limit_mib = stats.heap_size_limit() / (1024 * 1024),
                "V8 heap limit in effect"
            );
        }
        // The callback's data pointer owns one reference to `worker`. It is
        // released below only after the callback is removed; if this thread
        // unwinds first, the reference leaks instead of dangling.
        let gc_data = Arc::into_raw(worker.clone()) as *mut c_void;
        runtime.v8_isolate().add_gc_epilogue_callback(
            after_full_gc,
            gc_data,
            v8::GCType::kGCTypeMarkSweepCompact,
        );
        worker.register_cpu_clock();
        worker.beat();
        let heartbeat = {
            let worker = worker.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(HEARTBEAT);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tick.tick().await;
                    worker.beat();
                }
            })
        };
        let result = drive(&mut runtime, &worker).await;
        heartbeat.abort();
        runtime
            .v8_isolate()
            .remove_gc_epilogue_callback(after_full_gc, gc_data);
        // SAFETY: from `Arc::into_raw` above; V8 no longer holds the pointer.
        drop(unsafe { Arc::from_raw(gc_data as *const WorkerShared) });
        result
    })
}

/// The isolate's heap constraints: V8's defaults for a machine with
/// `heap_memory_bytes` of memory (young generation, growth factor, initial
/// limit), with the heap limit capped at `heap_limit_bytes`; or only an
/// explicit limit (V8 derives the young generation from it); or V8's own
/// defaults, from the machine's memory.
fn heap_params(limits: &Limits) -> Option<v8::CreateParams> {
    let params = configured_heap(limits)?;
    Some(match limits.young_semi_space_floor_bytes {
        Some(semi) => young_floor(params, semi),
        None => params,
    })
}

/// Raises the young generation to `3 × semi` (semi rounded down to a power
/// of two), taking the difference from the old generation so that their
/// total is unchanged, and never giving the young generation more than
/// half of it (RE-4G). A larger young generation is kept as it is.
fn young_floor(params: v8::CreateParams, semi: usize) -> v8::CreateParams {
    let young = params.max_young_generation_size_in_bytes();
    let old = params.max_old_generation_size_in_bytes();
    let total = young.saturating_add(old);
    let mut semi = 1usize << semi.max(1).ilog2();
    while semi > 1 && semi.saturating_mul(3) > total / 2 {
        semi /= 2;
    }
    let floor = semi.saturating_mul(3);
    if floor <= young {
        return params;
    }
    params
        .set_max_young_generation_size_in_bytes(floor)
        .set_max_old_generation_size_in_bytes(total - floor)
}

fn configured_heap(limits: &Limits) -> Option<v8::CreateParams> {
    match (limits.heap_limit_bytes, limits.heap_memory_bytes) {
        (Some(max), None) => Some(v8::CreateParams::default().heap_limits(0, max)),
        (cap, Some(memory)) => {
            let params = v8::CreateParams::default().heap_limits_from_system_memory(memory, 0);
            let young = params.max_young_generation_size_in_bytes();
            let old = params.max_old_generation_size_in_bytes();
            match cap {
                // The young generation stays as V8 sized it for the memory,
                // up to a quarter of the cap; the old generation gets the
                // rest. V8 raises sizes below its minimums to the minimums.
                Some(max) if max < young.saturating_add(old) => {
                    // V8 rounds a semi-space (a third of the young
                    // generation) up to a power of two; round down first so
                    // the cap holds.
                    let semi = (young.min(max / 4) / 3).max(1);
                    let young = (1usize << semi.ilog2()).saturating_mul(3);
                    Some(
                        params
                            .set_max_young_generation_size_in_bytes(young)
                            .set_max_old_generation_size_in_bytes(max.saturating_sub(young)),
                    )
                }
                _ => Some(params),
            }
        }
        (None, None) => None,
    }
}

/// Performs a memory reclamation on the isolate's own thread, between
/// turns of its event loop, and reports to everyone waiting for it.
fn reclaim(runtime: &mut JsRuntime, worker: &WorkerShared, request: ReclaimRequest) {
    // The collection runs between turns: start the stall clock here, not
    // at the previous turn, and again after it.
    worker.beat();
    let isolate = runtime.v8_isolate();
    let before = isolate.get_heap_statistics().total_heap_size();
    let started = Instant::now();
    match request.level {
        ReclaimLevel::Idle => isolate.low_memory_notification(),
        ReclaimLevel::Pressure => {
            isolate.memory_pressure_notification(v8::MemoryPressureLevel::Critical);
        }
    }
    let gc_time = started.elapsed();
    worker.beat();
    let stats = isolate.get_heap_statistics();
    worker.record_heap(&stats);
    let done = Reclaimed {
        heap_released_bytes: u64::try_from(before.saturating_sub(stats.total_heap_size()))
            .unwrap_or(0),
        gc_time,
    };
    tracing::debug!(
        target: "nextrs::executor",
        slot = worker.slot,
        level = request.level.as_str(),
        gc_ms = gc_time.as_secs_f64() * 1e3,
        heap_before = before,
        heap_after = stats.total_heap_size(),
        "memory reclaimed"
    );
    for waiter in request.done {
        // The requester may have stopped waiting (timeout); nothing to do.
        let _ = waiter.send(done);
    }
}

/// V8 GC epilogue for full collections: records the heap still in use.
unsafe extern "C" fn after_full_gc(
    isolate: v8::UnsafeRawIsolatePtr,
    _type: v8::GCType,
    _flags: v8::GCCallbackFlags,
    data: *mut c_void,
) {
    // SAFETY: `data` is the `Arc<WorkerShared>` reference registered in
    // `run`, alive until the callback is removed. V8 calls this on the
    // isolate's own thread with a valid isolate pointer; reading heap
    // statistics from a GC callback is permitted.
    let (worker, mut isolate) = unsafe {
        (
            &*(data as *const WorkerShared),
            v8::Isolate::from_raw_isolate_ptr(isolate),
        )
    };
    worker.record_full_gc(isolate.get_heap_statistics().used_heap_size());
}

/// `run_event_loop`, sampling V8 heap statistics at most once per
/// `HEAP_SAMPLE` between turns of the loop (the isolate is only reachable
/// from its own thread).
async fn event_loop(
    runtime: &mut JsRuntime,
    worker: &WorkerShared,
) -> Result<(), deno_core::error::CoreError> {
    let mut sampled: Option<Instant> = None;
    std::future::poll_fn(|cx| {
        if sampled.is_none_or(|at| at.elapsed() >= HEAP_SAMPLE) {
            worker.record_heap(&runtime.v8_isolate().get_heap_statistics());
            sampled = Some(Instant::now());
        }
        runtime.poll_event_loop(cx, PollEventLoopOptions::default())
    })
    .await
}

async fn drive(runtime: &mut JsRuntime, worker: &WorkerShared) -> WorkerExit {
    for (name, source) in SCRIPTS {
        runtime
            .execute_script(*name, *source)
            .map_err(|e| format!("{e}"))?;
    }
    runtime
        .execute_script("ext:nextrs/start", "globalThis.__nextrsStart()")
        .map_err(|e| format!("{e}"))?;
    #[cfg(feature = "fault-injection")]
    let fault_panic = worker.fault_panic.notified();
    #[cfg(not(feature = "fault-injection"))]
    let fault_panic = std::future::pending::<()>();
    let mut fault_panic = std::pin::pin!(fault_panic);
    loop {
        // The event loop future only borrows the runtime between polls, so
        // leaving it for a reclamation and polling it again is safe.
        tokio::select! {
            biased;
            () = worker.stopped() => return Ok(()),
            () = &mut fault_panic => panic!("fault injection: executor thread panic"),
            request = worker.reclaim_requested() => reclaim(runtime, worker, request),
            result = event_loop(runtime, worker) => return result.map_err(|e| format!("{e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 1024 * 1024;

    fn sizes(limits: &Limits) -> Option<(usize, usize)> {
        heap_params(limits).map(|p| {
            (
                p.max_young_generation_size_in_bytes(),
                p.max_old_generation_size_in_bytes(),
            )
        })
    }

    fn limits(limit: Option<usize>, memory: Option<u64>, floor: Option<usize>) -> Limits {
        Limits {
            heap_limit_bytes: limit,
            heap_memory_bytes: memory,
            young_semi_space_floor_bytes: floor,
            ..Limits::default()
        }
    }

    #[test]
    fn the_floor_takes_its_room_from_the_old_generation() {
        // `--js-heap-limit auto` at 256 MiB with one isolate: a 112 MiB cap,
        // V8 sized for 256 MiB (2 MiB semi-spaces).
        let base = sizes(&limits(Some(112 * MIB), Some(256 << 20), None)).unwrap();
        assert_eq!(base.0, 6 * MIB);
        let floored = sizes(&limits(Some(112 * MIB), Some(256 << 20), Some(8 * MIB))).unwrap();
        assert_eq!(
            floored.0,
            24 * MIB,
            "two semi-spaces and the large-object space"
        );
        assert_eq!(
            floored.0 + floored.1,
            base.0 + base.1,
            "the total is unchanged"
        );
    }

    #[test]
    fn a_larger_young_generation_from_v8_is_kept() {
        let base = sizes(&limits(None, Some(1 << 30), None)).unwrap();
        assert_eq!(
            sizes(&limits(None, Some(1 << 30), Some(2 * MIB))).unwrap(),
            base
        );
    }

    #[test]
    fn the_young_generation_never_takes_more_than_half() {
        let (young, old) = sizes(&limits(Some(32 * MIB), Some(256 << 20), Some(16 * MIB))).unwrap();
        assert!(young <= (young + old) / 2, "{young} of {}", young + old);
        assert_eq!(young, 12 * MIB, "16 MiB halved to 4 MiB semi-spaces");
        assert_eq!(young + old, 32 * MIB);
    }

    #[test]
    fn the_floor_is_rounded_down_to_a_power_of_two() {
        let (young, _) = sizes(&limits(Some(256 * MIB), Some(1 << 30), Some(12 * MIB))).unwrap();
        assert_eq!(young, 24 * MIB, "12 MiB rounds down to 8 MiB semi-spaces");
    }

    #[test]
    fn with_an_explicit_limit_only_the_total_is_kept() {
        let base = sizes(&limits(Some(128 * MIB), None, None)).unwrap();
        let floored = sizes(&limits(Some(128 * MIB), None, Some(8 * MIB))).unwrap();
        assert_eq!(floored.0, base.0.max(24 * MIB));
        assert_eq!(floored.0 + floored.1, base.0 + base.1);
    }

    #[test]
    fn without_a_configured_heap_nothing_is_set() {
        assert!(sizes(&limits(None, None, Some(8 * MIB))).is_none());
    }
}
