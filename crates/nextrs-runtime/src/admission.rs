//! Memory-aware admission (RE-4B), optional: before a request is given an
//! isolate slot, the container's memory state decides whether it is
//! admitted, waits in the queue, or is refused with a retryable 503.
//!
//! The gate lives inside the pool's state and runs under the pool lock
//! that admission already takes, so its reservations are serialized: two
//! simultaneous requests cannot both spend the same headroom.
//!
//! * **One total.** The decision uses one measured total: the cgroup's
//!   working set (usage minus inactive page cache), or the process's
//!   resident anonymous memory when there is no cgroup and the operator
//!   gave a budget. V8 and allocator figures are already inside it and are
//!   never added to it.
//! * **On demand.** The usage is re-read when the reading is older than
//!   `sample_interval` (the page-cache figure every `stat_interval`), by
//!   whichever admission finds it stale. An idle server reads nothing.
//! * **Reservations.** Every admission adds its estimated cost to
//!   `reserved`, which a fresh reading resets (the memory of requests
//!   admitted before a reading has started to show up in it). Within one
//!   reading interval, a burst cannot spend more than the headroom that
//!   reading left.
//! * **States.** `normal`, `elevated` (from `pressure_percent`: a reduced
//!   concurrency cap, the rest wait), `critical` (from `critical_percent`:
//!   refused at once) and `degraded` (no successful reading for
//!   `stale_after`: treated as elevated, never as a stop). Moving up is
//!   immediate; moving down needs the level to fall `hysteresis_percent`
//!   below the threshold and `cooldown` in the current state.
//! * **Progress.** When no JavaScript request is active at all, one is
//!   always admitted, whatever the state: memory that is not held by
//!   requests is not released by refusing them, and refusing everything
//!   would turn a full container into an outage.
//!
//! None of this limits memory. A request admitted on an estimate can
//! allocate far more than it; the kernel still enforces the container's
//! limit. See `engineering/performance/resource-efficiency/RE-4-*`.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::memory::Cgroup;
use crate::{Invocation, RenderRequest, RequestBody};

tokio::task_local! {
    /// Set while Next.js's own requests to its server (`InternalRoute`)
    /// are handled: they belong to a request that was already admitted.
    static NESTED: ();
}

/// Runs `request` as part of an already-admitted request: its own
/// executor requests bypass the memory gate. Deferring or refusing them
/// would leave the parent waiting on a child queued behind it (or fail a
/// Server Action half way); the concurrency limits and
/// `MAX_INTERNAL_REQUESTS` still bound them.
pub(crate) async fn nested<F: std::future::Future>(request: F) -> F::Output {
    NESTED.scope((), request).await
}

/// Whether the current task handles a nested request (see [`nested`]).
pub(crate) fn is_nested() -> bool {
    NESTED.try_with(|_| ()).is_ok()
}

/// One memory reading from a [`MemoryProbe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryReading {
    /// The limit the probe found (cgroup); `None`: none or not read now.
    pub limit: Option<u64>,
    /// Memory in use, as the probe counts it (cgroup usage, or resident
    /// anonymous memory).
    pub usage: u64,
    /// Page cache the kernel reclaims first; `None`: not read this time.
    pub inactive_file: Option<u64>,
}

/// Where admission reads memory from. Implemented for the cgroup and the
/// process; tests supply their own. `read` is called with the executor's
/// pool lock held: it must be quick and must not call back into the
/// executor (`Executor::metrics` would deadlock).
pub trait MemoryProbe: Send + Sync {
    /// Reads memory in use. With `full`, also the slower figures (the
    /// limit and the inactive page cache); otherwise they may be `None`.
    /// `None`: the reading failed.
    fn read(&self, full: bool) -> Option<MemoryReading>;

    /// What is read, for logs.
    fn describe(&self) -> String;
}

/// The process's memory cgroup (v2 or v1).
pub struct CgroupProbe(pub Cgroup);

impl MemoryProbe for CgroupProbe {
    fn read(&self, full: bool) -> Option<MemoryReading> {
        if full {
            let memory = self.0.read()?;
            Some(MemoryReading {
                limit: memory.limit,
                usage: memory.usage,
                inactive_file: Some(memory.inactive_file),
            })
        } else {
            Some(MemoryReading {
                limit: None,
                usage: self.0.read_usage()?,
                inactive_file: None,
            })
        }
    }

    fn describe(&self) -> String {
        format!("cgroup {}", self.0.dir.display())
    }
}

/// The process's resident anonymous memory (`/proc/self/statm`: resident
/// minus shared pages), for hosts without a memory cgroup. It has no
/// limit of its own: an explicit budget is required.
pub struct ProcessProbe;

impl MemoryProbe for ProcessProbe {
    fn read(&self, _full: bool) -> Option<MemoryReading> {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let mut fields = statm.split_ascii_whitespace().skip(1);
        let resident: u64 = fields.next()?.parse().ok()?;
        let shared: u64 = fields.next()?.parse().ok()?;
        // SAFETY: sysconf has no preconditions.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        let page = u64::try_from(page).ok().filter(|p| *p > 0)?;
        Some(MemoryReading {
            limit: None,
            usage: resident.saturating_sub(shared).saturating_mul(page),
            inactive_file: Some(0),
        })
    }

    fn describe(&self) -> String {
        "process resident anonymous memory".into()
    }
}

/// Configuration of memory-aware admission ([`crate::Limits::memory_admission`]).
#[derive(Clone)]
pub struct MemoryAdmission {
    /// The memory budget. `None`: the effective cgroup limit (lowest
    /// `memory.max`/`memory.high` up the hierarchy). With both, the lower.
    /// Without either, admission is off (warned at startup).
    pub budget_bytes: Option<u64>,
    /// Working set (plus reservations) at which admission caps concurrency.
    pub pressure_percent: f64,
    /// At which new requests are refused (503) while others run.
    pub critical_percent: f64,
    /// How far below a threshold the level must fall to leave its state.
    pub hysteresis_percent: f64,
    /// Least time in a state before moving down.
    pub cooldown: Duration,
    /// Estimated memory per admitted request (see [`Ticket`]).
    pub request_reserve_bytes: u64,
    /// Active JavaScript requests allowed, over all isolates, while
    /// elevated or degraded. `None`: 4 per isolate.
    pub pressure_max_active: Option<usize>,
    /// Skip an isolate with requests in flight whose sampled heap use is
    /// at least this percentage of its heap limit. `None`: never skip.
    pub heap_percent: Option<f64>,
    /// Header and URL bytes queued requests may hold, in total.
    pub max_queued_bytes: usize,
    /// Longest a usage reading is used before it is re-read.
    pub sample_interval: Duration,
    /// Longest the inactive-page-cache and limit figures are used.
    pub stat_interval: Duration,
    /// Without a successful reading for this long, admission is degraded.
    pub stale_after: Duration,
    /// Where to read memory. `None`: the cgroup, else (with a budget) the
    /// process.
    pub probe: Option<Arc<dyn MemoryProbe>>,
}

impl Default for MemoryAdmission {
    fn default() -> Self {
        Self {
            budget_bytes: None,
            pressure_percent: 85.0,
            critical_percent: 95.0,
            hysteresis_percent: 5.0,
            cooldown: Duration::from_secs(1),
            request_reserve_bytes: 2 * 1024 * 1024,
            pressure_max_active: None,
            heap_percent: Some(90.0),
            max_queued_bytes: 16 * 1024 * 1024,
            sample_interval: Duration::from_millis(50),
            stat_interval: Duration::from_millis(500),
            stale_after: Duration::from_secs(2),
            probe: None,
        }
    }
}

impl fmt::Debug for MemoryAdmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryAdmission")
            .field("budget_bytes", &self.budget_bytes)
            .field("pressure_percent", &self.pressure_percent)
            .field("critical_percent", &self.critical_percent)
            .field("hysteresis_percent", &self.hysteresis_percent)
            .field("cooldown", &self.cooldown)
            .field("request_reserve_bytes", &self.request_reserve_bytes)
            .field("pressure_max_active", &self.pressure_max_active)
            .field("heap_percent", &self.heap_percent)
            .field("max_queued_bytes", &self.max_queued_bytes)
            .field("probe", &self.probe.as_ref().map(|p| p.describe()))
            .finish_non_exhaustive()
    }
}

impl MemoryAdmission {
    /// Checks the percentages: 0 < pressure ≤ critical ≤ 100, hysteresis
    /// in [0, pressure), heap in (0, 100].
    pub fn validate(&self) -> Result<(), String> {
        let ok = |p: f64| p.is_finite() && p > 0.0 && p <= 100.0;
        if !ok(self.pressure_percent) || !ok(self.critical_percent) {
            return Err(
                "memory admission thresholds must be percentages above 0, at most 100".into(),
            );
        }
        if self.pressure_percent > self.critical_percent {
            return Err(format!(
                "memory admission: the pressure threshold ({}) is above the critical one ({})",
                self.pressure_percent, self.critical_percent
            ));
        }
        if !self.hysteresis_percent.is_finite()
            || self.hysteresis_percent < 0.0
            || self.hysteresis_percent >= self.pressure_percent
        {
            return Err(
                "memory admission hysteresis must be at least 0 and below the pressure threshold"
                    .into(),
            );
        }
        if let Some(heap) = self.heap_percent
            && !ok(heap)
        {
            return Err(
                "memory admission heap threshold must be a percentage above 0, at most 100".into(),
            );
        }
        Ok(())
    }
}

/// What kind of request is asking: decides its reserve and the metrics
/// label. Derived from the request alone, never from route configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestClass {
    /// SSR, RSC, route handlers and Pages Router pages.
    Render,
    /// A Server Action (`POST` with `next-action`).
    ServerAction,
    /// A request with a large or unknown-length body.
    Upload,
    /// The compiled proxy (middleware).
    Proxy,
}

impl RequestClass {
    pub(crate) const ALL: [Self; 4] = [Self::Render, Self::ServerAction, Self::Upload, Self::Proxy];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::ServerAction => "server_action",
            Self::Upload => "upload",
            Self::Proxy => "proxy",
        }
    }

    pub(crate) fn index(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }
}

/// A body larger than this (declared) makes a request an upload.
const UPLOAD_THRESHOLD: u64 = 64 * 1024;
/// Fixed cost of one queued request beyond its headers and URL.
const QUEUED_OVERHEAD: usize = 1024;

/// A request's admission costs: its class, the memory it is estimated to
/// need (reserved against the headroom), and what it holds while queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ticket {
    pub class: RequestClass,
    pub reserve: u64,
    pub queued_bytes: usize,
    /// Part of an admitted request ([`nested`]): never gated.
    pub nested: bool,
}

impl Ticket {
    pub fn for_request(request: &RenderRequest, reserve: u64) -> Self {
        let header = |name: &str| {
            request
                .headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        let queued_bytes = request
            .headers
            .iter()
            .map(|(n, v)| n.len() + v.len())
            .sum::<usize>()
            .saturating_add(request.url.len())
            .saturating_add(QUEUED_OVERHEAD);
        let has_body = !matches!(request.body, RequestBody::Empty);
        let declared = header("content-length").and_then(|v| v.trim().parse::<u64>().ok());
        let chunked = header("transfer-encoding").is_some_and(|v| {
            v.split(',')
                .any(|t| t.trim().eq_ignore_ascii_case("chunked"))
        });
        let body_reserve = match (has_body, declared) {
            (false, _) => 0,
            (true, Some(length)) if length > UPLOAD_THRESHOLD => length,
            (true, Some(_)) => 0,
            (true, None) if chunked => reserve,
            (true, None) => 0,
        };
        let action = request.method.eq_ignore_ascii_case("POST") && header("next-action").is_some();
        let class = if matches!(request.invocation, Invocation::Proxy { .. }) {
            RequestClass::Proxy
        } else if action {
            RequestClass::ServerAction
        } else if body_reserve > 0 {
            RequestClass::Upload
        } else {
            RequestClass::Render
        };
        let base = if class == RequestClass::Proxy {
            reserve / 2
        } else {
            reserve
        };
        Self {
            class,
            reserve: base.saturating_add(body_reserve),
            queued_bytes,
            nested: is_nested(),
        }
    }
}

/// Admission state, from the latest reading and the reservations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryState {
    Normal,
    Elevated,
    Critical,
    /// No recent reading: treated as elevated.
    Degraded,
}

impl MemoryState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Elevated => "elevated",
            Self::Critical => "critical",
            Self::Degraded => "degraded",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Elevated | Self::Degraded => 1,
            Self::Critical => 2,
        }
    }

    fn from_rank(rank: u8) -> Self {
        match rank {
            0 => Self::Normal,
            1 => Self::Elevated,
            _ => Self::Critical,
        }
    }
}

/// What the gate decided for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Admit (if an isolate slot is free); reserve its cost.
    Admit,
    /// Admit regardless of state: nothing else is running.
    Progress,
    /// Wait in the queue for capacity or memory.
    Defer,
    /// Refuse with `RenderError::MemoryPressure`.
    Refuse,
}

/// The gate's state. Owned by the pool state, used under its lock.
pub(crate) struct Gate {
    config: MemoryAdmission,
    probe: Arc<dyn MemoryProbe>,
    isolates: usize,
    /// The budget from the latest full reading (configured and found).
    budget: Option<u64>,
    usage: u64,
    inactive_file: u64,
    read_at: Option<Instant>,
    stat_at: Option<Instant>,
    /// The latest read attempt, successful or not: a failing probe is not
    /// retried more often than `sample_interval` (it runs under the pool
    /// lock).
    attempt_at: Option<Instant>,
    /// Estimated cost of requests admitted since `read_at`.
    reserved: u64,
    state: MemoryState,
    since: Instant,
    pub transitions: u64,
    pub sample_failures: u64,
}

impl Gate {
    /// Builds the gate, with a first reading. `Err`: there is nothing to
    /// compare against (no budget and no limit), with the reason.
    pub fn new(config: MemoryAdmission, isolates: usize) -> Result<Self, String> {
        config.validate()?;
        let probe: Arc<dyn MemoryProbe> = match &config.probe {
            Some(probe) => probe.clone(),
            None => match Cgroup::locate() {
                Some(cgroup) => Arc::new(CgroupProbe(cgroup)),
                None if config.budget_bytes.is_some() => Arc::new(ProcessProbe),
                None => {
                    return Err(
                        "no memory cgroup was found and no memory admission budget was \
                                given (--memory-admission-budget)"
                            .into(),
                    );
                }
            },
        };
        let now = Instant::now();
        let mut gate = Self {
            config,
            probe,
            isolates: isolates.max(1),
            budget: None,
            usage: 0,
            inactive_file: 0,
            read_at: None,
            stat_at: None,
            attempt_at: None,
            reserved: 0,
            state: MemoryState::Degraded,
            since: now,
            transitions: 0,
            sample_failures: 0,
        };
        gate.refresh(now);
        if gate.budget.is_none() {
            return Err(format!(
                "{} has no memory limit and no memory admission budget was given \
                 (--memory-admission-budget)",
                gate.probe.describe()
            ));
        }
        gate.update(now);
        Ok(gate)
    }

    pub fn probe_description(&self) -> String {
        self.probe.describe()
    }

    pub fn config(&self) -> &MemoryAdmission {
        &self.config
    }

    fn refresh(&mut self, now: Instant) {
        if self
            .attempt_at
            .is_some_and(|at| now.duration_since(at) < self.config.sample_interval)
        {
            return;
        }
        self.attempt_at = Some(now);
        let full = self
            .stat_at
            .is_none_or(|at| now.duration_since(at) >= self.config.stat_interval);
        match self.probe.read(full) {
            Some(reading) => {
                self.usage = reading.usage;
                if let Some(inactive) = reading.inactive_file {
                    self.inactive_file = inactive;
                }
                if full {
                    self.stat_at = Some(now);
                    self.budget = match (self.config.budget_bytes, reading.limit) {
                        (Some(b), Some(l)) => Some(b.min(l)),
                        (b, l) => b.or(l),
                    };
                }
                self.read_at = Some(now);
                self.reserved = 0;
            }
            None => self.sample_failures += 1,
        }
    }

    /// The measured total: usage minus inactive page cache. The page
    /// cache figure may be older than the usage; it is capped at it.
    pub fn working_set(&self) -> u64 {
        self.usage
            .saturating_sub(self.inactive_file.min(self.usage))
    }

    fn effective(&self) -> u64 {
        self.working_set().saturating_add(self.reserved)
    }

    fn threshold(&self, percent: f64) -> u64 {
        let budget = self.budget.unwrap_or(u64::MAX) as f64;
        (budget * percent / 100.0) as u64
    }

    fn rank_at(&self, level: u64, offset: f64) -> u8 {
        let critical = self.threshold((self.config.critical_percent - offset).max(0.0));
        let pressure = self.threshold((self.config.pressure_percent - offset).max(0.0));
        if level >= critical {
            2
        } else if level >= pressure {
            1
        } else {
            0
        }
    }

    fn update(&mut self, now: Instant) {
        let stale = self
            .read_at
            .is_none_or(|at| now.duration_since(at) >= self.config.stale_after);
        let next = if stale || self.budget.is_none() {
            MemoryState::Degraded
        } else {
            let level = self.effective();
            let up = self.rank_at(level, 0.0);
            let current = self.state.rank();
            if self.state == MemoryState::Degraded {
                // A fresh reading after a gap: take it as it is.
                MemoryState::from_rank(up)
            } else if up > current {
                MemoryState::from_rank(up)
            } else if now.duration_since(self.since) >= self.config.cooldown {
                let down = self.rank_at(level, self.config.hysteresis_percent);
                MemoryState::from_rank(down.min(current))
            } else {
                self.state
            }
        };
        if next != self.state {
            tracing::info!(
                target: "nextrs::admission",
                from = self.state.as_str(),
                to = next.as_str(),
                working_set_mib = self.working_set() / (1024 * 1024),
                reserved_mib = self.reserved / (1024 * 1024),
                budget_mib = self.budget.map(|b| b / (1024 * 1024)),
                "memory admission state changed"
            );
            self.state = next;
            self.since = now;
            self.transitions += 1;
        }
    }

    fn pressure_cap(&self) -> usize {
        self.config
            .pressure_max_active
            .unwrap_or(4 * self.isolates)
            .max(1)
    }

    /// Decides for a request with `reserve`, with `active` JavaScript
    /// requests running over all isolates.
    pub fn decide(&mut self, now: Instant, reserve: u64, active: usize) -> Decision {
        self.refresh(now);
        self.update(now);
        if active == 0 {
            return if self.state == MemoryState::Normal {
                Decision::Admit
            } else {
                Decision::Progress
            };
        }
        let fits = self.effective().saturating_add(reserve)
            <= self.threshold(self.config.critical_percent);
        match self.state {
            MemoryState::Normal if fits => Decision::Admit,
            MemoryState::Normal => Decision::Defer,
            MemoryState::Elevated if fits && active < self.pressure_cap() => Decision::Admit,
            MemoryState::Elevated => Decision::Defer,
            MemoryState::Degraded if active < self.pressure_cap() => Decision::Admit,
            MemoryState::Degraded => Decision::Defer,
            MemoryState::Critical => Decision::Refuse,
        }
    }

    /// Records an admission made after `decide` said yes.
    pub fn admitted(&mut self, reserve: u64) {
        self.reserved = self.reserved.saturating_add(reserve);
    }

    pub fn state(&self) -> MemoryState {
        self.state
    }

    pub fn budget(&self) -> Option<u64> {
        self.budget
    }

    pub fn reserved(&self) -> u64 {
        self.reserved
    }

    pub fn reading_age(&self, now: Instant) -> Option<Duration> {
        self.read_at.map(|at| now.duration_since(at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const MIB: u64 = 1024 * 1024;

    /// A probe whose reading the test sets; `None` makes reads fail.
    struct Fake {
        reading: Mutex<Option<MemoryReading>>,
        reads: Mutex<(u64, u64)>,
    }

    impl Fake {
        fn new(limit: u64, usage: u64) -> Arc<Self> {
            Arc::new(Self {
                reading: Mutex::new(Some(MemoryReading {
                    limit: Some(limit),
                    usage,
                    inactive_file: Some(0),
                })),
                reads: Mutex::new((0, 0)),
            })
        }

        fn set_usage(&self, usage: u64) {
            if let Some(r) = self.reading.lock().unwrap().as_mut() {
                r.usage = usage;
            }
        }

        fn fail(&self) {
            *self.reading.lock().unwrap() = None;
        }
    }

    impl MemoryProbe for Fake {
        fn read(&self, full: bool) -> Option<MemoryReading> {
            let mut reads = self.reads.lock().unwrap();
            if full {
                reads.1 += 1;
            } else {
                reads.0 += 1;
            }
            let reading = (*self.reading.lock().unwrap())?;
            Some(if full {
                reading
            } else {
                MemoryReading {
                    limit: None,
                    inactive_file: None,
                    ..reading
                }
            })
        }

        fn describe(&self) -> String {
            "fake".into()
        }
    }

    fn config(probe: Arc<Fake>) -> MemoryAdmission {
        MemoryAdmission {
            // Every decision re-reads, unless a test says otherwise.
            sample_interval: Duration::ZERO,
            stat_interval: Duration::ZERO,
            cooldown: Duration::ZERO,
            probe: Some(probe),
            ..MemoryAdmission::default()
        }
    }

    #[test]
    fn thresholds_follow_the_working_set() {
        let probe = Fake::new(100 * MIB, 50 * MIB);
        let mut gate = Gate::new(config(probe.clone()), 1).unwrap();
        let now = Instant::now();
        assert_eq!(gate.decide(now, MIB, 1), Decision::Admit);
        assert_eq!(gate.state(), MemoryState::Normal);
        probe.set_usage(86 * MIB);
        assert_eq!(gate.decide(now, MIB, 1), Decision::Admit);
        assert_eq!(gate.state(), MemoryState::Elevated);
        probe.set_usage(96 * MIB);
        assert_eq!(gate.decide(now, MIB, 1), Decision::Refuse);
        assert_eq!(gate.state(), MemoryState::Critical);
    }

    #[test]
    fn inactive_page_cache_is_not_counted() {
        let probe = Fake::new(100 * MIB, 99 * MIB);
        probe
            .reading
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .inactive_file = Some(40 * MIB);
        let gate = Gate::new(config(probe), 1).unwrap();
        assert_eq!(gate.working_set(), 59 * MIB);
        assert_eq!(gate.state(), MemoryState::Normal);
    }

    #[test]
    fn a_configured_budget_below_the_limit_wins() {
        let probe = Fake::new(1000 * MIB, 90 * MIB);
        let mut cfg = config(probe);
        cfg.budget_bytes = Some(100 * MIB);
        let gate = Gate::new(cfg, 1).unwrap();
        assert_eq!(gate.budget(), Some(100 * MIB));
        assert_eq!(gate.state(), MemoryState::Elevated);
    }

    #[test]
    fn no_limit_and_no_budget_is_refused_at_construction() {
        let probe = Fake::new(0, 0);
        probe.reading.lock().unwrap().as_mut().unwrap().limit = None;
        assert!(Gate::new(config(probe), 1).is_err());
    }

    #[test]
    fn a_burst_cannot_spend_the_same_headroom_twice() {
        // One reading for the whole burst: reservations alone decide.
        let probe = Fake::new(100 * MIB, 80 * MIB);
        let mut cfg = config(probe);
        cfg.sample_interval = Duration::from_secs(3600);
        cfg.stat_interval = Duration::from_secs(3600);
        cfg.stale_after = Duration::from_secs(7200);
        cfg.pressure_max_active = Some(1000);
        let mut gate = Gate::new(cfg, 1).unwrap();
        let now = Instant::now();
        let mut admitted = 0;
        for active in 1..100 {
            match gate.decide(now, 2 * MIB, active) {
                Decision::Admit => {
                    gate.admitted(2 * MIB);
                    admitted += 1;
                }
                Decision::Defer | Decision::Refuse => break,
                Decision::Progress => unreachable!(),
            }
        }
        // 80 MiB used, critical at 95 MiB: at most 7 reserves of 2 MiB.
        assert_eq!(admitted, 7);
        assert_eq!(gate.reserved(), 14 * MIB);
    }

    #[test]
    fn a_fresh_reading_resets_reservations() {
        let probe = Fake::new(100 * MIB, 10 * MIB);
        let mut gate = Gate::new(config(probe), 1).unwrap();
        gate.admitted(50 * MIB);
        assert_eq!(gate.reserved(), 50 * MIB);
        gate.decide(Instant::now(), MIB, 1);
        assert_eq!(gate.reserved(), 0);
    }

    #[test]
    fn elevated_caps_concurrency_and_defers_the_rest() {
        let probe = Fake::new(100 * MIB, 88 * MIB);
        let mut cfg = config(probe);
        cfg.pressure_max_active = Some(3);
        let mut gate = Gate::new(cfg, 1).unwrap();
        let now = Instant::now();
        assert_eq!(gate.decide(now, 0, 2), Decision::Admit);
        assert_eq!(gate.decide(now, 0, 3), Decision::Defer);
    }

    #[test]
    fn nothing_running_always_admits_one() {
        let probe = Fake::new(100 * MIB, 99 * MIB);
        let mut gate = Gate::new(config(probe), 1).unwrap();
        assert_eq!(gate.state(), MemoryState::Critical);
        assert_eq!(gate.decide(Instant::now(), MIB, 0), Decision::Progress);
        assert_eq!(gate.decide(Instant::now(), MIB, 1), Decision::Refuse);
    }

    #[test]
    fn leaving_a_state_needs_hysteresis_and_cooldown() {
        let probe = Fake::new(100 * MIB, 96 * MIB);
        let mut cfg = config(probe.clone());
        cfg.cooldown = Duration::from_secs(1);
        let mut gate = Gate::new(cfg, 1).unwrap();
        let start = Instant::now();
        assert_eq!(gate.state(), MemoryState::Critical);
        // Just below the threshold: within the hysteresis band.
        probe.set_usage(93 * MIB);
        gate.decide(start + Duration::from_secs(2), 0, 1);
        assert_eq!(gate.state(), MemoryState::Critical);
        // Below the band, but before the cooldown: stays.
        probe.set_usage(70 * MIB);
        let entered = gate.since;
        gate.decide(entered + Duration::from_millis(500), 0, 1);
        assert_eq!(gate.state(), MemoryState::Critical);
        // After the cooldown: straight down to normal.
        gate.decide(entered + Duration::from_millis(1500), 0, 1);
        assert_eq!(gate.state(), MemoryState::Normal);
        // Moving up is immediate.
        probe.set_usage(97 * MIB);
        gate.decide(gate.since, 0, 1);
        assert_eq!(gate.state(), MemoryState::Critical);
    }

    #[test]
    fn failed_readings_degrade_after_the_stale_limit_and_recover() {
        let probe = Fake::new(100 * MIB, 10 * MIB);
        let mut cfg = config(probe.clone());
        cfg.stale_after = Duration::from_secs(2);
        cfg.pressure_max_active = Some(2);
        let mut gate = Gate::new(cfg, 1).unwrap();
        let start = Instant::now();
        probe.fail();
        assert_eq!(
            gate.decide(start + Duration::from_secs(1), 0, 1),
            Decision::Admit
        );
        assert_eq!(gate.state(), MemoryState::Normal);
        assert_eq!(
            gate.decide(start + Duration::from_secs(3), 0, 1),
            Decision::Admit
        );
        assert_eq!(gate.state(), MemoryState::Degraded);
        // Degraded caps concurrency, but never stops.
        assert_eq!(
            gate.decide(start + Duration::from_secs(3), 0, 2),
            Decision::Defer
        );
        assert_eq!(
            gate.decide(start + Duration::from_secs(3), 0, 0),
            Decision::Progress
        );
        assert!(gate.sample_failures >= 3);
        probe.set_usage(10 * MIB);
        *probe.reading.lock().unwrap() = Some(MemoryReading {
            limit: Some(100 * MIB),
            usage: 10 * MIB,
            inactive_file: Some(0),
        });
        gate.decide(start + Duration::from_secs(4), 0, 1);
        assert_eq!(gate.state(), MemoryState::Normal);
    }

    #[test]
    fn usage_is_re_read_on_demand_and_stats_less_often() {
        let probe = Fake::new(100 * MIB, 10 * MIB);
        let mut cfg = config(probe.clone());
        // Wide intervals: construction may lag `start` by a scheduling delay.
        cfg.sample_interval = Duration::from_secs(5);
        cfg.stat_interval = Duration::from_secs(50);
        cfg.stale_after = Duration::from_secs(500);
        let mut gate = Gate::new(cfg, 1).unwrap();
        let start = Instant::now();
        for s in [1, 2, 6, 7, 12, 60] {
            gate.decide(start + Duration::from_secs(s), 0, 1);
        }
        let (fast, full) = *probe.reads.lock().unwrap();
        // Construction (full), 6 and 12 (fast), 60 (full).
        assert_eq!((fast, full), (2, 2));
    }

    #[test]
    fn tickets_classify_without_route_configuration() {
        let request = |method: &str, headers: &[(&str, &str)], body: RequestBody| RenderRequest {
            method: method.into(),
            url: "/x".into(),
            headers: headers
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect(),
            body,
            entry: "/e.js".into(),
            remote_address: None,
            initial_headers: Vec::new(),
            initial_status: None,
            invocation: Invocation::Handler,
            init_url: None,
        };
        let r = 2 * MIB;
        let get = Ticket::for_request(&request("GET", &[], RequestBody::Empty), r);
        assert_eq!((get.class, get.reserve), (RequestClass::Render, r));
        assert!(get.queued_bytes >= QUEUED_OVERHEAD + 2);
        let action = Ticket::for_request(
            &request(
                "POST",
                &[("Next-Action", "abc"), ("content-length", "10")],
                RequestBody::Full(bytes::Bytes::from_static(b"0123456789")),
            ),
            r,
        );
        assert_eq!(
            (action.class, action.reserve),
            (RequestClass::ServerAction, r)
        );
        let upload = Ticket::for_request(
            &request(
                "PUT",
                &[("content-length", "5000000")],
                RequestBody::Full(bytes::Bytes::new()),
            ),
            r,
        );
        assert_eq!(
            (upload.class, upload.reserve),
            (RequestClass::Upload, r + 5_000_000)
        );
        let chunked = Ticket::for_request(
            &request(
                "POST",
                &[("transfer-encoding", "gzip, chunked")],
                RequestBody::Full(bytes::Bytes::new()),
            ),
            r,
        );
        assert_eq!(
            (chunked.class, chunked.reserve),
            (RequestClass::Upload, 2 * r)
        );
        let mut proxy = request("GET", &[], RequestBody::Empty);
        proxy.invocation = Invocation::Proxy {
            url: "http://localhost/x".into(),
        };
        let proxy = Ticket::for_request(&proxy, r);
        assert_eq!((proxy.class, proxy.reserve), (RequestClass::Proxy, r / 2));
    }

    #[tokio::test]
    async fn requests_inside_a_nested_scope_are_marked_and_only_there() {
        let request = || RenderRequest {
            method: "GET".into(),
            url: "/x".into(),
            headers: Vec::new(),
            body: RequestBody::Empty,
            entry: "/e.js".into(),
            remote_address: None,
            initial_headers: Vec::new(),
            initial_status: None,
            invocation: Invocation::Handler,
            init_url: None,
        };
        assert!(!Ticket::for_request(&request(), MIB).nested);
        let inside = nested(async { Ticket::for_request(&request(), MIB).nested }).await;
        assert!(inside);
        // A task spawned from inside does not inherit it.
        let spawned = nested(async {
            tokio::spawn(async move { Ticket::for_request(&request(), MIB).nested })
                .await
                .unwrap()
        })
        .await;
        assert!(!spawned);
        assert!(!Ticket::for_request(&request(), MIB).nested);
    }

    #[test]
    fn invalid_thresholds_are_rejected() {
        let bad = |f: fn(&mut MemoryAdmission)| {
            let mut c = MemoryAdmission::default();
            f(&mut c);
            c.validate().is_err()
        };
        assert!(bad(|c| c.pressure_percent = 0.0));
        assert!(bad(|c| c.critical_percent = 101.0));
        assert!(bad(|c| c.pressure_percent = 96.0));
        assert!(bad(|c| c.hysteresis_percent = 90.0));
        assert!(bad(|c| c.heap_percent = Some(f64::NAN)));
        assert!(MemoryAdmission::default().validate().is_ok());
    }
}
