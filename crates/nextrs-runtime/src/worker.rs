//! One isolate thread: a current-thread Tokio runtime driving a `JsRuntime`
//! until the supervisor stops it or JavaScript fails.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use deno_core::{JsRuntime, PollEventLoopOptions, RuntimeOptions, v8};
use tokio::sync::mpsc;

use crate::metrics::Counters;
use crate::pool::{FailureReason, Job, Termination, WorkerExit, WorkerShared};
use crate::{ExecutorConfig, REPORTED_NODE_VERSION, ops};

/// How often the isolate's event loop proves it is not stuck in JavaScript.
const HEARTBEAT: Duration = Duration::from_millis(25);
/// How often the event loop records V8 heap statistics.
const HEAP_SAMPLE: Duration = Duration::from_secs(1);
/// Extra heap granted after the near-limit callback fires, so the
/// termination can unwind instead of V8 aborting the process.
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
    ("ext:nextrs/02_node.js", include_str!("../js/02_node.js")),
    (
        "ext:nextrs/03_server.js",
        include_str!("../js/03_server.js"),
    ),
];

pub(crate) fn spawn(
    config: ExecutorConfig,
    worker: Arc<WorkerShared>,
    counters: Arc<Counters>,
    jobs: mpsc::UnboundedReceiver<Box<Job>>,
    ready: Sender<Result<(), String>>,
    exit: Sender<WorkerExit>,
) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new()
        .name(format!("nextrs-v8-{}", worker.slot))
        .spawn(move || {
            let outcome = run(config, worker.clone(), counters, jobs, ready.clone());
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
    counters: Arc<Counters>,
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
        };
        let create_params = config
            .limits
            .heap_limit_bytes
            .map(|max| v8::CreateParams::default().heap_limits(0, max));
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![
                deno_webidl::deno_webidl::init(),
                deno_web::deno_web::init(
                    Arc::new(deno_web::BlobStore::default()),
                    None,
                    false,
                    deno_web::InMemoryBroadcastChannel::default(),
                ),
                ops::nextrs_runtime::init(
                    boot,
                    config.fs_policy(),
                    ops::Inbox(Rc::new(tokio::sync::Mutex::new(jobs))),
                    ops::ReadySignal(Some(ready)),
                    worker.clone(),
                    counters,
                ),
            ],
            create_params,
            ..Default::default()
        });
        let _ = worker
            .isolate
            .set(runtime.v8_isolate().thread_safe_handle());
        {
            let worker = worker.clone();
            runtime.add_near_heap_limit_callback(move |current, _initial| {
                worker.terminate(Termination::Failed(FailureReason::HeapLimit));
                current + HEAP_HEADROOM
            });
        }
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
        result
    })
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
    tokio::select! {
        biased;
        () = worker.stopped() => Ok(()),
        () = fault_panic => panic!("fault injection: executor thread panic"),
        result = event_loop(runtime, worker) => result.map_err(|e| format!("{e}")),
    }
}
