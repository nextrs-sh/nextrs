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
//! * **Memory admission** (optional, `Limits::memory_admission`). A gate
//!   in the pool state, used under the same lock, decides from the
//!   container's memory whether a request is admitted, waits, or is
//!   refused ([`crate::admission`]). With it on, a newcomer never
//!   overtakes the queue, and isolates near their heap limit are skipped
//!   while they have requests in flight.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use deno_core::v8;
use tokio::sync::{Notify, mpsc, oneshot};

use crate::admission::{CommitmentId, Decision, Gate, Ticket};
use crate::cache::CacheStore;
use crate::metrics::CacheSnapshot;
use crate::metrics::{self, AdmissionSnapshot, Counters, ExecutorSnapshot, MetricsSnapshot};
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

/// Why the isolates are asked to give memory back
/// ([`Executor::reclaim_memory`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReclaimLevel {
    /// The server has been idle: V8's `LowMemoryNotification`, a full,
    /// compacting collection that also releases unused heap pages.
    Idle,
    /// The container is near its memory limit: V8's critical memory
    /// pressure response (an immediate full collection, shrinking the
    /// heap), possibly while requests run.
    Pressure,
}

impl ReclaimLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Pressure => "pressure",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Idle => 0,
            Self::Pressure => 1,
        }
    }
}

/// What a memory reclamation did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReclaimReport {
    /// Isolates that collected garbage in time.
    pub isolates: usize,
    /// Committed V8 heap memory given back, summed over the isolates.
    pub heap_released_bytes: u64,
    /// Collection time, summed over the isolates.
    pub gc_time: Duration,
}

/// One isolate's part of a reclamation, sent back by its thread.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Reclaimed {
    pub heap_released_bytes: u64,
    pub gc_time: Duration,
}

/// Reclamations an isolate has yet to perform: the strongest level asked
/// for, and everyone waiting for it.
pub(crate) struct ReclaimRequest {
    pub level: ReclaimLevel,
    pub done: Vec<oneshot::Sender<Reclaimed>>,
}

/// How long `reclaim_memory` waits for an isolate busy in JavaScript; the
/// request stays queued and is performed when it next yields.
const RECLAIM_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// The isolate thread's CPU time (ns) at its last heartbeat, and the
    /// clock to read it from another thread (`thread_cpu_clock`). Unset
    /// where no per-thread CPU clock exists: stalls are then judged by
    /// wall time alone.
    heartbeat_cpu_ns: AtomicU64,
    cpu_clock: OnceLock<CpuClock>,
    /// V8 heap statistics, sampled by the isolate thread (`record_heap`).
    heap_used: AtomicUsize,
    heap_total: AtomicUsize,
    heap_external: AtomicUsize,
    /// The heap limit V8 enforces (its configuration, plus any headroom
    /// granted at the near-limit callback).
    heap_limit: AtomicUsize,
    /// Heap in use right after the latest full (mark-sweep) collection:
    /// the live-data floor. 0 until the first full GC (`record_full_gc`).
    heap_after_gc: AtomicUsize,
    full_gcs: AtomicU64,
    /// Near-heap-limit callback calls (RE-4E): all of them, those declined
    /// (V8's last-resort collection after a refused ArrayBuffer
    /// allocation, not a full heap), and the limit V8 passed at the latest
    /// and the first call (0: none).
    near_limit_calls: AtomicU64,
    near_limit_declined: AtomicU64,
    near_limit_last_bytes: AtomicUsize,
    near_limit_first_bytes: AtomicUsize,
    /// When the first granted call asked for termination (ms since
    /// `epoch`, 0: never), for the time to unwind and replace.
    near_limit_terminated_ms: AtomicU64,
    pub isolate: OnceLock<v8::IsolateHandle>,
    termination: Mutex<Option<Termination>>,
    stop: Notify,
    stopping: AtomicBool,
    /// Memory reclamation asked of this isolate, taken by its thread
    /// between turns of the event loop.
    reclaim: Mutex<Option<ReclaimRequest>>,
    reclaim_signal: Notify,
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
            heartbeat_cpu_ns: AtomicU64::new(0),
            cpu_clock: OnceLock::new(),
            heap_used: AtomicUsize::new(0),
            heap_total: AtomicUsize::new(0),
            heap_external: AtomicUsize::new(0),
            heap_limit: AtomicUsize::new(0),
            heap_after_gc: AtomicUsize::new(0),
            full_gcs: AtomicU64::new(0),
            near_limit_calls: AtomicU64::new(0),
            near_limit_declined: AtomicU64::new(0),
            near_limit_last_bytes: AtomicUsize::new(0),
            near_limit_first_bytes: AtomicUsize::new(0),
            near_limit_terminated_ms: AtomicU64::new(0),
            isolate: OnceLock::new(),
            termination: Mutex::new(None),
            stop: Notify::new(),
            stopping: AtomicBool::new(false),
            reclaim: Mutex::new(None),
            reclaim_signal: Notify::new(),
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

    /// Called on the isolate thread whenever its event loop has control.
    pub fn beat(&self) {
        let now = self.epoch.elapsed().as_millis() as u64;
        if let Some(clock) = self.cpu_clock.get()
            && let Some(cpu) = clock.now_ns()
        {
            self.heartbeat_cpu_ns.store(cpu, Ordering::Relaxed);
        }
        self.heartbeat_ms.store(now, Ordering::Relaxed);
    }

    /// Called once on the isolate thread: lets the supervisor read this
    /// thread's CPU time.
    pub fn register_cpu_clock(&self) {
        if let Some(clock) = CpuClock::current_thread() {
            let _ = self.cpu_clock.set(clock);
        }
    }

    /// CPU time the isolate thread has used since its event loop last had
    /// control, if a per-thread CPU clock is available.
    fn busy_cpu_for(&self) -> Option<Duration> {
        let now = self.cpu_clock.get()?.now_ns()?;
        Some(Duration::from_nanos(now.saturating_sub(
            self.heartbeat_cpu_ns.load(Ordering::Relaxed),
        )))
    }

    pub fn record_heap(&self, stats: &v8::HeapStatistics) {
        self.heap_used
            .store(stats.used_heap_size(), Ordering::Relaxed);
        self.heap_total
            .store(stats.total_heap_size(), Ordering::Relaxed);
        self.heap_external
            .store(stats.external_memory(), Ordering::Relaxed);
        self.heap_limit
            .store(stats.heap_size_limit(), Ordering::Relaxed);
    }

    /// V8's near-heap-limit callback, on the isolate thread: `current` is
    /// the old-generation limit V8 enforces now, `initial` the configured
    /// one. Returns the new limit.
    ///
    /// Normally: terminate the isolate (it is replaced) and grant
    /// `headroom` so that the termination can unwind instead of V8 aborting
    /// the process. Every call is counted and logged; V8 may call it more
    /// than once per episode, and each granted call raises the limit again
    /// (RE-4-V8-HEAP-OVERSHOOT).
    ///
    /// Declined (limit unchanged, isolate kept) only when this thread's
    /// ArrayBuffer budget refused an allocation (and none succeeded since,
    /// within a second) *and* the heap is clearly not at its limit: the
    /// committed old generation below 90 % of `current` and all committed
    /// heap below `current`. V8's last-resort collection for that refusal
    /// calls this whatever the heap's size, and the refusal must reach
    /// JavaScript as a RangeError, not replace the isolate. V8's other two
    /// calls (a heap that reached its limit, ineffective mark-compacts)
    /// happen only when the old generation's capacity is at the limit,
    /// which committed memory bounds from above: they are never declined
    /// (declining them would abort the process).
    pub fn on_near_heap_limit(
        &self,
        current: usize,
        initial: usize,
        headroom: usize,
        heap: CommittedHeap,
    ) -> usize {
        let call = self.near_limit_calls.fetch_add(1, Ordering::Relaxed) + 1;
        self.near_limit_last_bytes.store(current, Ordering::Relaxed);
        let _ = self.near_limit_first_bytes.compare_exchange(
            0,
            current,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        let live = self.heap_after_gc.load(Ordering::Relaxed);
        let clearly_below = heap.old < current / 10 * 9 && heap.total < current;
        if clearly_below && crate::external::take_refusal() {
            self.near_limit_declined.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(
                target: "nextrs::executor",
                slot = self.slot,
                call,
                limit_mib = current / (1024 * 1024),
                committed_old_mib = heap.old / (1024 * 1024),
                live_mib = live / (1024 * 1024),
                "near-heap-limit call after a refused ArrayBuffer allocation; heap not full, \
                 limit kept"
            );
            return current;
        }
        let granted = current.saturating_add(headroom);
        let now = self.epoch.elapsed().as_millis() as u64;
        let _ = self.near_limit_terminated_ms.compare_exchange(
            0,
            now.max(1),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        // Once at warn per isolate; V8's further calls in the episode at
        // debug (the episode summary follows when the isolate exits).
        macro_rules! near_limit_event {
            ($level:ident) => {
                tracing::$level!(
                    target: "nextrs::executor",
                    slot = self.slot,
                    call,
                    limit_mib = current / (1024 * 1024),
                    initial_mib = initial / (1024 * 1024),
                    granted_mib = granted / (1024 * 1024),
                    committed_old_mib = heap.old / (1024 * 1024),
                    live_after_gc_mib = live / (1024 * 1024),
                    external_mib = self.heap_external.load(Ordering::Relaxed) / (1024 * 1024),
                    "V8 heap near its limit: terminating the isolate"
                )
            };
        }
        if call == 1 + self.near_limit_declined.load(Ordering::Relaxed) {
            near_limit_event!(warn);
        } else {
            near_limit_event!(debug);
        }
        self.terminate(Termination::Failed(FailureReason::HeapLimit));
        granted
    }

    /// Whether the sampled heap use is at least `percent` of the limit.
    fn near_heap_limit(&self, percent: f64) -> bool {
        let limit = self.heap_limit.load(Ordering::Relaxed);
        limit > 0
            && (self.heap_used.load(Ordering::Relaxed) as f64) >= limit as f64 * percent / 100.0
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
        self.stopping.store(true, Ordering::SeqCst);
        self.stop.notify_one();
        self.interrupt();
    }

    /// Interrupts running JavaScript if a termination was requested. The
    /// supervisor repeats it until the thread exits: a request made before
    /// the isolate existed (its handle is published after creation, which
    /// can be slow on a cold machine) would otherwise be lost, and a
    /// bootstrap that never yields would run forever.
    pub(crate) fn interrupt(&self) {
        if !self.stopping.load(Ordering::SeqCst) {
            return;
        }
        if let Some(isolate) = self.isolate.get() {
            isolate.terminate_execution();
        }
    }

    fn termination(&self) -> Option<Termination> {
        *self.termination.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Asks the isolate thread to collect garbage at `level` (or the
    /// stronger level already pending) and to report on `done`.
    fn request_reclaim(&self, level: ReclaimLevel, done: oneshot::Sender<Reclaimed>) {
        {
            let mut pending = self.reclaim.lock().unwrap_or_else(|e| e.into_inner());
            match pending.as_mut() {
                Some(request) => {
                    request.level = request.level.max(level);
                    request.done.push(done);
                }
                None => {
                    *pending = Some(ReclaimRequest {
                        level,
                        done: vec![done],
                    });
                }
            }
        }
        self.reclaim_signal.notify_one();
    }

    /// Resolves with the next reclamation request (isolate thread).
    /// Cancel-safe: a request is only taken in the poll that returns it.
    pub async fn reclaim_requested(&self) -> ReclaimRequest {
        loop {
            let signal = self.reclaim_signal.notified();
            if let Some(request) = self
                .reclaim
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                return request;
            }
            signal.await;
        }
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

/// Committed heap memory at a near-heap-limit call (`worker::committed_heap`).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CommittedHeap {
    /// Every space except the young generation's.
    pub old: usize,
    /// All spaces.
    pub total: usize,
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
    /// With memory admission, the request's commitment, released with the
    /// slot (RE-4.1B): its lifetime is the request's.
    commitment: Option<CommitmentId>,
}

impl Slot {
    /// Gives the claim back without going through `release` (which takes
    /// the pool lock), for use while that lock is held: the caller undoes
    /// the claim and the commitment.
    fn disarm(mut self) -> (Arc<WorkerShared>, Option<CommitmentId>) {
        self.pool = None;
        (self.worker.clone(), self.commitment.take())
    }

    /// Releases the claim and the commitment now (the drop then does
    /// nothing): for a request about to acquire a replacement slot, which
    /// must not count its dead one as active or keep its estimate twice.
    fn release_now(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.release(&self.worker, self.commitment.take());
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.release(&self.worker, self.commitment.take());
        }
    }
}

struct Waiter {
    id: u64,
    slot: oneshot::Sender<Result<Slot, RenderError>>,
    ticket: Ticket,
}

#[derive(Default)]
struct SlotHistory {
    generation: u64,
    restarts: u64,
    failures: [u64; FailureReason::ALL.len()],
    last_failure: Option<String>,
    /// Near-heap-limit calls of the slot's earlier isolates.
    near_limit_calls: u64,
    near_limit_declined: u64,
    last_heap_limit: Option<crate::metrics::HeapLimitEpisode>,
}

#[derive(Default)]
struct PoolState {
    /// The isolate currently serving each slot (None while it restarts).
    workers: Vec<Option<Arc<WorkerShared>>>,
    history: Vec<SlotHistory>,
    waiters: VecDeque<Waiter>,
    next_waiter: u64,
    /// Header and URL bytes the queued requests hold (`Ticket::queued_bytes`).
    queued_bytes: usize,
    /// Memory-aware admission, when enabled.
    gate: Option<Gate>,
}

pub(crate) struct PoolInner {
    me: Weak<PoolInner>,
    pub config: ExecutorConfig,
    pub counters: Arc<Counters>,
    /// Shared by every isolate (one connection pool); `None` without a
    /// network configuration.
    pub net: Option<Arc<NetClient>>,
    /// Shared by every isolate; `None` when disabled.
    pub cache: Option<Arc<CacheStore>>,
    /// Next.js requests to its own server, served in-process.
    pub internal: crate::internal::InternalRoute,
    /// The ArrayBuffer budget all isolates allocate from, when limited.
    pub external: Option<Arc<crate::external::ExternalBudget>>,
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

    fn slot(&self, worker: &Arc<WorkerShared>, commitment: Option<CommitmentId>) -> Slot {
        Slot {
            pool: self.me.upgrade(),
            worker: worker.clone(),
            commitment,
        }
    }

    /// Records the admission of `ticket` with the gate, if there is one.
    /// Requires the pool lock.
    fn commit(state: &mut PoolState, ticket: &Ticket) -> Option<CommitmentId> {
        state
            .gate
            .as_mut()
            .map(|gate| gate.admit(Instant::now(), ticket))
    }

    /// Least outstanding requests among ready isolates with a free slot,
    /// preferring isolates whose event loop is turning (a request sent to
    /// one stuck on the CPU would wait behind it, and fail with it if the
    /// watchdog fires). Ties go to the lowest slot: at low concurrency,
    /// spreading sequential requests round-robin left every isolate's JIT
    /// and caches lukewarm (4 isolates at concurrency 1: 313 against 441
    /// req/s on /books/42). Claims the slot. Requires the pool lock.
    fn claim(&self, state: &mut PoolState, gated: bool) -> Option<Arc<WorkerShared>> {
        let limit = self.config.limits.max_active_per_isolate;
        let heap_percent = state
            .gate
            .as_ref()
            .filter(|_| gated)
            .and_then(|gate| gate.config().heap_percent);
        let mut best: Option<((bool, usize), &Arc<WorkerShared>)> = None;
        for worker in state.workers.iter().flatten() {
            let active = worker.active.load(Ordering::Relaxed);
            if worker.state() != READY || active >= limit {
                continue;
            }
            // With memory admission: an isolate about to reach its heap
            // limit takes no more requests while it has some (a heap-limit
            // termination fails all of them). An idle one always may.
            if let Some(percent) = heap_percent
                && active > 0
                && worker.near_heap_limit(percent)
            {
                metrics::add(&self.counters.admission_heap_skips, 1);
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

    /// Requests active over all isolates. Requires the pool lock.
    fn total_active(state: &PoolState) -> usize {
        state
            .workers
            .iter()
            .flatten()
            .map(|w| w.active.load(Ordering::Relaxed))
            .sum()
    }

    /// Hands free capacity to queued requests, oldest first. With memory
    /// admission, the oldest waiter also needs the gate's consent: while it
    /// must wait, everyone behind it waits too (no overtaking); in the
    /// critical state queued requests are refused rather than held.
    fn dispatch_waiters(&self, state: &mut PoolState) {
        while let Some(front) = state.waiters.front() {
            let ticket = front.ticket;
            let active = Self::total_active(state);
            let decision = if ticket.nested || ticket.retry {
                None
            } else {
                state
                    .gate
                    .as_mut()
                    .map(|gate| gate.decide(Instant::now(), ticket.reserve, active))
            };
            let progress = decision == Some(Decision::Progress);
            match decision {
                None | Some(Decision::Admit | Decision::Progress) => {}
                Some(Decision::Defer) => return,
                Some(Decision::Refuse) => {
                    let waiter = state.waiters.pop_front().expect("not empty");
                    state.queued_bytes = state.queued_bytes.saturating_sub(ticket.queued_bytes);
                    metrics::add(
                        &self.counters.rejected_memory_pressure[ticket.class.index()],
                        1,
                    );
                    let _ = waiter.slot.send(Err(RenderError::MemoryPressure));
                    continue;
                }
            }
            let Some(worker) = self.claim(state, !ticket.nested && !ticket.retry) else {
                return;
            };
            let waiter = state.waiters.pop_front().expect("not empty");
            state.queued_bytes = state.queued_bytes.saturating_sub(ticket.queued_bytes);
            let commitment = Self::commit(state, &ticket);
            match waiter.slot.send(Ok(self.slot(&worker, commitment))) {
                Ok(()) => {
                    if progress {
                        metrics::add(&self.counters.admission_progress, 1);
                    }
                }
                // The waiter timed out meanwhile; take the claim and the
                // commitment back.
                Err(Ok(slot)) => {
                    let (worker, commitment) = slot.disarm();
                    worker.active.fetch_sub(1, Ordering::Relaxed);
                    if let (Some(gate), Some(id)) = (state.gate.as_mut(), commitment) {
                        gate.withdraw(id);
                    }
                }
                Err(Err(_)) => {}
            }
        }
    }

    fn release(&self, worker: &Arc<WorkerShared>, commitment: Option<CommitmentId>) {
        let mut state = self.lock();
        worker.active.fetch_sub(1, Ordering::Relaxed);
        if let (Some(gate), Some(id)) = (state.gate.as_mut(), commitment) {
            gate.complete(id);
        }
        self.dispatch_waiters(&mut state);
    }

    async fn acquire(&self, ticket: Ticket) -> Result<Slot, RenderError> {
        let (id, receiver) = {
            let mut state = self.lock();
            if self.is_shutting_down() {
                metrics::add(&self.counters.rejected_shutting_down, 1);
                return Err(RenderError::ShuttingDown);
            }
            let active = Self::total_active(&state);
            let queued = !state.waiters.is_empty();
            // Next.js's requests to its own server belong to an admitted
            // request: not gated (see `admission::nested`); nor is a retry
            // of a request that was admitted, whose isolate died before
            // taking it (RE-4.1 review F5). Concurrency limits still apply.
            let decision = if ticket.nested || ticket.retry {
                None
            } else {
                state
                    .gate
                    .as_mut()
                    .map(|gate| gate.decide(Instant::now(), ticket.reserve, active))
            };
            let mut deferred = false;
            match decision {
                None => {
                    if let Some(worker) = self.claim(&mut state, false) {
                        // A nested request's estimate is recorded (not
                        // gated): its memory is as real as its parent's.
                        let commitment = Self::commit(&mut state, &ticket);
                        return Ok(self.slot(&worker, commitment));
                    }
                }
                Some(Decision::Refuse) => {
                    metrics::add(
                        &self.counters.rejected_memory_pressure[ticket.class.index()],
                        1,
                    );
                    return Err(RenderError::MemoryPressure);
                }
                // Behind earlier waiters, a newcomer queues (FIFO).
                Some(Decision::Admit | Decision::Progress) if queued => deferred = true,
                Some(decision @ (Decision::Admit | Decision::Progress)) => {
                    if let Some(worker) = self.claim(&mut state, true) {
                        let commitment = Self::commit(&mut state, &ticket);
                        if decision == Decision::Progress {
                            metrics::add(&self.counters.admission_progress, 1);
                        }
                        return Ok(self.slot(&worker, commitment));
                    }
                }
                Some(Decision::Defer) => {
                    deferred = true;
                    metrics::add(&self.counters.admission_deferred, 1);
                }
            }
            if state.waiters.len() >= self.config.limits.max_queued {
                metrics::add(&self.counters.rejected_overloaded, 1);
                return Err(RenderError::Overloaded);
            }
            if let Some(gate) = &state.gate
                && state.queued_bytes.saturating_add(ticket.queued_bytes)
                    > gate.config().max_queued_bytes
            {
                metrics::add(&self.counters.rejected_overloaded, 1);
                return Err(RenderError::Overloaded);
            }
            let (sender, receiver) = oneshot::channel();
            state.next_waiter += 1;
            let id = state.next_waiter;
            state.queued_bytes = state.queued_bytes.saturating_add(ticket.queued_bytes);
            state.waiters.push_back(Waiter {
                id,
                slot: sender,
                ticket,
            });
            if deferred {
                // The head may be dispatchable now (a newcomer behind it
                // must not be the only one checking).
                self.dispatch_waiters(&mut state);
            }
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
            Ok(Ok(Ok(slot))) => Ok(slot),
            Ok(Ok(Err(error))) => Err(error),
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

    fn install(&self, worker: &Arc<WorkerShared>, heap_limit_at: Option<Instant>) {
        let mut state = self.lock();
        worker.set_state(READY);
        state.workers[worker.slot] = Some(worker.clone());
        state.history[worker.slot].generation = worker.generation;
        if let Some(at) = heap_limit_at
            && let Some(episode) = state.history[worker.slot].last_heap_limit.as_mut()
        {
            episode.replacement_ms = at.elapsed().as_millis() as u64;
            tracing::info!(
                target: "nextrs::executor",
                slot = worker.slot,
                replacement_ms = episode.replacement_ms,
                "isolate replaced after reaching its heap limit"
            );
        }
        self.dispatch_waiters(&mut state);
    }

    fn uninstall(&self, worker: &Arc<WorkerShared>, failure: Option<(FailureReason, &str)>) {
        let mut state = self.lock();
        {
            let history = &mut state.history[worker.slot];
            history.near_limit_calls += worker.near_limit_calls.load(Ordering::Relaxed);
            history.near_limit_declined += worker.near_limit_declined.load(Ordering::Relaxed);
        }
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
        let waiters = {
            let mut state = self.lock();
            state.queued_bytes = 0;
            std::mem::take(&mut state.waiters)
        };
        drop(waiters);
        let (lock, condvar) = &self.shutdown;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        condvar.notify_all();
    }

    fn snapshot(&self) -> MetricsSnapshot {
        let mut snapshot = MetricsSnapshot::from_counters(&self.counters);
        snapshot.external_memory = self.external.as_ref().map(|budget| budget.snapshot());
        snapshot.cache = self.cache.as_ref().map(|cache| {
            CacheSnapshot::new(
                &self.counters,
                self.config.cache.as_ref().is_some_and(|c| c.dir.is_some()),
                cache.entries(),
            )
        });
        let limits = &self.config.limits;
        snapshot.isolates = limits.isolates;
        snapshot.max_active_per_isolate = limits.max_active_per_isolate;
        snapshot.max_queued = limits.max_queued;
        let state = self.lock();
        snapshot.queued = state.waiters.len();
        snapshot.admission = state.gate.as_ref().map(|gate| {
            AdmissionSnapshot::new(&self.counters, gate, state.queued_bytes, Instant::now())
        });
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
                    heap_limit_bytes: worker.map_or(0, |w| w.heap_limit.load(Ordering::Relaxed)),
                    heap_used_after_gc_bytes: worker
                        .map_or(0, |w| w.heap_after_gc.load(Ordering::Relaxed)),
                    full_gcs: worker.map_or(0, |w| w.full_gcs.load(Ordering::Relaxed)),
                    restarts: history.restarts,
                    failures: FailureReason::ALL
                        .iter()
                        .map(|r| (r.as_str(), history.failures[r.index()]))
                        .collect(),
                    last_failure: history.last_failure.clone(),
                    near_heap_limit_calls: history.near_limit_calls
                        + worker.map_or(0, |w| w.near_limit_calls.load(Ordering::Relaxed)),
                    near_heap_limit_declined: history.near_limit_declined
                        + worker.map_or(0, |w| w.near_limit_declined.load(Ordering::Relaxed)),
                    last_heap_limit: history.last_heap_limit,
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
        let mut state = self.pool.lock();
        if let Some(index) = state.waiters.iter().position(|w| w.id == self.id) {
            let waiter = state.waiters.remove(index).expect("found");
            state.queued_bytes = state
                .queued_bytes
                .saturating_sub(waiter.ticket.queued_bytes);
            // A waiter that left at the head may have been blocking others.
            if index == 0 {
                self.pool.dispatch_waiters(&mut state);
            }
        }
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
        let cache = match &config.cache {
            Some(cache) => {
                let build_id =
                    std::fs::read_to_string(config.dist_dir.join("BUILD_ID")).unwrap_or_default();
                let namespace = crate::cache::namespace(build_id.trim(), &config.env);
                Some(Arc::new(
                    CacheStore::open(cache.clone(), &namespace, counters.clone())
                        .map_err(|e| ExecutorError::Io(std::io::Error::other(e)))?,
                ))
            }
            None => None,
        };
        let gate = match &config.limits.memory_admission {
            Some(admission) => match Gate::new(admission.clone(), isolates) {
                Ok(gate) => {
                    tracing::info!(
                        target: "nextrs::admission",
                        source = %gate.probe_description(),
                        budget_mib = gate.budget().map(|b| b / (1024 * 1024)),
                        state = gate.state().as_str(),
                        pressure_percent = admission.pressure_percent,
                        critical_percent = admission.critical_percent,
                        "memory-aware admission enabled"
                    );
                    Some(gate)
                }
                Err(reason) => {
                    tracing::warn!(
                        target: "nextrs::admission",
                        "memory-aware admission is off: {reason}"
                    );
                    None
                }
            },
            None => None,
        };
        let external_limit = config.limits.external_memory_limit_bytes;
        let inner = Arc::new_cyclic(|me| PoolInner {
            me: me.clone(),
            config,
            counters,
            net,
            cache,
            internal: crate::internal::InternalRoute::default(),
            external: external_limit.map(crate::external::ExternalBudget::new),
            state: Mutex::new(PoolState {
                workers: vec![None; isolates],
                history: (0..isolates).map(|_| SlotHistory::default()).collect(),
                gate,
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
        let ticket = Ticket::for_request(
            &request,
            limits
                .memory_admission
                .as_ref()
                .map_or(0, |a| a.request_reserve_bytes),
        );
        let mut job = Box::new(Job {
            request,
            response: sender,
            slot: self.inner.acquire(ticket).await?,
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
                    // The dead isolate's slot and commitment go first, so
                    // that the retry neither counts it as active nor keeps
                    // its estimate twice (RE-4 review finding 5).
                    job.slot.release_now();
                    job.slot = self
                        .inner
                        .acquire(Ticket {
                            retry: true,
                            ..ticket
                        })
                        .await?;
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

    /// Asks every ready isolate to collect garbage and give unused heap
    /// memory back (V8's low-memory or critical memory-pressure response,
    /// by `level`), and waits for them, at most `RECLAIM_TIMEOUT` (an
    /// isolate busy in JavaScript does it when it next yields). Each
    /// isolate collects on its own thread between turns of its event loop:
    /// requests running in it wait for the collection, as for any full GC.
    /// For idle periods and memory pressure, never per request. The native
    /// allocator's free memory is the caller's to release afterwards
    /// ([`crate::release_free_memory`]).
    pub async fn reclaim_memory(&self, level: ReclaimLevel) -> ReclaimReport {
        let workers: Vec<Arc<WorkerShared>> = {
            let state = self.inner.lock();
            state
                .workers
                .iter()
                .flatten()
                .filter(|w| w.state() == READY)
                .cloned()
                .collect()
        };
        let receivers: Vec<_> = workers
            .iter()
            .map(|worker| {
                let (done, receiver) = oneshot::channel();
                worker.request_reclaim(level, done);
                receiver
            })
            .collect();
        let counters = &self.inner.counters;
        metrics::add(&counters.memory_reclaims[level.index()], 1);
        let deadline = tokio::time::Instant::now() + RECLAIM_TIMEOUT;
        let mut report = ReclaimReport::default();
        for receiver in receivers {
            // A dropped sender is an isolate that exited first.
            if let Ok(Ok(done)) = tokio::time::timeout_at(deadline, receiver).await {
                report.isolates += 1;
                report.heap_released_bytes = report
                    .heap_released_bytes
                    .saturating_add(done.heap_released_bytes);
                report.gc_time = report.gc_time.saturating_add(done.gc_time);
            }
        }
        metrics::add(
            &counters.memory_reclaim_micros,
            u64::try_from(report.gc_time.as_micros()).unwrap_or(u64::MAX),
        );
        metrics::add(
            &counters.memory_reclaim_heap_released,
            report.heap_released_bytes,
        );
        report
    }

    /// Whether no request is executing in an isolate or waiting for one.
    pub fn is_idle(&self) -> bool {
        let state = self.inner.lock();
        state.waiters.is_empty()
            && state
                .workers
                .iter()
                .flatten()
                .all(|w| w.active.load(Ordering::Relaxed) == 0)
    }

    /// Whether memory admission moved up into `critical` or `emergency`
    /// since the last call: the reclaimer then reclaims without waiting
    /// for its interval (RE-4.1C). `false` without admission.
    pub fn take_reclaim_request(&self) -> bool {
        self.inner
            .lock()
            .gate
            .as_mut()
            .is_some_and(|gate| gate.take_reclaim_request())
    }

    pub fn metrics(&self) -> MetricsSnapshot {
        self.inner.snapshot()
    }

    pub fn limits(&self) -> &crate::Limits {
        &self.inner.config.limits
    }

    /// Whether the shared data cache saw a revalidation of any of `tags`
    /// (revalidateTag, updateTag, revalidatePath) after `since_ms` (Unix
    /// milliseconds). Always false without the shared cache, which is the
    /// only place NextRS sees revalidations.
    pub fn tags_revalidated_since(&self, tags: &[String], since_ms: u64) -> bool {
        self.inner
            .cache
            .as_ref()
            .is_some_and(|cache| cache.revalidated_since(tags, since_ms))
    }

    /// Whether the data cache is shared by the isolates (and revalidations
    /// are visible to the server).
    /// Forwarding for external rewrites under the network policy; `None`
    /// without one (`--net-allow` not given).
    pub fn upstream(&self) -> Option<crate::net::Upstream> {
        self.inner.net.clone().map(crate::net::Upstream::new)
    }

    pub fn has_shared_cache(&self) -> bool {
        self.inner.cache.is_some()
    }

    /// Lets Next.js reach its own server (`fetch(.., { next: { internal } })`
    /// to `http://localhost`) through `dispatch` instead of the network;
    /// see `InternalDispatch`. Must be called inside a Tokio runtime, which
    /// runs the dispatched requests. Replaces an earlier dispatcher.
    pub fn set_internal_dispatch(&self, dispatch: Arc<dyn crate::InternalDispatch>) {
        self.inner.internal.install(dispatch);
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
    // When the previous isolate's heap-limit termination was requested.
    let mut heap_limit_at: Option<Instant> = None;
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
            crate::ops::HostServices {
                counters: inner.counters.clone(),
                net: inner.net.clone(),
                cache: inner.cache.clone(),
                internal: inner.internal.clone(),
            },
            inner.external.clone(),
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
            match wait_ready(&inner, &worker, &ready_rx, started) {
                Ok(()) => {
                    // Dispatchable before start() returns.
                    inner.install(&worker, heap_limit_at.take());
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
        let terminated_ms = worker.near_limit_terminated_ms.load(Ordering::Relaxed);
        if terminated_ms > 0 {
            let unwind = worker
                .epoch
                .elapsed()
                .saturating_sub(Duration::from_millis(terminated_ms));
            heap_limit_at = Instant::now().checked_sub(unwind);
            let episode = crate::metrics::HeapLimitEpisode {
                generation,
                callback_calls: worker.near_limit_calls.load(Ordering::Relaxed),
                first_limit_bytes: worker.near_limit_first_bytes.load(Ordering::Relaxed),
                last_limit_bytes: worker.near_limit_last_bytes.load(Ordering::Relaxed),
                unwind_ms: unwind.as_millis() as u64,
                replacement_ms: 0,
            };
            tracing::warn!(
                target: "nextrs::executor",
                slot,
                generation,
                callback_calls = episode.callback_calls,
                first_limit_mib = episode.first_limit_bytes / (1024 * 1024),
                last_limit_mib = episode.last_limit_bytes / (1024 * 1024),
                unwind_ms = episode.unwind_ms,
                "heap-limit termination finished"
            );
            inner.lock().history[slot].last_heap_limit = Some(episode);
        }
        if failure.is_none() {
            failure = classify(&inner, &worker, exit, panicked);
        }
        worker.set_state(if failure.is_some() { FAILED } else { STOPPED });
        inner.uninstall(&worker, failure.as_ref().map(|(r, d)| (*r, d.as_str())));
        // Regeneration leases its requests held can no longer be released
        // by them; let the other isolates' waiters go now.
        if let Some(cache) = &inner.cache {
            cache.release_leases_of((slot, generation));
        }

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

/// Waits for the isolate's bootstrap under its own deadline
/// (`bootstrap_timeout`, wall clock). The per-turn stall limit does not
/// apply: loading Next's bundles is one long turn, and under CPU contention
/// it outlasted a 2-3 s stall limit, which killed healthy bootstraps.
fn wait_ready(
    inner: &PoolInner,
    worker: &WorkerShared,
    ready: &Receiver<Result<(), String>>,
    started: Instant,
) -> Result<(), String> {
    let limit = inner.config.limits.bootstrap_timeout;
    let mut expired = false;
    let deadline = || {
        format!(
            "the isolate did not finish its bootstrap within {:.1} s (--js-bootstrap-timeout)",
            limit.unwrap_or_default().as_secs_f64()
        )
    };
    loop {
        // Once the deadline terminated the isolate, whatever it reports
        // (even a late success) is that failure.
        match ready.recv_timeout(WATCH_INTERVAL) {
            Ok(result) => return if expired { Err(deadline()) } else { result },
            Err(RecvTimeoutError::Disconnected) if expired => return Err(deadline()),
            Err(RecvTimeoutError::Disconnected) => {
                return Err("the isolate thread exited during bootstrap".into());
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if inner.is_shutting_down() {
            worker.terminate(Termination::Shutdown);
        }
        // A termination already requested (the deadline, shutdown, the
        // heap limit) is repeated until the thread reports.
        worker.interrupt();
        if let Some(limit) = limit
            && !expired
            && started.elapsed() > limit
        {
            expired = true;
            tracing::error!(
                target: "nextrs::executor",
                slot = worker.slot,
                limit_ms = limit.as_millis() as u64,
                "isolate bootstrap exceeded its deadline; terminating it"
            );
            worker.terminate(Termination::Failed(FailureReason::Bootstrap));
        }
    }
}

/// A turn that has run longer than the stall timeout on the CPU is a
/// runaway; one that has not yielded for `STALL_WALL_FACTOR` times the
/// limit is terminated even if it is blocked rather than computing.
const STALL_WALL_FACTOR: u32 = 6;

/// Whether a turn counts as stalled. With a thread CPU clock, the CPU time
/// of the turn decides: a thread blocked in the kernel (slow disk, page
/// faults, cgroup CPU throttling) is not a runaway, and killing it would
/// turn slowness into failed requests. Wall time is the backstop. Without
/// a CPU clock, wall time decides.
fn stalled(limit: Duration, busy: Duration, cpu: Option<Duration>) -> bool {
    match cpu {
        Some(cpu) => cpu > limit || busy > limit * STALL_WALL_FACTOR,
        None => busy > limit,
    }
}

/// A thread's CPU-time clock, readable from other threads (Linux:
/// `pthread_getcpuclockid`).
#[derive(Debug, Clone, Copy)]
struct CpuClock(libc::clockid_t);

impl CpuClock {
    #[cfg(target_os = "linux")]
    fn current_thread() -> Option<Self> {
        let mut clock: libc::clockid_t = 0;
        // SAFETY: pthread_self() is the calling thread; the out pointer is
        // a valid clockid_t.
        let rc = unsafe { libc::pthread_getcpuclockid(libc::pthread_self(), &mut clock) };
        (rc == 0).then_some(Self(clock))
    }

    #[cfg(not(target_os = "linux"))]
    fn current_thread() -> Option<Self> {
        None
    }

    /// Nanoseconds of CPU the thread has used; `None` once it has exited.
    fn now_ns(self) -> Option<u64> {
        let mut ts = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: a valid timespec out pointer; an invalid clock (the
        // thread exited) is reported through the return value.
        let rc = unsafe { libc::clock_gettime(self.0, &mut ts) };
        (rc == 0).then(|| ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64)
    }
}

/// Watches a ready isolate until its thread reports an exit.
fn watch(inner: &PoolInner, worker: &WorkerShared, exit: &Receiver<WorkerExit>) -> WorkerExit {
    let limits = &inner.config.limits;
    let mut draining_since: Option<Instant> = None;
    // The heartbeat of the turn last reported as blocked.
    let mut blocked_turn: Option<u64> = None;
    loop {
        match exit.recv_timeout(WATCH_INTERVAL) {
            Ok(result) => return result,
            // Exited without reporting: the thread panicked.
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => {}
        }
        // A termination already requested is repeated until the thread
        // exits (see `interrupt`).
        worker.interrupt();
        // One reading of each: the turn may end between a second call and
        // the log, which then showed the stall as busy_ms=0.
        let busy = worker.busy_for();
        let cpu = worker.busy_cpu_for();
        if let Some(limit) = limits.stall_timeout
            && stalled(limit, busy, cpu)
            && worker.termination().is_none()
        {
            tracing::error!(
                target: "nextrs::executor",
                slot = worker.slot,
                busy_ms = busy.as_millis() as u64,
                cpu_ms = cpu.map(|c| c.as_millis() as u64),
                active = worker.active.load(Ordering::Relaxed),
                "JavaScript exceeded the stall timeout; terminating the isolate"
            );
            worker.terminate(Termination::Failed(FailureReason::CpuStall));
        } else if let Some(limit) = limits.stall_timeout
            && busy > limit
            && worker.termination().is_none()
            && blocked_turn != Some(worker.heartbeat_ms.load(Ordering::Relaxed))
        {
            // Long in one turn but not on the CPU: blocked (a synchronous
            // read on a slow disk, page faults under memory pressure, CPU
            // throttling). Reported once per turn, not terminated.
            blocked_turn = Some(worker.heartbeat_ms.load(Ordering::Relaxed));
            tracing::warn!(
                target: "nextrs::executor",
                slot = worker.slot,
                busy_ms = busy.as_millis() as u64,
                cpu_ms = cpu.map(|c| c.as_millis() as u64),
                "JavaScript has not yielded for longer than the stall timeout, but is not \
                 using the CPU (blocked or throttled); not terminating it"
            );
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
