//! Process memory: the container's memory limit and usage (Linux cgroups),
//! and the native allocator's retention (glibc arenas).
//!
//! Two different budgets exist and are kept apart:
//!
//! * the **container** (cgroup) limit applies to the whole process: V8
//!   heaps, the native heap Rust and V8 allocate from, thread stacks, and
//!   the page cache of files the process read;
//! * a **V8 heap** limit applies to one isolate's JavaScript heap only.
//!
//! Nothing here enforces a process limit; the kernel does that. What this
//! module provides is the information to size heaps from the container's
//! limit (as Node.js does) and to return retained memory to the system.

use std::path::{Path, PathBuf};

/// cgroup v1 reports "no limit" as a page-aligned value near `i64::MAX`.
const V1_UNLIMITED: u64 = 1 << 60;

/// The memory cgroup this process is in, at one point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CgroupMemory {
    /// The lowest memory limit on the way from this process's cgroup to the
    /// root. `None`: no limit.
    pub limit: Option<u64>,
    /// Memory charged to the cgroup (includes the page cache).
    pub usage: u64,
    /// Inactive page cache the kernel reclaims first under pressure.
    pub inactive_file: u64,
}

impl CgroupMemory {
    /// Usage minus inactive page cache: what the kernel cannot easily
    /// reclaim, and what an OOM kill is judged against in practice (the
    /// same "working set" container platforms report).
    pub fn working_set(&self) -> u64 {
        self.usage.saturating_sub(self.inactive_file)
    }

    /// Reads this process's memory cgroup (v2, else v1). `None` outside
    /// Linux or where no memory cgroup is mounted.
    pub fn current() -> Option<Self> {
        let cgroups = std::fs::read_to_string("/proc/self/cgroup").ok()?;
        if let Some(path) = v2_path(&cgroups) {
            let dir = Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/'));
            if dir.join("memory.current").exists() {
                return read_v2(&dir, Path::new("/sys/fs/cgroup"));
            }
        }
        let dir = v1_dir(&cgroups)?;
        read_v1(&dir)
    }
}

/// The unified (v2) hierarchy's path: the `0::` line of /proc/self/cgroup.
fn v2_path(cgroups: &str) -> Option<&str> {
    cgroups.lines().find_map(|line| line.strip_prefix("0::"))
}

/// The v1 memory controller's directory for this process.
fn v1_dir(cgroups: &str) -> Option<PathBuf> {
    let path = cgroups.lines().find_map(|line| {
        let mut fields = line.splitn(3, ':');
        let (_, controllers, path) = (fields.next()?, fields.next()?, fields.next()?);
        controllers
            .split(',')
            .any(|c| c == "memory")
            .then_some(path)
    })?;
    let mount = Path::new("/sys/fs/cgroup/memory");
    // Inside a container with a private cgroup namespace the path is `/`
    // and the controller is mounted at the cgroup itself.
    let dir = mount.join(path.trim_start_matches('/'));
    Some(if dir.join("memory.usage_in_bytes").exists() {
        dir
    } else {
        mount.to_path_buf()
    })
}

fn read_number(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// A v2 `memory.max` value: a byte count, or `max` for none.
pub(crate) fn parse_v2_limit(text: &str) -> Option<u64> {
    match text.trim() {
        "max" | "" => None,
        value => value.parse().ok(),
    }
}

/// A `memory.stat` field.
pub(crate) fn stat_field(stat: &str, key: &str) -> Option<u64> {
    stat.lines().find_map(|line| {
        let (k, v) = line.split_once(' ')?;
        (k == key).then(|| v.trim().parse().ok()).flatten()
    })
}

fn read_v2(dir: &Path, root: &Path) -> Option<CgroupMemory> {
    let usage = read_number(&dir.join("memory.current"))?;
    let stat = std::fs::read_to_string(dir.join("memory.stat")).unwrap_or_default();
    // The effective limit is the lowest one among the cgroup and its
    // ancestors (a pod limit above a container cgroup, for example).
    let mut limit: Option<u64> = None;
    let mut current = Some(dir);
    while let Some(d) = current {
        if let Some(value) = std::fs::read_to_string(d.join("memory.max"))
            .ok()
            .and_then(|t| parse_v2_limit(&t))
        {
            limit = Some(limit.map_or(value, |l| l.min(value)));
        }
        if d == root {
            break;
        }
        current = d.parent().filter(|p| p.starts_with(root));
    }
    Some(CgroupMemory {
        limit,
        usage,
        inactive_file: stat_field(&stat, "inactive_file").unwrap_or(0),
    })
}

fn read_v1(dir: &Path) -> Option<CgroupMemory> {
    let usage = read_number(&dir.join("memory.usage_in_bytes"))?;
    let stat = std::fs::read_to_string(dir.join("memory.stat")).unwrap_or_default();
    // hierarchical_memory_limit already folds in the ancestors' limits.
    let limit = stat_field(&stat, "hierarchical_memory_limit")
        .or_else(|| read_number(&dir.join("memory.limit_in_bytes")))
        .filter(|l| *l < V1_UNLIMITED);
    Some(CgroupMemory {
        limit,
        usage,
        inactive_file: stat_field(&stat, "total_inactive_file")
            .or_else(|| stat_field(&stat, "inactive_file"))
            .unwrap_or(0),
    })
}

/// Physical memory of the machine (or VM), in bytes.
pub fn physical_memory() -> Option<u64> {
    #[cfg(unix)]
    {
        // SAFETY: sysconf has no preconditions.
        let (pages, page_size) = unsafe {
            (
                libc::sysconf(libc::_SC_PHYS_PAGES),
                libc::sysconf(libc::_SC_PAGESIZE),
            )
        };
        let pages = u64::try_from(pages).ok()?;
        let page_size = u64::try_from(page_size).ok()?;
        pages.checked_mul(page_size)
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// The memory this process may use: the container limit where one is set,
/// capped by physical memory (what Node.js sizes its heap from:
/// `uv_get_constrained_memory`, else `uv_get_total_memory`).
pub fn constrained_memory() -> Option<u64> {
    let physical = physical_memory();
    let limit = CgroupMemory::current().and_then(|m| m.limit);
    match (limit, physical) {
        (Some(l), Some(p)) => Some(l.min(p)),
        (l, p) => l.or(p),
    }
}

/// Caps glibc's malloc arenas. glibc gives each thread that allocates
/// concurrently an arena of its own, up to 8 per CPU of the machine (not
/// of the container), and an arena's free memory stays resident; fewer
/// arenas means less retained memory, at the cost of lock contention when
/// many threads allocate at once. Not applied when the operator set it
/// already (`MALLOC_ARENA_MAX`, `GLIBC_TUNABLES`). Call before threads
/// start. Returns whether it was applied.
pub fn set_malloc_arena_max(arenas: usize) -> bool {
    if arenas == 0
        || std::env::var_os("MALLOC_ARENA_MAX").is_some()
        || std::env::var("GLIBC_TUNABLES").is_ok_and(|t| t.contains("arena_max"))
    {
        return false;
    }
    set_arena_max(arenas)
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn set_arena_max(arenas: usize) -> bool {
    let Ok(value) = libc::c_int::try_from(arenas) else {
        return false;
    };
    // SAFETY: mallopt only changes allocator parameters; M_ARENA_MAX takes
    // any positive value.
    unsafe { libc::mallopt(libc::M_ARENA_MAX, value) == 1 }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn set_arena_max(_arenas: usize) -> bool {
    false
}

/// Returns the native allocator's free memory to the system (glibc's
/// `malloc_trim(0)`: every arena's free pages, not only the top of the main
/// heap). Takes each arena's lock in turn, so allocations on other threads
/// wait briefly; call when idle or under memory pressure, not per request.
/// Returns whether anything was released (false where unsupported).
pub fn release_free_memory() -> bool {
    trim()
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim() -> bool {
    // SAFETY: malloc_trim has no preconditions; it only releases free
    // pages of the allocator's own arenas.
    unsafe { libc::malloc_trim(0) == 1 }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_unified_hierarchy_path() {
        let text = "12:memory:/docker/abc\n0::/system.slice/docker-abc.scope\n";
        assert_eq!(v2_path(text), Some("/system.slice/docker-abc.scope"));
        assert_eq!(v2_path("0::/\n"), Some("/"));
        assert_eq!(v2_path("4:memory:/x\n"), None);
    }

    #[test]
    fn reads_v2_limits() {
        assert_eq!(parse_v2_limit("max\n"), None);
        assert_eq!(parse_v2_limit("268435456\n"), Some(268_435_456));
        assert_eq!(parse_v2_limit("garbage"), None);
    }

    #[test]
    fn reads_stat_fields_exactly() {
        let stat = "anon 100\ninactive_file 4096\nactive_file 8\ntotal_inactive_file 7\n";
        assert_eq!(stat_field(stat, "inactive_file"), Some(4096));
        assert_eq!(stat_field(stat, "total_inactive_file"), Some(7));
        assert_eq!(stat_field(stat, "file"), None);
    }

    #[test]
    fn working_set_excludes_inactive_page_cache() {
        let m = CgroupMemory {
            limit: Some(100),
            usage: 80,
            inactive_file: 30,
        };
        assert_eq!(m.working_set(), 50);
        let odd = CgroupMemory {
            inactive_file: 90,
            ..m
        };
        assert_eq!(odd.working_set(), 0);
    }

    #[test]
    fn reads_a_v2_hierarchy_with_the_lowest_limit() {
        let root = std::env::temp_dir().join(format!("nextrs-cg-{}", std::process::id()));
        let leaf = root.join("pod/container");
        std::fs::create_dir_all(&leaf).unwrap();
        std::fs::write(root.join("pod/memory.max"), "268435456\n").unwrap();
        std::fs::write(leaf.join("memory.max"), "max\n").unwrap();
        std::fs::write(leaf.join("memory.current"), "1000\n").unwrap();
        std::fs::write(leaf.join("memory.stat"), "anon 900\ninactive_file 100\n").unwrap();
        let m = read_v2(&leaf, &root).unwrap();
        assert_eq!(m.limit, Some(268_435_456));
        assert_eq!(m.usage, 1000);
        assert_eq!(m.working_set(), 900);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
