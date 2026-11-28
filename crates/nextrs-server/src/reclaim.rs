//! Memory reclamation: gives memory the process no longer uses back to the
//! system after load, and under container memory pressure.
//!
//! Two sources of retained memory dominate after load (RE-1 baseline):
//! V8 heaps that grew for the load and stay committed, and free memory the
//! native allocator (glibc) keeps in its arenas. Neither is a leak, and both
//! are reused by the next load; on a memory-limited container they are what
//! stands between the steady state and the limit.
//!
//! * **Idle:** once the server has been quiet for `idle_after`, each
//!   isolate runs V8's low-memory collection and the allocator's free pages
//!   are released (`malloc_trim`). Quiet, not silent: health checks and
//!   other background probes keep arriving on an idle server (Docker's
//!   `HEALTHCHECK`, Kubernetes liveness and readiness probes, load
//!   balancers), so up to about one request per 3 s (at least 3 per
//!   window) still counts as quiet. Once per idle period: only traffic
//!   above twice that re-arms it, so probes, even jittery ones, never
//!   cause repeated collections.
//! * **Pressure:** when the container's working set (usage minus inactive
//!   page cache) reaches `pressure_percent` of its memory limit, the same
//!   happens with V8's critical memory-pressure response, even under load,
//!   at most once per `pressure_interval`. When a reclamation leaves the
//!   working set above the threshold (live data, not garbage, fills the
//!   container), the interval doubles, up to 8 times, so a container that
//!   simply runs near its limit is not collected over and over; it resets
//!   once the working set drops below the threshold.
//!
//! * **Admission** (RE-4.1C): with memory-aware admission's emergency
//!   state configured (`--memory-admission-emergency`), when admission
//!   moves up into its critical or emergency state, a pressure reclamation
//!   runs at the next tick instead of waiting for the backed-off interval,
//!   though never
//!   within `pressure_interval` of the previous one. Admission is refusing
//!   requests then; garbage the isolates hold is the memory it can get
//!   back without running any.
//!
//! Neither is a memory limit: the kernel's OOM killer still applies at the
//! container's limit. Requests are never refused or delayed by this, except
//! by the collection itself (requests in an isolate wait for its GC, as
//! for any full collection).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use nextrs_runtime::{Cgroup, Executor, ReclaimLevel};

use crate::HttpMetrics;

/// How often the reclaimer looks at activity and memory.
const TICK: Duration = Duration::from_secs(1);
/// Largest multiple of `pressure_interval` between pressure reclamations
/// that do not bring the working set below the threshold.
const MAX_PRESSURE_BACKOFF: u32 = 8;
/// Background request rate an idle server may still see (probes): a
/// liveness and a readiness probe every 10 s each is 0.2 per second.
const QUIET_REQUESTS_PER_SECOND: f64 = 0.3;

/// Requests an `idle_after` window may hold and still be quiet.
fn quiet_allowance(idle_after: Duration) -> u64 {
    // Saturating float-to-int conversion; windows are at most a day.
    let allowed = (idle_after.as_secs_f64() * QUIET_REQUESTS_PER_SECOND).ceil() as u64;
    allowed.max(3)
}

/// When to reclaim memory. Both triggers are off by default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReclaimPolicy {
    /// Reclaim after this long without requests; `None` disables it.
    pub idle_after: Option<Duration>,
    /// Reclaim when the container's working set reaches this percentage of
    /// its memory limit (no effect without a limit); `None` disables it.
    pub pressure_percent: Option<f64>,
    /// Least time between two pressure reclamations.
    pub pressure_interval: Duration,
}

impl Default for ReclaimPolicy {
    fn default() -> Self {
        Self {
            idle_after: None,
            pressure_percent: None,
            pressure_interval: Duration::from_secs(10),
        }
    }
}

impl ReclaimPolicy {
    pub fn is_enabled(&self) -> bool {
        self.idle_after.is_some() || self.pressure_percent.is_some()
    }
}

/// What the reclaimer did, for `/metrics`.
#[derive(Debug, Default)]
pub struct ReclaimStats {
    pub idle: AtomicU64,
    pub pressure: AtomicU64,
    /// Total time spent in reclamations (V8 collections and the trim).
    pub micros: AtomicU64,
    /// Resident anonymous memory before the latest reclamation, after its
    /// V8 collections, and after the native allocator's trim: the two
    /// steps' effects, apart.
    pub last_anon_before: AtomicU64,
    pub last_anon_after_gc: AtomicU64,
    pub last_anon_after: AtomicU64,
    /// Reclamations that ran with the pressure interval backed off.
    pub pressure_backoffs: AtomicU64,
    /// Pressure reclamations that admission asked for (also counted in
    /// `pressure`).
    pub admission_requested: AtomicU64,
    /// The container's memory limit and working set at the latest tick
    /// that read them (0: unknown or no limit).
    pub cgroup_limit: AtomicU64,
    pub cgroup_working_set: AtomicU64,
}

impl ReclaimStats {
    /// Prometheus text format.
    pub fn to_prometheus(&self) -> String {
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        format!(
            "# HELP nextrs_memory_reclaims_total Memory reclamations (V8 collections and native allocator trims), by trigger.\n\
             # TYPE nextrs_memory_reclaims_total counter\n\
             nextrs_memory_reclaims_total{{trigger=\"idle\"}} {}\n\
             nextrs_memory_reclaims_total{{trigger=\"pressure\"}} {}\n\
             # HELP nextrs_memory_reclaim_seconds_total Time spent reclaiming memory.\n\
             # TYPE nextrs_memory_reclaim_seconds_total counter\n\
             nextrs_memory_reclaim_seconds_total {:.6}\n\
             # HELP nextrs_memory_reclaim_last_anon_before_bytes Resident anonymous memory before the latest reclamation.\n\
             # TYPE nextrs_memory_reclaim_last_anon_before_bytes gauge\n\
             nextrs_memory_reclaim_last_anon_before_bytes {}\n\
             # HELP nextrs_memory_reclaim_last_anon_after_gc_bytes Resident anonymous memory after the latest reclamation's V8 collections, before the allocator trim.\n\
             # TYPE nextrs_memory_reclaim_last_anon_after_gc_bytes gauge\n\
             nextrs_memory_reclaim_last_anon_after_gc_bytes {}\n\
             # HELP nextrs_memory_reclaim_last_anon_after_bytes Resident anonymous memory after the latest reclamation.\n\
             # TYPE nextrs_memory_reclaim_last_anon_after_bytes gauge\n\
             nextrs_memory_reclaim_last_anon_after_bytes {}\n\
             # HELP nextrs_memory_reclaim_pressure_backoffs_total Pressure reclamations that found the working set still above the threshold after the previous one.\n\
             # TYPE nextrs_memory_reclaim_pressure_backoffs_total counter\n\
             nextrs_memory_reclaim_pressure_backoffs_total {}\n\
             # HELP nextrs_memory_reclaim_admission_requested_total Pressure reclamations run early because memory admission became critical.\n\
             # TYPE nextrs_memory_reclaim_admission_requested_total counter\n\
             nextrs_memory_reclaim_admission_requested_total {}\n\
             # HELP nextrs_cgroup_memory_limit_bytes The container's memory limit (0: none or unknown).\n\
             # TYPE nextrs_cgroup_memory_limit_bytes gauge\n\
             nextrs_cgroup_memory_limit_bytes {}\n\
             # HELP nextrs_cgroup_memory_working_set_bytes The container's memory usage minus inactive page cache.\n\
             # TYPE nextrs_cgroup_memory_working_set_bytes gauge\n\
             nextrs_cgroup_memory_working_set_bytes {}\n",
            get(&self.idle),
            get(&self.pressure),
            get(&self.micros) as f64 / 1e6,
            get(&self.last_anon_before),
            get(&self.last_anon_after_gc),
            get(&self.last_anon_after),
            get(&self.pressure_backoffs),
            get(&self.admission_requested),
            get(&self.cgroup_limit),
            get(&self.cgroup_working_set),
        )
    }

    pub fn to_json(&self) -> serde_json::Value {
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        serde_json::json!({
            "reclaims_idle": get(&self.idle),
            "reclaims_pressure": get(&self.pressure),
            "reclaim_seconds_total": get(&self.micros) as f64 / 1e6,
            "last_anon_before_bytes": get(&self.last_anon_before),
            "last_anon_after_gc_bytes": get(&self.last_anon_after_gc),
            "last_anon_after_bytes": get(&self.last_anon_after),
            "pressure_backoffs": get(&self.pressure_backoffs),
            "admission_requested": get(&self.admission_requested),
            "cgroup_limit_bytes": get(&self.cgroup_limit),
            "cgroup_working_set_bytes": get(&self.cgroup_working_set),
        })
    }
}

/// The reclaimer's decisions, separate from the clock and the system so
/// they can be tested.
#[derive(Debug)]
struct Tracker {
    policy: ReclaimPolicy,
    started: Instant,
    requests_seen: u64,
    /// Requests that arrived per tick within the last `idle_after`
    /// (ticks with none are not stored): at most one entry per tick.
    recent: std::collections::VecDeque<(Instant, u64)>,
    /// Traffic above the quiet rate was seen since the last idle
    /// reclamation. Starting counts: loading the build leaves garbage and
    /// freed allocator memory behind.
    armed: bool,
    last_pressure: Option<Instant>,
    /// Multiple of `pressure_interval` before the next pressure
    /// reclamation: 1, doubled while reclamations do not help.
    pressure_backoff: u32,
    /// A pressure reclamation ran and the working set has not dropped
    /// below the threshold since: the next one doubles the backoff.
    pressure_unhelpful: bool,
}

impl Tracker {
    fn new(policy: ReclaimPolicy, now: Instant) -> Self {
        Self {
            policy,
            started: now,
            requests_seen: 0,
            recent: std::collections::VecDeque::new(),
            armed: true,
            last_pressure: None,
            pressure_backoff: 1,
            pressure_unhelpful: false,
        }
    }

    /// `requests` is a counter of requests received (counted on arrival);
    /// `busy`: a request is in flight or waiting. Returns the reclamation
    /// to perform now.
    fn tick(
        &mut self,
        now: Instant,
        requests: u64,
        busy: bool,
        memory: Option<(u64, u64)>,
    ) -> Option<ReclaimLevel> {
        let arrived = requests.saturating_sub(self.requests_seen);
        self.requests_seen = requests;
        let quiet = match self.policy.idle_after {
            Some(idle_after) => {
                if arrived > 0 {
                    self.recent.push_back((now, arrived));
                }
                while self
                    .recent
                    .front()
                    .is_some_and(|(at, _)| now.duration_since(*at) >= idle_after)
                {
                    self.recent.pop_front();
                }
                let in_window = self
                    .recent
                    .iter()
                    .fold(0u64, |sum, (_, n)| sum.saturating_add(*n));
                let allowance = quiet_allowance(idle_after);
                // Hysteresis: re-armed only well above the quiet rate.
                if in_window > allowance.saturating_mul(2) {
                    self.armed = true;
                }
                in_window <= allowance && now.duration_since(self.started) >= idle_after
            }
            None => false,
        };
        let above = match (self.policy.pressure_percent, memory) {
            (Some(percent), Some((working_set, limit))) if limit > 0 => {
                working_set as f64 >= limit as f64 * percent / 100.0
            }
            _ => false,
        };
        if !above {
            self.pressure_backoff = 1;
            self.pressure_unhelpful = false;
        }
        let interval = self
            .policy
            .pressure_interval
            .saturating_mul(self.pressure_backoff);
        if above
            && self
                .last_pressure
                .is_none_or(|at| now.duration_since(at) >= interval)
        {
            // Still above the threshold after an earlier reclamation (no
            // drop below it in between): that one did not help enough.
            if self.pressure_unhelpful {
                self.pressure_backoff = self
                    .pressure_backoff
                    .saturating_mul(2)
                    .min(MAX_PRESSURE_BACKOFF);
            }
            self.pressure_unhelpful = true;
            self.last_pressure = Some(now);
            // Pressure also covers what an idle reclamation would do.
            if !busy {
                self.armed = false;
            }
            return Some(ReclaimLevel::Pressure);
        }
        if quiet && self.armed && !busy {
            self.armed = false;
            return Some(ReclaimLevel::Idle);
        }
        None
    }
}

impl Tracker {
    /// Admission moved up into critical or emergency: a pressure
    /// reclamation now, unless pressure reclamation is off or one ran
    /// within `pressure_interval` (the backoff does not apply).
    fn admission_reclaim(&mut self, now: Instant) -> Option<ReclaimLevel> {
        self.policy.pressure_percent?;
        if self
            .last_pressure
            .is_some_and(|at| now.duration_since(at) < self.policy.pressure_interval)
        {
            return None;
        }
        self.last_pressure = Some(now);
        self.pressure_unhelpful = true;
        Some(ReclaimLevel::Pressure)
    }
}

/// Requests received so far (counted on arrival), and whether any is in
/// flight.
fn requests_received(http: &HttpMetrics) -> (u64, bool) {
    let in_flight = http.in_flight.load(Ordering::Relaxed) > 0;
    (http.arrivals.load(Ordering::Relaxed), in_flight)
}

fn resident_anon() -> u64 {
    crate::http_metrics::ProcessSnapshot::current(0.0).resident_anon_bytes
}

/// Starts the reclaimer on the current Tokio runtime, or returns `None`
/// when the policy enables nothing.
pub fn spawn(
    policy: ReclaimPolicy,
    http: Arc<HttpMetrics>,
    executor: Option<Arc<Executor>>,
    stats: Arc<ReclaimStats>,
) -> Option<tokio::task::JoinHandle<()>> {
    if !policy.is_enabled() {
        return None;
    }
    Some(tokio::spawn(async move {
        let mut tracker = Tracker::new(policy, Instant::now());
        // Located once: the process does not change cgroups.
        let cgroup = policy.pressure_percent.and_then(|_| Cgroup::locate());
        if policy.pressure_percent.is_some() && cgroup.is_none() {
            tracing::info!("no memory cgroup found: pressure reclamation has no limit to act on");
        }
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let (requests, in_flight) = requests_received(&http);
            let busy = in_flight || executor.as_ref().is_some_and(|e| !e.is_idle());
            let memory = if policy.pressure_percent.is_some() {
                let current = cgroup.as_ref().and_then(Cgroup::read);
                let limit = current.and_then(|m| m.limit).unwrap_or(0);
                let working_set = current.map_or(0, |m| m.working_set());
                stats.cgroup_limit.store(limit, Ordering::Relaxed);
                stats
                    .cgroup_working_set
                    .store(working_set, Ordering::Relaxed);
                Some((working_set, limit))
            } else {
                None
            };
            let backoff_before = tracker.pressure_backoff;
            let now = Instant::now();
            let mut level = tracker.tick(now, requests, busy, memory);
            // Taken every tick, so that a request seen while a reclamation
            // is not allowed does not fire one later, out of date.
            let requested = executor.as_ref().is_some_and(|e| e.take_reclaim_request());
            if level.is_none() && requested {
                level = tracker.admission_reclaim(now);
                if level.is_some() {
                    stats.admission_requested.fetch_add(1, Ordering::Relaxed);
                }
            }
            let Some(level) = level else {
                continue;
            };
            if level == ReclaimLevel::Pressure && tracker.pressure_backoff > backoff_before {
                stats.pressure_backoffs.fetch_add(1, Ordering::Relaxed);
            }
            let started = Instant::now();
            let before = resident_anon();
            let report = match &executor {
                Some(executor) => Some(executor.reclaim_memory(level).await),
                None => None,
            };
            let after_gc = resident_anon();
            // After the isolates: their collections free native memory too.
            let trimmed = tokio::task::spawn_blocking(nextrs_runtime::release_free_memory)
                .await
                .unwrap_or(false);
            let after = resident_anon();
            let counter = match level {
                ReclaimLevel::Idle => &stats.idle,
                ReclaimLevel::Pressure => &stats.pressure,
            };
            counter.fetch_add(1, Ordering::Relaxed);
            stats.micros.fetch_add(
                u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
            stats.last_anon_before.store(before, Ordering::Relaxed);
            stats.last_anon_after_gc.store(after_gc, Ordering::Relaxed);
            stats.last_anon_after.store(after, Ordering::Relaxed);
            tracing::debug!(
                level = level.as_str(),
                isolates = report.map_or(0, |r| r.isolates),
                heap_released = report.map_or(0, |r| r.heap_released_bytes),
                trimmed,
                anon_before = before,
                anon_after_gc = after_gc,
                anon_after = after,
                ms = started.elapsed().as_secs_f64() * 1e3,
                "memory reclaimed"
            );
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(idle: Option<u64>, pressure: Option<f64>) -> ReclaimPolicy {
        ReclaimPolicy {
            idle_after: idle.map(Duration::from_secs),
            pressure_percent: pressure,
            pressure_interval: Duration::from_secs(10),
        }
    }

    #[test]
    fn disabled_by_default() {
        assert!(!ReclaimPolicy::default().is_enabled());
        assert!(policy(Some(10), None).is_enabled());
        assert!(policy(None, Some(90.0)).is_enabled());
    }

    #[test]
    fn idle_reclaims_once_per_idle_period() {
        let t0 = Instant::now();
        let s = |n| t0 + Duration::from_secs(n);
        let mut t = Tracker::new(policy(Some(10), None), t0);
        // Load until 5 s.
        assert_eq!(t.tick(s(1), 10, true, None), None);
        assert_eq!(t.tick(s(5), 50, false, None), None);
        // Idle 10 s after the last request.
        assert_eq!(t.tick(s(14), 50, false, None), None);
        assert_eq!(t.tick(s(15), 50, false, None), Some(ReclaimLevel::Idle));
        // Only once.
        assert_eq!(t.tick(s(60), 50, false, None), None);
        // A few requests do not re-arm it (probes); traffic does.
        assert_eq!(t.tick(s(61), 54, false, None), None);
        assert_eq!(t.tick(s(80), 54, false, None), None);
        assert_eq!(t.tick(s(81), 61, false, None), None);
        assert_eq!(t.tick(s(90), 60, false, None), None);
        assert_eq!(t.tick(s(91), 60, false, None), Some(ReclaimLevel::Idle));
    }

    #[test]
    fn health_probes_do_not_prevent_or_repeat_idle_reclamation() {
        let t0 = Instant::now();
        let s = |n| t0 + Duration::from_secs(n);
        // Liveness and readiness probes every 10 s each, the second one
        // jittering by a few seconds; idle after 10 s (allowance 3) and
        // after 30 s (allowance 9).
        for idle in [10, 30] {
            let mut t = Tracker::new(policy(Some(idle), None), t0);
            let mut requests = 0;
            let mut fired = Vec::new();
            for n in 1..=600u64 {
                if n <= 100 {
                    requests += 30; // load
                } else {
                    requests += u64::from(n % 10 == 0); // liveness
                    requests += u64::from(n % 10 == (n / 10) % 4); // readiness, jittery
                }
                if t.tick(s(n), requests, false, None).is_some() {
                    fired.push(n);
                }
            }
            // Once, `idle` seconds after the load: probes neither delay
            // nor repeat it.
            assert_eq!(fired, vec![100 + idle], "idle {idle}: {fired:?}");
        }
    }

    #[test]
    fn idle_waits_for_idle_after_from_start() {
        let t0 = Instant::now();
        let mut t = Tracker::new(policy(Some(10), None), t0);
        for n in 1..10 {
            assert_eq!(t.tick(t0 + Duration::from_secs(n), 0, false, None), None);
        }
        assert_eq!(
            t.tick(t0 + Duration::from_secs(10), 0, false, None),
            Some(ReclaimLevel::Idle)
        );
    }

    #[test]
    fn quiet_allowance_scales_with_the_window() {
        assert_eq!(quiet_allowance(Duration::from_secs(1)), 3);
        assert_eq!(quiet_allowance(Duration::from_secs(10)), 3);
        assert_eq!(quiet_allowance(Duration::from_secs(30)), 9);
        assert_eq!(quiet_allowance(Duration::from_secs(86_400)), 25_920);
    }

    #[test]
    fn idle_never_reclaims_while_a_request_is_in_flight() {
        let t0 = Instant::now();
        let mut t = Tracker::new(policy(Some(1), None), t0);
        // A long streaming response: the counter does not move, but it is busy.
        for n in 1..100 {
            assert_eq!(t.tick(t0 + Duration::from_secs(n), 1, true, None), None);
        }
        assert_eq!(
            t.tick(t0 + Duration::from_secs(101), 1, false, None),
            Some(ReclaimLevel::Idle)
        );
    }

    #[test]
    fn pressure_reclaims_at_the_threshold_rate_limited() {
        let t0 = Instant::now();
        let s = |n| t0 + Duration::from_secs(n);
        let mut t = Tracker::new(policy(None, Some(90.0)), t0);
        let limit = 1000;
        assert_eq!(t.tick(s(1), 1, true, Some((899, limit))), None);
        assert_eq!(
            t.tick(s(2), 2, true, Some((900, limit))),
            Some(ReclaimLevel::Pressure)
        );
        // Rate-limited while still above it.
        assert_eq!(t.tick(s(5), 3, true, Some((950, limit))), None);
        assert_eq!(
            t.tick(s(12), 4, true, Some((950, limit))),
            Some(ReclaimLevel::Pressure)
        );
        // No limit, no pressure.
        assert_eq!(t.tick(s(40), 5, true, Some((950, 0))), None);
    }

    #[test]
    fn pressure_that_does_not_help_backs_off_and_resets() {
        let t0 = Instant::now();
        let s = |n| t0 + Duration::from_secs(n);
        let mut t = Tracker::new(policy(None, Some(90.0)), t0);
        let high = Some((950, 1000));
        // Reclaimed at 0, still above: next after 20 s, then 40, then 80,
        // and never more than 80 apart.
        let mut fired = Vec::new();
        for n in 0..400 {
            if t.tick(s(n), n, true, high).is_some() {
                fired.push(n);
            }
        }
        assert_eq!(&fired[..5], &[0, 10, 30, 70, 150], "{fired:?}");
        assert!(fired.windows(2).all(|w| w[1] - w[0] <= 80), "{fired:?}");
        // Dropping below the threshold resets the interval.
        assert_eq!(t.tick(s(400), 400, true, Some((100, 1000))), None);
        let after = (401..420).find(|&n| t.tick(s(n), n, true, high).is_some());
        assert!(after.is_some_and(|n| n <= 410), "{after:?}");
        // The first reclamation of a new episode does not count as
        // unhelpful: the next one follows after the base interval.
        let next = (after.unwrap() + 1..460).find(|&n| t.tick(s(n), n, true, high).is_some());
        assert_eq!(next.map(|n| n - after.unwrap()), Some(10));
    }

    #[test]
    fn pressure_while_idle_counts_as_the_idle_reclamation() {
        let t0 = Instant::now();
        let s = |n| t0 + Duration::from_secs(n);
        let mut t = Tracker::new(policy(Some(10), Some(90.0)), t0);
        assert_eq!(t.tick(s(1), 5, false, Some((100, 1000))), None);
        assert_eq!(
            t.tick(s(3), 5, false, Some((950, 1000))),
            Some(ReclaimLevel::Pressure)
        );
        assert_eq!(t.tick(s(20), 5, false, Some((100, 1000))), None);
    }

    #[test]
    fn admission_asks_for_a_reclaim_within_the_interval_not_the_backoff() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let policy = ReclaimPolicy {
            pressure_percent: Some(85.0),
            pressure_interval: Duration::from_secs(10),
            ..ReclaimPolicy::default()
        };
        let mut t = Tracker::new(policy, t0);
        assert_eq!(t.admission_reclaim(s(1)), Some(ReclaimLevel::Pressure));
        assert_eq!(t.admission_reclaim(s(5)), None, "within the interval");
        assert_eq!(t.admission_reclaim(s(11)), Some(ReclaimLevel::Pressure));
        // Without pressure reclamation configured, admission asks nothing.
        let mut off = Tracker::new(ReclaimPolicy::default(), t0);
        assert_eq!(off.admission_reclaim(s(1)), None);
    }
}
