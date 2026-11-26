//! Memory reclamation: gives memory the process no longer uses back to the
//! system after load, and under container memory pressure.
//!
//! Two sources of retained memory dominate after load (RE-1 baseline):
//! V8 heaps that grew for the load and stay committed, and free memory the
//! native allocator (glibc) keeps in its arenas. Neither is a leak, and both
//! are reused by the next load; on a memory-limited container they are what
//! stands between the steady state and the limit.
//!
//! * **Idle:** once the server has had no request for `idle_after`, each
//!   isolate runs V8's low-memory collection and the allocator's free pages
//!   are released (`malloc_trim`). Once per idle period: the next request
//!   re-arms it.
//! * **Pressure:** when the container's working set (usage minus inactive
//!   page cache) reaches `pressure_percent` of its memory limit, the same
//!   happens with V8's critical memory-pressure response, even under load,
//!   at most once per `pressure_interval`.
//!
//! Neither is a memory limit: the kernel's OOM killer still applies at the
//! container's limit. Requests are never refused or delayed by this, except
//! by the collection itself (requests in an isolate wait for its GC, as
//! for any full collection).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use nextrs_runtime::{CgroupMemory, Executor, ReclaimLevel};

use crate::HttpMetrics;

/// How often the reclaimer looks at activity and memory.
const TICK: Duration = Duration::from_secs(1);

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
    /// Resident anonymous memory before and after the latest reclamation.
    pub last_anon_before: AtomicU64,
    pub last_anon_after: AtomicU64,
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
             # HELP nextrs_memory_reclaim_last_anon_after_bytes Resident anonymous memory after the latest reclamation.\n\
             # TYPE nextrs_memory_reclaim_last_anon_after_bytes gauge\n\
             nextrs_memory_reclaim_last_anon_after_bytes {}\n\
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
            get(&self.last_anon_after),
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
            "last_anon_after_bytes": get(&self.last_anon_after),
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
    requests_seen: u64,
    last_activity: Instant,
    /// A request arrived since the last idle reclamation. Starting counts
    /// as activity: loading the build leaves garbage and freed allocator
    /// memory behind.
    armed: bool,
    last_pressure: Option<Instant>,
}

impl Tracker {
    fn new(policy: ReclaimPolicy, now: Instant) -> Self {
        Self {
            policy,
            requests_seen: 0,
            last_activity: now,
            armed: true,
            last_pressure: None,
        }
    }

    /// `requests` is a counter of requests received; `busy`: a request is
    /// in flight or waiting. Returns the reclamation to perform now.
    fn tick(
        &mut self,
        now: Instant,
        requests: u64,
        busy: bool,
        memory: Option<(u64, u64)>,
    ) -> Option<ReclaimLevel> {
        if requests != self.requests_seen || busy {
            self.requests_seen = requests;
            self.last_activity = now;
            self.armed = true;
        }
        if let (Some(percent), Some((working_set, limit))) = (self.policy.pressure_percent, memory)
            && limit > 0
            && working_set as f64 >= limit as f64 * percent / 100.0
            && self
                .last_pressure
                .is_none_or(|at| now.duration_since(at) >= self.policy.pressure_interval)
        {
            self.last_pressure = Some(now);
            // Pressure also covers what an idle reclamation would do.
            if !busy {
                self.armed = false;
            }
            return Some(ReclaimLevel::Pressure);
        }
        if let Some(idle_after) = self.policy.idle_after
            && self.armed
            && !busy
            && now.duration_since(self.last_activity) >= idle_after
        {
            self.armed = false;
            return Some(ReclaimLevel::Idle);
        }
        None
    }
}

/// Requests received so far (completed or in flight).
fn requests_received(http: &HttpMetrics) -> (u64, bool) {
    let completed: u64 = http
        .requests
        .iter()
        .map(|c| c.load(Ordering::Relaxed))
        .sum();
    let in_flight = http.in_flight.load(Ordering::Relaxed).max(0);
    let in_flight = u64::try_from(in_flight).unwrap_or(0);
    (completed.saturating_add(in_flight), in_flight > 0)
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
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let (requests, in_flight) = requests_received(&http);
            let busy = in_flight || executor.as_ref().is_some_and(|e| !e.is_idle());
            let memory = if policy.pressure_percent.is_some() {
                let current = CgroupMemory::current();
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
            let Some(level) = tracker.tick(Instant::now(), requests, busy, memory) else {
                continue;
            };
            let started = Instant::now();
            let before = resident_anon();
            let report = match &executor {
                Some(executor) => Some(executor.reclaim_memory(level).await),
                None => None,
            };
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
            stats.last_anon_after.store(after, Ordering::Relaxed);
            tracing::debug!(
                level = level.as_str(),
                isolates = report.map_or(0, |r| r.isolates),
                heap_released = report.map_or(0, |r| r.heap_released_bytes),
                trimmed,
                anon_before = before,
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
        // A new request re-arms it.
        assert_eq!(t.tick(s(61), 51, false, None), None);
        assert_eq!(t.tick(s(71), 51, false, None), Some(ReclaimLevel::Idle));
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
}
