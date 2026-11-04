//! The isolate pool: admission, dispatch and supervision.
//!
//! * **Admission.** A request needs a slot on a ready isolate (at most
//!   `max_active_per_isolate` each). Without one it waits in a FIFO queue of
//!   at most `max_queued` entries for at most `queue_timeout`. A released
//!   slot is handed directly to the oldest waiter, so nothing can overtake
//!   the queue.
//! * **Dispatch.** Least outstanding requests among ready isolates, ties
//!   to the lowest slot (light load stays on one warm isolate); isolates
//!   busy on the CPU (event loop not turned for 200 ms) only when no other
//!   has a free slot.
//! * **Supervision.** One supervisor thread per slot starts the isolate,
//!   watches its heartbeat (CPU stall watchdog), and on any exit decides
//!   whether it failed. A failed isolate is removed from dispatch at once,
//!   its in-flight requests fail with it, and a fresh isolate replaces it
//!   after an exponential backoff. Requests are never replayed.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use deno_core::v8;
use tokio::sync::{Notify, mpsc, oneshot};

use crate::metrics::{self, Counters, ExecutorSnapshot, MetricsSnapshot};
use crate::net::NetClient;
use crate::response::{self, ResponseSender};
use crate::{ExecutorConfig, ExecutorError, RenderError, RenderRequest, ResponseStream, worker};

/// How often supervisors check heartbeats and shutdown progress.
const WATCH_INTERVAL: Duration = Duration::from_millis(50);
/// An isolate whose event loop has not turned for this long is busy on the
/// CPU; dispatch prefers others while any is responsive.
const BUSY_HINT: Duration = Duration::from_millis(200);
/// Failures within this window count towards the restart backoff.
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

/// Why an isolate was taken out of service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// The bootstrap threw or the runtime could not be created.
    Bootstrap,
    /// An uncaught exception or unhandled rejection with no listener ended
    /// the event loop (Node would exit the process).
    Uncaught,
    /// JavaScript ran longer than `stall_timeout` without yielding; the
    /// isolate was terminated.
    CpuStall,
    /// The V8 heap reached its limit; the isolate was terminated.
    HeapLimit,
    /// The executor thread panicked.
    Panic,
    /// The event loop ended without being asked to.
    UnexpectedExit,
}

impl FailureReason {
    const ALL: [Self; 6] = [
        Self::Bootstrap,
        Self::Uncaught,
        Self::CpuStall,
        Self::HeapLimit,
        Self::Panic,
        Self::UnexpectedExit,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bootstrap => "bootstrap",
            Self::Uncaught => "uncaught",
            Self::CpuStall => "cpu_stall",
            Self::HeapLimit => "heap_limit",
            Self::Panic => "panic",
            Self::UnexpectedExit => "unexpected_exit",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|r| *r == self).unwrap_or(0)
    }
}

/// Why the supervisor stopped an isolate on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Termination {
    Failed(FailureReason),
    Shutdown,
}

const STARTING: u8 = 0;
const READY: u8 = 1;
const FAILED: u8 = 2;
const DRAINING: u8 = 3;
const STOPPED: u8 = 4;

fn state_name(state: u8) -> &'static str {
    match state {
        STARTING => "starting",
        READY => "ready",
        FAILED => "failed",
        DRAINING => "draining",
        _ => "stopped",
    }
}

/// One running isolate, shared between its thread, its supervisor, the
/// dispatcher and the ops.
pub(crate) struct WorkerShared {
    pub slot: usize,
    pub generation: u64,
    state: AtomicU8,
    /// Slots in use. Changed only under the pool lock.
    active: AtomicUsize,
    inbox: Mutex<Option<mpsc::UnboundedSender<Box<Job>>>>,
    epoch: Instant,
    heartbeat_ms: AtomicU64,
    /// V8 heap statistics, sampled by the isolate thread (`record_heap`).
    heap_used: AtomicUsize,
    heap_total: AtomicUsize,
    heap_external: AtomicUsize,
    /// Heap in use right after the latest full (mark-sweep) collection:
    /// the live-data floor. 0 until the first full GC (`record_full_gc`).
    heap_after_gc: AtomicUsize,
    full_gcs: AtomicU64,
    pub isolate: OnceLock<v8::IsolateHandle>,
    termination: Mutex<Option<Termination>>,
    stop: Notify,
    stopping: AtomicBool,
    #[cfg(feature = "fault-injection")]
    pub fault_panic: Notify,
}

impl WorkerShared {
    fn new(slot: usize, generation: u64, inbox: mpsc::UnboundedSender<Box<Job>>) -> Self {
        Self {
            slot,
            generation,
            state: AtomicU8::new(STARTING),
            active: AtomicUsize::new(0),
            inbox: Mutex::new(Some(inbox)),
            epoch: Instant::now(),
            heartbeat_ms: AtomicU64::new(0),
            heap_used: AtomicUsize::new(0),
            heap_total: AtomicUsize::new(0),
            heap_external: AtomicUsize::new(0),
            heap_after_gc: AtomicUsize::new(0),
            full_gcs: AtomicU64::new(0),
            isolate: OnceLock::new(),
            termination: Mutex::new(None),
            stop: Notify::new(),
            stopping: AtomicBool::new(false),
            #[cfg(feature = "fault-injection")]
            fault_panic: Notify::new(),
        }
    }

    fn state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    fn set_state(&self, state: u8) {
        self.state.store(state, Ordering::Release);
    }

    pub fn beat(&self) {
        let now = self.epoch.elapsed().as_millis() as u64;
        self.heartbeat_ms.store(now, Ordering::Relaxed);
    }

    pub fn record_heap(&self, stats: &v8::HeapStatistics) {
        self.heap_used
            .store(stats.used_heap_size(), Ordering::Relaxed);
        self.heap_total
            .store(stats.total_heap_size(), Ordering::Relaxed);
        self.heap_external
            .store(stats.external_memory(), Ordering::Relaxed);
    }

    /// Called from V8's GC epilogue after every full collection.
    pub fn record_full_gc(&self, used_heap: usize) {
        self.heap_after_gc.store(used_heap, Ordering::Relaxed);
        self.full_gcs.fetch_add(1, Ordering::Relaxed);
    }

    /// Time since the isolate's event loop last got control back.
    fn busy_for(&self) -> Duration {
        let now = self.epoch.elapsed().as_millis() as u64;
        Duration::from_millis(now.saturating_sub(self.heartbeat_ms.load(Ordering::Relaxed)))
    }

    fn send(&self, job: Box<Job>) -> Result<(), Box<Job>> {
        let inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
        match inbox.as_ref() {
            Some(sender) => sender.send(job).map_err(|e| e.0),
            None => Err(job),
        }
    }

    /// Called by the isolate thread as its event loop ends, so dispatch
    /// stops choosing it before the supervisor notices.
    pub fn mark_exited(&self) {
        let _ = self
            .state
            .compare_exchange(READY, FAILED, Ordering::AcqRel, Ordering::Acquire);
        self.close_inbox();
    }

    /// Ends the JavaScript request loop once the queued jobs are taken.
    fn close_inbox(&self) {
        self.inbox.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    /// Stops the isolate: takes it out of dispatch, interrupts running
    /// JavaScript, and ends its event loop. The first reason given wins.
    pub fn terminate(&self, why: Termination) {
        {
            let mut termination = self.termination.lock().unwrap_or_else(|e| e.into_inner());
            termination.get_or_insert(why);
        }
        if matches!(why, Termination::Failed(_)) {
            self.set_state(FAILED);
        }
        self.stopping.store(true, Ordering::Release);
        self.stop.notify_one();
        if let Some(isolate) = self.isolate.get() {
            isolate.terminate_execution();
        }
    }

    fn termination(&self) -> Option<Termination> {
        *self.termination.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Resolves once `terminate` was called.
    pub async fn stopped(&self) {
        loop {
            if self.stopping.load(Ordering::Acquire) {
                return;
            }
            self.stop.notified().await;
        }
    }
}

/// A request on its way to (or inside) an isolate.
pub(crate) struct Job {
    pub request: RenderRequest,
    pub response: ResponseSender,
    pub slot: Slot,
    pub timeout_ms: Option<u64>,
    pub queued_at: Instant,
}

/// A claim on one of an isolate's concurrency slots, released on drop:
/// when JavaScript reports the request done, or when the isolate (and with
/// it the op state that holds the slot) is dropped.
pub(crate) struct Slot {
    pool: Option<Arc<PoolInner>>,
    worker: Arc<WorkerShared>,
}

impl Slot {
    /// Gives the claim back without going through `release` (which takes
    /// the pool lock), for use while that lock is held.
    fn disarm(mut self) -> Arc<WorkerShared> {
        self.pool = None;
        self.worker.clone()
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.release(&self.worker);
        }
    }
}

struct Waiter {
    id: u64,
    slot: oneshot::Sender<Slot>,
}

#[derive(Default)]
struct SlotHistory {
    generation: u64,
    restarts: u64,
    failures: [u64; FailureReason::ALL.len()],
    last_failure: Option<String>,
}

#[derive(Default)]
struct PoolState {
    /// The isolate currently serving each slot (None while it restarts).
    workers: Vec<Option<Arc<WorkerShared>>>,
    history: Vec<SlotHistory>,
    waiters: VecDeque<Waiter>,
    next_waiter: u64,
}

pub(crate) struct PoolInner {
    me: Weak<PoolInner>,
    pub config: ExecutorConfig,
    pub counters: Arc<Counters>,
    /// Shared by every isolate (one connection pool); `None` without a
    /// network configuration.
    pub net: Option<Arc<NetClient>>,
    state: Mutex<PoolState>,
    shutting_down: AtomicBool,
    /// Wakes supervisors sleeping in a restart backoff.
    shutdown: (Mutex<bool>, Condvar),
}

impl PoolInner {
    fn lock(&self) -> MutexGuard<'_, PoolState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }

    fn slot(&self, worker: &Arc<WorkerShared>) -> Slot {
        Slot {
            pool: self.me.upgrade(),
            worker: worker.clone(),
        }
    }

    /// Least outstanding requests among ready isolates with a free slot,
    /// preferring isolates whose event loop is turning (a request sent to
    /// one stuck on the CPU would wait behind it, and fail with it if the
    /// watchdog fires). Ties go to the lowest slot: at low concurrency,
    /// spreading sequential requests round-robin left every isolate's JIT
    /// and caches lukewarm (4 isolates at concurrency 1: 313 against 441
    /// req/s on /books/42). Claims the slot. Requires the pool lock.
    fn claim(&self, state: &mut PoolState) -> Option<Arc<WorkerShared>> {
        let limit = self.config.limits.max_active_per_isolate;
        let n = state.workers.len();
        let mut best: Option<((bool, usize), &Arc<WorkerShared>)> = None;
        for index in 0..n {
            let Some(worker) = &state.workers[index] else {
                continue;
            };
            let active = worker.active.load(Ordering::Relaxed);
            if worker.state() != READY || active >= limit {
                continue;
            }
            let key = (worker.busy_for() > BUSY_HINT, active);
            if best.is_none_or(|(k, _)| key < k) {
                best = Some((key, worker));
            }
        }
        let worker = best?.1.clone();
        worker.active.fetch_add(1, Ordering::Relaxed);
        Some(worker)
    }

    /// Hands free capacity to queued requests, oldest first.
    fn dispatch_waiters(&self, state: &mut PoolState) {
        while !state.waiters.is_empty() {
            let Some(worker) = self.claim(state) else {
                return;
            };
            let waiter = state.waiters.pop_front().expect("not empty");
            if let Err(slot) = waiter.slot.send(self.slot(&worker)) {
                // The waiter timed out meanwhile; take the claim back.
                slot.disarm().active.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    fn release(&self, worker: &Arc<WorkerShared>) {
        let mut state = self.lock();
        worker.active.fetch_sub(1, Ordering::Relaxed);
        self.dispatch_waiters(&mut state);
    }

    async fn acquire(&self) -> Result<Slot, RenderError> {
        let (id, receiver) = {
            let mut state = self.lock();
            if self.is_shutting_down() {
                metrics::add(&self.counters.rejected_shutting_down, 1);
                return Err(RenderError::ShuttingDown);
            }
            if let Some(worker) = self.claim(&mut state) {
                return Ok(self.slot(&worker));
            }
            if state.waiters.len() >= self.config.limits.max_queued {
                metrics::add(&self.counters.rejected_overloaded, 1);
                return Err(RenderError::Overloaded);
            }
            let (sender, receiver) = oneshot::channel();
            state.next_waiter += 1;
            let id = state.next_waiter;
            state.waiters.push_back(Waiter { id, slot: sender });
            (id, receiver)
        };
        // A caller that gives up (client gone) leaves the queue at once
        // instead of holding a queue position until a slot passes by.
        let _leave = LeaveQueue { pool: self, id };
        let started = Instant::now();
        let outcome = tokio::time::timeout(self.config.limits.queue_timeout, receiver).await;
        metrics::add(&self.counters.queue_waits, 1);
        metrics::add(
            &self.counters.queue_wait_micros,
            started.elapsed().as_micros() as u64,
        );
        match outcome {
            Ok(Ok(slot)) => Ok(slot),
            // The pool dropped the waiter: shutdown.
            Ok(Err(_)) => {
                metrics::add(&self.counters.rejected_shutting_down, 1);
                Err(RenderError::ShuttingDown)
            }
            Err(_) => {
                metrics::add(&self.counters.rejected_queue_timeout, 1);
                Err(RenderError::QueueTimeout)
            }
        }
    }

    fn install(&self, worker: &Arc<WorkerShared>) {
        let mut state = self.lock();
        worker.set_state(READY);
        state.workers[worker.slot] = Some(worker.clone());
        state.history[worker.slot].generation = worker.generation;
        self.dispatch_waiters(&mut state);
    }

    fn uninstall(&self, worker: &Arc<WorkerShared>, failure: Option<(FailureReason, &str)>) {
        let mut state = self.lock();
        let current = &mut state.workers[worker.slot];
        if current
            .as_ref()
            .is_some_and(|w| w.generation == worker.generation)
        {
            *current = None;
        }
        if let Some((reason, detail)) = failure {
            let history = &mut state.history[worker.slot];
            history.failures[reason.index()] += 1;
            history.last_failure = Some(format!("{}: {}", reason.as_str(), first_line(detail)));
        }
    }

    fn record_restart(&self, slot: usize) {
        self.lock().history[slot].restarts += 1;
    }

    /// Sleeps up to `duration`; returns true if shutdown began.
    fn wait_for_shutdown(&self, duration: Duration) -> bool {
        let (lock, condvar) = &self.shutdown;
        let guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let (guard, _) = condvar
            .wait_timeout_while(guard, duration, |stopping| !*stopping)
            .unwrap_or_else(|e| e.into_inner());
        *guard
    }

    fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        let waiters = std::mem::take(&mut self.lock().waiters);
        drop(waiters);
        let (lock, condvar) = &self.shutdown;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        condvar.notify_all();
    }

    fn snapshot(&self) -> MetricsSnapshot {
        let mut snapshot = MetricsSnapshot::from_counters(&self.counters);
        let limits = &self.config.limits;
        snapshot.isolates = limits.isolates;
        snapshot.max_active_per_isolate = limits.max_active_per_isolate;
        snapshot.max_queued = limits.max_queued;
        let state = self.lock();
        snapshot.queued = state.waiters.len();
        snapshot.executors = state
            .history
            .iter()
            .enumerate()
            .map(|(slot, history)| {
                let worker = state.workers[slot].as_ref();
                ExecutorSnapshot {
                    slot,
                    generation: history.generation,
                    state: worker.map_or("restarting", |w| state_name(w.state())),
                    active: worker.map_or(0, |w| w.active.load(Ordering::Relaxed)),
                    heap_used_bytes: worker.map_or(0, |w| w.heap_used.load(Ordering::Relaxed)),
                    heap_total_bytes: worker.map_or(0, |w| w.heap_total.load(Ordering::Relaxed)),
                    external_bytes: worker.map_or(0, |w| w.heap_external.load(Ordering::Relaxed)),
                    heap_used_after_gc_bytes: worker
                        .map_or(0, |w| w.heap_after_gc.load(Ordering::Relaxed)),
                    full_gcs: worker.map_or(0, |w| w.full_gcs.load(Ordering::Relaxed)),
                    restarts: history.restarts,
                    failures: FailureReason::ALL
                        .iter()
                        .map(|r| (r.as_str(), history.failures[r.index()]))
                        .collect(),
                    last_failure: history.last_failure.clone(),
                }
            })
            .collect();
        snapshot
    }
}

struct LeaveQueue<'a> {
    pool: &'a PoolInner,
    id: u64,
}

impl Drop for LeaveQueue<'_> {
    fn drop(&mut self) {
        self.pool.lock().waiters.retain(|w| w.id != self.id);
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

/// The JavaScript executor: a supervised pool of V8 isolates. Dropping the
/// last handle shuts it down (see [`Executor::shutdown`]).
pub struct Executor {
    inner: Arc<PoolInner>,
    supervisors: Mutex<Vec<JoinHandle<()>>>,
}

impl Executor {
    /// Starts every isolate and loads Next's Node bootstrap in each.
    /// Returns once all of them accept requests, or the first bootstrap
    /// error (a broken build fails at startup, not on the first request).
    pub fn start(config: ExecutorConfig) -> Result<Arc<Self>, ExecutorError> {
        let isolates = config.limits.isolates.max(1);
        let counters = Arc::new(Counters::default());
        let net = match &config.network {
            Some(network) => Some(Arc::new(
                NetClient::new(network, counters.clone()).map_err(|e| {
                    ExecutorError::Io(std::io::Error::other(format!(
                        "outbound network client: {e}"
                    )))
                })?,
            )),
            None => None,
        };
        let inner = Arc::new_cyclic(|me| PoolInner {
            me: me.clone(),
            config,
            counters,
            net,
            state: Mutex::new(PoolState {
                workers: vec![None; isolates],
                history: (0..isolates).map(|_| SlotHistory::default()).collect(),
                ..PoolState::default()
            }),
            shutting_down: AtomicBool::new(false),
            shutdown: (Mutex::new(false), Condvar::new()),
        });
        let (booted, boots) = std::sync::mpsc::channel();
        let mut supervisors = Vec::new();
        for slot in 0..isolates {
            let inner = inner.clone();
            let booted = booted.clone();
            let handle = std::thread::Builder::new()
                .name(format!("nextrs-supervisor-{slot}"))
                .spawn(move || supervise(inner, slot, Some(booted)))
                .map_err(ExecutorError::Io)?;
            supervisors.push(handle);
        }
        drop(booted);
        let mut failure = None;
        for _ in 0..isolates {
            match boots.recv() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failure.get_or_insert(ExecutorError::Bootstrap(error));
                }
                Err(_) => {
                    failure.get_or_insert(ExecutorError::Stopped);
                }
            }
        }
        let executor = Self {
            inner,
            supervisors: Mutex::new(supervisors),
        };
        match failure {
            None => Ok(Arc::new(executor)),
            Some(error) => {
                executor.shutdown();
                Err(error)
            }
        }
    }

    /// Queues a request for an isolate. Fails at once when the pool is
    /// overloaded or shutting down, or after `queue_timeout`; in those
    /// cases JavaScript never saw the request.
    pub async fn render(&self, request: RenderRequest) -> Result<ResponseStream, RenderError> {
        let queued_at = Instant::now();
        let limits = &self.inner.config.limits;
        let (sender, stream) =
            response::channel(limits.response_buffer_bytes, self.inner.counters.clone());
        let mut job = Box::new(Job {
            request,
            response: sender,
            slot: self.inner.acquire().await?,
            timeout_ms: limits.request_timeout.map(|t| t.as_millis() as u64),
            queued_at,
        });
        // An isolate can die between being chosen and taking the job. The
        // job never reached JavaScript, so dispatching it again is safe.
        let mut attempts = 0;
        let worker = loop {
            let worker = job.slot.worker.clone();
            match worker.send(job) {
                Ok(()) => break worker,
                Err(returned) => {
                    worker.mark_exited();
                    attempts += 1;
                    job = returned;
                    if attempts > limits.isolates {
                        metrics::add(&self.inner.counters.rejected_unavailable, 1);
                        return Err(RenderError::Unavailable);
                    }
                    // Replacing the slot releases the dead isolate's one.
                    job.slot = self.inner.acquire().await?;
                }
            }
        };
        let counters = &self.inner.counters;
        metrics::add(&counters.requests_admitted, 1);
        tracing::trace!(
            target: "nextrs::executor",
            slot = worker.slot,
            generation = worker.generation,
            "request dispatched"
        );
        Ok(stream)
    }

    pub fn metrics(&self) -> MetricsSnapshot {
        self.inner.snapshot()
    }

    pub fn limits(&self) -> &crate::Limits {
        &self.inner.config.limits
    }

    /// Stops admitting requests, lets in-flight requests finish for up to
    /// `shutdown_grace`, then terminates whatever still runs. Blocks until
    /// every isolate thread has exited. Idempotent.
    pub fn shutdown(&self) {
        self.inner.begin_shutdown();
        let supervisors =
            std::mem::take(&mut *self.supervisors.lock().unwrap_or_else(|e| e.into_inner()));
        for handle in supervisors {
            let _ = handle.join();
        }
    }

    /// Test hook: fails the isolate in `slot` as the watchdog would.
    #[cfg(feature = "fault-injection")]
    pub fn fault_terminate(&self, slot: usize) -> bool {
        let state = self.inner.lock();
        match &state.workers[slot] {
            Some(worker) => {
                worker.terminate(Termination::Failed(FailureReason::CpuStall));
                true
            }
            None => false,
        }
    }
}

impl std::fmt::Debug for Executor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Executor")
            .field("isolates", &self.inner.config.limits.isolates)
            .field("shutting_down", &self.inner.is_shutting_down())
            .finish()
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// What the isolate thread reports when it ends.
pub(crate) type WorkerExit = Result<(), String>;

/// Runs one slot: start an isolate, watch it, replace it when it fails.
fn supervise(
    inner: Arc<PoolInner>,
    slot: usize,
    mut first_boot: Option<std::sync::mpsc::Sender<Result<(), String>>>,
) {
    let limits = inner.config.limits.clone();
    let mut recent_failures: VecDeque<Instant> = VecDeque::new();
    let mut generation = 0;
    loop {
        if inner.is_shutting_down() {
            break;
        }
        generation += 1;
        let (inbox, jobs) = mpsc::unbounded_channel();
        let worker = Arc::new(WorkerShared::new(slot, generation, inbox));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (exit_tx, exit_rx) = std::sync::mpsc::channel();
        let started = Instant::now();
        let thread = match worker::spawn(
            inner.config.clone(),
            worker.clone(),
            inner.counters.clone(),
            inner.net.clone(),
            jobs,
            ready_tx,
            exit_tx,
        ) {
            Ok(thread) => Some(thread),
            Err(error) => {
                tracing::error!(target: "nextrs::executor", slot, %error, "cannot spawn isolate thread");
                if let Some(first) = first_boot.take() {
                    let _ = first.send(Err(error.to_string()));
                    return;
                }
                None
            }
        };

        let mut exit: Option<WorkerExit> = None;
        let mut failure: Option<(FailureReason, String)> = None;
        if thread.is_some() {
            match wait_ready(&inner, &worker, &ready_rx) {
                Ok(()) => {
                    // Dispatchable before start() returns.
                    inner.install(&worker);
                    if let Some(first) = first_boot.take() {
                        let _ = first.send(Ok(()));
                    }
                    tracing::info!(
                        target: "nextrs::executor",
                        slot,
                        generation,
                        boot_ms = started.elapsed().as_millis() as u64,
                        "isolate ready"
                    );
                    exit = Some(watch(&inner, &worker, &exit_rx));
                }
                Err(error) => {
                    if let Some(first) = first_boot.take() {
                        worker.terminate(Termination::Shutdown);
                        if let Some(thread) = thread {
                            let _ = thread.join();
                        }
                        let _ = first.send(Err(error));
                        return;
                    }
                    if !inner.is_shutting_down() {
                        failure = Some((FailureReason::Bootstrap, error));
                    }
                }
            }
        } else {
            failure = Some((FailureReason::Bootstrap, "thread spawn failed".into()));
        }

        // Out of dispatch before anything else.
        if worker.state() == READY {
            worker.set_state(if inner.is_shutting_down() {
                STOPPED
            } else {
                FAILED
            });
        }
        worker.close_inbox();
        let panicked = thread.is_some_and(|t| t.join().is_err());
        if failure.is_none() {
            failure = classify(&inner, &worker, exit, panicked);
        }
        worker.set_state(if failure.is_some() { FAILED } else { STOPPED });
        inner.uninstall(&worker, failure.as_ref().map(|(r, d)| (*r, d.as_str())));

        let Some((reason, detail)) = failure else {
            break;
        };
        tracing::error!(
            target: "nextrs::executor",
            slot,
            generation,
            reason = reason.as_str(),
            uptime_ms = started.elapsed().as_millis() as u64,
            "isolate failed: {}",
            first_line(&detail)
        );
        if inner.is_shutting_down() {
            break;
        }
        let now = Instant::now();
        while recent_failures
            .front()
            .is_some_and(|t| now.duration_since(*t) > FAILURE_WINDOW)
        {
            recent_failures.pop_front();
        }
        recent_failures.push_back(now);
        let exponent = (recent_failures.len() as u32 - 1).min(16);
        let delay = limits
            .restart_backoff_initial
            .saturating_mul(1 << exponent)
            .min(limits.restart_backoff_max);
        tracing::warn!(
            target: "nextrs::executor",
            slot,
            delay_ms = delay.as_millis() as u64,
            recent_failures = recent_failures.len(),
            "restarting isolate"
        );
        if inner.wait_for_shutdown(delay) {
            break;
        }
        inner.record_restart(slot);
    }
}

/// Waits for the isolate's bootstrap, applying the stall watchdog.
fn wait_ready(
    inner: &PoolInner,
    worker: &WorkerShared,
    ready: &Receiver<Result<(), String>>,
) -> Result<(), String> {
    loop {
        match ready.recv_timeout(WATCH_INTERVAL) {
            Ok(result) => return result,
            Err(RecvTimeoutError::Disconnected) => {
                return Err("the isolate thread exited during bootstrap".into());
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if inner.is_shutting_down() {
            worker.terminate(Termination::Shutdown);
        }
        if let Some(limit) = inner.config.limits.stall_timeout
            && worker.busy_for() > limit
        {
            worker.terminate(Termination::Failed(FailureReason::CpuStall));
        }
    }
}

/// Watches a ready isolate until its thread reports an exit.
fn watch(inner: &PoolInner, worker: &WorkerShared, exit: &Receiver<WorkerExit>) -> WorkerExit {
    let limits = &inner.config.limits;
    let mut draining_since: Option<Instant> = None;
    loop {
        match exit.recv_timeout(WATCH_INTERVAL) {
            Ok(result) => return result,
            // Exited without reporting: the thread panicked.
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => {}
        }
        if let Some(limit) = limits.stall_timeout
            && worker.busy_for() > limit
            && worker.termination().is_none()
        {
            tracing::error!(
                target: "nextrs::executor",
                slot = worker.slot,
                busy_ms = worker.busy_for().as_millis() as u64,
                active = worker.active.load(Ordering::Relaxed),
                "JavaScript exceeded the stall timeout; terminating the isolate"
            );
            worker.terminate(Termination::Failed(FailureReason::CpuStall));
        }
        if inner.is_shutting_down() {
            match draining_since {
                None => {
                    worker.set_state(DRAINING);
                    worker.close_inbox();
                    draining_since = Some(Instant::now());
                }
                Some(since) => {
                    let idle = worker.active.load(Ordering::Relaxed) == 0;
                    if idle || since.elapsed() >= limits.shutdown_grace {
                        if !idle {
                            tracing::warn!(
                                target: "nextrs::executor",
                                slot = worker.slot,
                                active = worker.active.load(Ordering::Relaxed),
                                "shutdown grace expired; terminating in-flight requests"
                            );
                        }
                        worker.terminate(Termination::Shutdown);
                    }
                }
            }
        }
    }
}

fn classify(
    inner: &PoolInner,
    worker: &WorkerShared,
    exit: Option<WorkerExit>,
    panicked: bool,
) -> Option<(FailureReason, String)> {
    if panicked {
        return Some((FailureReason::Panic, "the isolate thread panicked".into()));
    }
    match (worker.termination(), exit) {
        (Some(Termination::Shutdown), _) => None,
        (Some(Termination::Failed(reason)), Some(Err(error))) => Some((reason, error)),
        (Some(Termination::Failed(reason)), _) => Some((reason, "terminated".into())),
        (None, Some(Err(error))) => Some((FailureReason::Uncaught, error)),
        (None, _) if inner.is_shutting_down() => None,
        (None, _) => Some((FailureReason::UnexpectedExit, "the event loop ended".into())),
    }
}
