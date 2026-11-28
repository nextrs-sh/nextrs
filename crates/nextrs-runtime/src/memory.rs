//! Process memory: the container's memory limit and usage (Linux cgroups),
//! the V8 heap budget derived from it, and the native allocator's
//! retention (glibc arenas).
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
//! limit and to return retained memory to the system.
//!
//! # Finding the cgroup
//!
//! The process's cgroup is the path in `/proc/self/cgroup` (`0::PATH` for
//! the unified v2 hierarchy, `N:memory:PATH` for the v1 memory
//! controller). That path is relative to the root of the hierarchy as the
//! process's cgroup namespace sees it, which is not necessarily what is
//! mounted at `/sys/fs/cgroup`: `/proc/self/mountinfo` names the mount's
//! own root within the hierarchy, and the path is resolved against it.
//! When that fails (a path outside the mount, an unreadable mountinfo, a
//! private namespace whose root is the container itself), the mount point
//! itself is used if it carries the controller's files. The effective
//! limit is the lowest `memory.max` or `memory.high` from the cgroup up to
//! the mount point (v1: `hierarchical_memory_limit`, which folds in the
//! ancestors). Ancestors above a private namespace's root are invisible
//! from inside it; the container runtime puts the container's limit on the
//! namespace root.

use std::path::{Component, Path, PathBuf};

const MIB: u64 = 1024 * 1024;

/// cgroup v1 reports "no limit" as a page-aligned value near `i64::MAX`.
const V1_UNLIMITED: u64 = 1 << 60;

/// Which control file set a cgroup's effective limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// `memory.max` (v2) or the v1 limit: reaching it means OOM kills.
    Max,
    /// `memory.high` (v2): above it the kernel throttles and reclaims
    /// hard. Not a kill limit, but no workload stays above it for long.
    High,
}

/// The memory cgroup this process is in, at one point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CgroupMemory {
    /// The lowest memory limit on the way from this process's cgroup to the
    /// top of the visible hierarchy. `None`: no limit.
    pub limit: Option<u64>,
    /// What set `limit`.
    pub limit_kind: Option<LimitKind>,
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
    /// Linux or where no memory cgroup is found. Locates the cgroup on
    /// every call; to read repeatedly, keep a [`Cgroup`].
    pub fn current() -> Option<Self> {
        Cgroup::locate()?.read()
    }
}

/// cgroup hierarchy version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupVersion {
    V1,
    V2,
}

/// A located memory cgroup: where its control files are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cgroup {
    pub version: CgroupVersion,
    /// The cgroup's directory.
    pub dir: PathBuf,
    /// The hierarchy's mount point: limits are looked for from `dir` up to
    /// here.
    pub mount: PathBuf,
}

impl Cgroup {
    /// Locates this process's memory cgroup. `None` outside Linux or where
    /// no memory controller is reachable.
    pub fn locate() -> Option<Self> {
        let cgroups = std::fs::read_to_string("/proc/self/cgroup").ok()?;
        let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        Self::locate_in(&cgroups, &mountinfo, Path::new("/"))
    }

    /// [`Cgroup::locate`] from given `/proc/self/cgroup` and
    /// `/proc/self/mountinfo` text, with mount points under `prefix`
    /// (`/` outside tests).
    fn locate_in(cgroups: &str, mountinfo: &str, prefix: &Path) -> Option<Self> {
        let mounts = parse_mountinfo(mountinfo);
        if let Some(path) = cgroups.lines().find_map(|line| line.strip_prefix("0::")) {
            let candidates = mounts.iter().filter(|m| m.fstype == "cgroup2");
            if let Some(found) = resolve(
                CgroupVersion::V2,
                path,
                candidates,
                "/sys/fs/cgroup",
                "memory.current",
                prefix,
            ) {
                return Some(found);
            }
        }
        let path = cgroups.lines().find_map(|line| {
            let mut fields = line.splitn(3, ':');
            let (_, controllers, path) = (fields.next()?, fields.next()?, fields.next()?);
            controllers
                .split(',')
                .any(|c| c == "memory")
                .then_some(path)
        })?;
        let candidates = mounts
            .iter()
            .filter(|m| m.fstype == "cgroup" && m.super_options.split(',').any(|o| o == "memory"));
        resolve(
            CgroupVersion::V1,
            path,
            candidates,
            "/sys/fs/cgroup/memory",
            "memory.usage_in_bytes",
            prefix,
        )
    }

    /// Reads the cgroup's limit and usage. `None` if its files are gone or
    /// unreadable.
    pub fn read(&self) -> Option<CgroupMemory> {
        match self.version {
            CgroupVersion::V2 => read_v2(&self.dir, &self.mount),
            CgroupVersion::V1 => read_v1(&self.dir),
        }
    }
}

/// One cgroup filesystem mount from `/proc/self/mountinfo`.
#[derive(Debug)]
struct Mount {
    /// The mount's root within the hierarchy.
    root: String,
    mount_point: String,
    fstype: String,
    super_options: String,
}

/// The cgroup mounts in mountinfo text (`man 5 proc`: fields up to " - ",
/// then filesystem type, source, super options).
fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let mut fields = before.split(' ');
            let root = fields.nth(3)?;
            let mount_point = fields.next()?;
            let mut after = after.split(' ');
            let fstype = after.next()?;
            if !fstype.starts_with("cgroup") {
                return None;
            }
            let _source = after.next();
            Some(Mount {
                root: unescape(root),
                mount_point: unescape(mount_point),
                fstype: fstype.to_owned(),
                super_options: after.next().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

/// mountinfo escapes space, tab, newline and backslash as `\NNN` (octal).
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'\\'
            && let Some(digits) = bytes.get(i + 1..i + 4)
            && digits.iter().all(|d| (b'0'..=b'7').contains(d))
        {
            let value = digits
                .iter()
                .fold(0u32, |acc, d| acc * 8 + u32::from(d - b'0'));
            if let Ok(value) = u8::try_from(value) {
                out.push(value);
                i += 4;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `path` relative to `root`, both cgroup paths as `/proc` prints them;
/// `None` if `path` is not inside `root` or climbs out of it (`..`, which
/// a process moved outside its cgroup namespace sees).
fn relative_to(path: &str, root: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    let rest = path.strip_prefix(root).ok()?;
    rest.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| rest.to_path_buf())
}

/// The cgroup directory for `path`: below the first mount whose root
/// contains it, else a mount point that carries `marker` itself (a private
/// namespace's root), else the conventional mount point. A path that
/// climbs out of the hierarchy (`..`: the process sits outside its cgroup
/// namespace) resolves to nothing rather than to some other cgroup.
fn resolve<'a>(
    version: CgroupVersion,
    path: &str,
    mounts: impl Iterator<Item = &'a Mount>,
    conventional: &str,
    marker: &str,
    prefix: &Path,
) -> Option<Cgroup> {
    let at = |mount_point: &str| prefix.join(mount_point.trim_start_matches('/'));
    if Path::new(path)
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return None;
    }
    let mounts: Vec<&Mount> = mounts.collect();
    let mut fallbacks = Vec::new();
    for mount in &mounts {
        let mount_point = at(&mount.mount_point);
        if let Some(rel) = relative_to(path, &mount.root) {
            let dir = mount_point.join(rel);
            if dir.join(marker).exists() {
                return Some(Cgroup {
                    version,
                    dir,
                    mount: mount_point,
                });
            }
        }
        fallbacks.push(mount_point);
    }
    if mounts.is_empty() {
        // No readable mountinfo: assume the conventional layout.
        let mount_point = at(conventional);
        if let Some(rel) = relative_to(path, "/") {
            let dir = mount_point.join(rel);
            if dir.join(marker).exists() {
                return Some(Cgroup {
                    version,
                    dir,
                    mount: mount_point,
                });
            }
        }
        fallbacks.push(mount_point);
    }
    fallbacks
        .into_iter()
        .find(|mount_point| mount_point.join(marker).exists())
        .map(|mount_point| Cgroup {
            version,
            dir: mount_point.clone(),
            mount: mount_point,
        })
}

fn read_number(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// A v2 `memory.max` or `memory.high` value: a byte count, or `max` for
/// none. Anything else (malformed, unreadable) counts as no limit.
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

fn read_v2(dir: &Path, top: &Path) -> Option<CgroupMemory> {
    let usage = read_number(&dir.join("memory.current"))?;
    let stat = std::fs::read_to_string(dir.join("memory.stat")).unwrap_or_default();
    // The effective limit is the lowest one among the cgroup and its
    // ancestors (a pod limit above a container cgroup, for example).
    let mut limit: Option<(u64, LimitKind)> = None;
    let mut current = Some(dir);
    while let Some(d) = current {
        for (file, kind) in [
            ("memory.max", LimitKind::Max),
            ("memory.high", LimitKind::High),
        ] {
            if let Some(value) = std::fs::read_to_string(d.join(file))
                .ok()
                .and_then(|t| parse_v2_limit(&t))
                && limit.is_none_or(|(l, _)| value < l)
            {
                limit = Some((value, kind));
            }
        }
        if d == top {
            break;
        }
        current = d.parent().filter(|p| p.starts_with(top));
    }
    Some(CgroupMemory {
        limit: limit.map(|(l, _)| l),
        limit_kind: limit.map(|(_, k)| k),
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
        limit_kind: limit.map(|_| LimitKind::Max),
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
/// capped by physical memory.
pub fn constrained_memory() -> Option<u64> {
    let physical = physical_memory();
    let limit = CgroupMemory::current().and_then(|m| m.limit);
    match (limit, physical) {
        (Some(l), Some(p)) => Some(l.min(p)),
        (l, p) => l.or(p),
    }
}

/// Native memory a NextRS process uses outside the V8 heaps whatever the
/// isolate count, budgeted before the heaps (RE-3, measured on Velasia
/// Airways): the mapped binary and libraries, charged to the container as
/// page cache (37-42 MiB), the HTTP server, Tokio and the response cache
/// metadata (about 10 MiB), and headroom.
pub const HEAP_BUDGET_BASE_RESERVE: u64 = 64 * MIB;
/// Native memory per isolate outside its heap: V8's external memory
/// (array buffers, 9-28 MiB under load), the isolate thread's native
/// allocations and the allocator's retention around them. The 64 MiB
/// granted past the heap limit while an isolate is replaced is not
/// reserved: it is used only by the allocation that hit the limit.
pub const HEAP_BUDGET_ISOLATE_RESERVE: u64 = 48 * MIB;
/// The smallest heap limit `auto` gives an isolate. Next.js and React's
/// live data alone are about 50 MiB on Velasia; below this an isolate
/// would be replaced over and over rather than serve.
pub const HEAP_BUDGET_MIN: u64 = 64 * MIB;

/// How the V8 heaps of an isolate pool are sized from the memory the
/// process may use ([`heap_budget`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapBudget {
    /// Memory the process may use (the container limit, or physical).
    pub available: u64,
    /// Kept outside the heaps: the base reserve, the compression cache and
    /// every isolate's reserve.
    pub reserved: u64,
    pub isolates: usize,
    /// The heap limit of each isolate. V8 may lower it further: it never
    /// exceeds what V8 picks for `sizing_memory` on its own.
    pub heap_limit: u64,
    /// The memory V8 configures each heap's other defaults from (young
    /// generation, growth, initial limit), as if the isolate ran on a
    /// machine with this much memory: `available / isolates`.
    pub sizing_memory: u64,
    /// The budget left less than [`HEAP_BUDGET_MIN`] per isolate and the
    /// minimum was used instead: the process may not fit the container.
    pub clamped: bool,
}

/// Divides `available` bytes among `isolates` V8 heaps after reserving the
/// process's native memory (`compression_cache` bytes of response cache
/// among it). The whole pool shares one budget: two isolates get half the
/// heap each, not the container each.
#[must_use]
pub fn heap_budget(available: u64, isolates: usize, compression_cache: u64) -> HeapBudget {
    let isolates = isolates.max(1);
    let count = u64::try_from(isolates).unwrap_or(u64::MAX);
    let reserved = HEAP_BUDGET_BASE_RESERVE
        .saturating_add(compression_cache)
        .saturating_add(HEAP_BUDGET_ISOLATE_RESERVE.saturating_mul(count));
    let per_isolate = available.saturating_sub(reserved) / count;
    HeapBudget {
        available,
        reserved,
        isolates,
        heap_limit: per_isolate.max(HEAP_BUDGET_MIN),
        sizing_memory: (available / count).max(HEAP_BUDGET_MIN),
        clamped: per_isolate < HEAP_BUDGET_MIN,
    }
}

/// Caps glibc's malloc arenas. glibc gives each thread that allocates
/// concurrently an arena of its own, up to 8 per CPU of the machine (not
/// of the container), and an arena's free memory stays resident; fewer
/// arenas means less retained memory, at the cost of lock contention when
/// many threads allocate at once. Not applied when the operator set it
/// already (`MALLOC_ARENA_MAX`, `GLIBC_TUNABLES`: glibc reads those once,
/// when malloc initialises, so they are the operator's to set before
/// start). `mallopt` takes effect for arenas created afterwards: call
/// before threads start. Returns whether it was applied.
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
/// glibc keeps counting the released pages as free in `mallinfo2`: judge
/// the effect by resident memory, not by the allocator's statistics.
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

    /// A fake filesystem root for cgroup layouts, removed on drop.
    struct Root(PathBuf);

    impl Root {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "nextrs-cg-{}-{name}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, path: &str, content: &str) -> &Self {
            let file = self.0.join(path.trim_start_matches('/'));
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, content).unwrap();
            self
        }

        fn locate(&self, cgroups: &str, mountinfo: &str) -> Option<Cgroup> {
            Cgroup::locate_in(cgroups, mountinfo, &self.0)
        }

        fn read(&self, cgroups: &str, mountinfo: &str) -> Option<CgroupMemory> {
            self.locate(cgroups, mountinfo)?.read()
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const MI_V2_ROOT: &str = "35 24 0:30 / /sys/fs/cgroup ro,nosuid,nodev,noexec,relatime - cgroup2 cgroup rw,nsdelegate\n";
    const MI_V1: &str = "\
40 35 0:35 / /sys/fs/cgroup/memory ro,nosuid,nodev,noexec,relatime master:16 - cgroup cgroup rw,memory
41 35 0:36 / /sys/fs/cgroup/cpu,cpuacct ro,nosuid master:17 - cgroup cgroup rw,cpu,cpuacct
";

    #[test]
    fn docker_private_namespace_v2() {
        // `docker run --memory 256m` with a private cgroup namespace: the
        // container's cgroup is the namespace's root.
        let root = Root::new("docker-ns");
        root.write("sys/fs/cgroup/memory.max", "268435456\n")
            .write("sys/fs/cgroup/memory.high", "max\n")
            .write("sys/fs/cgroup/memory.current", "1000\n")
            .write("sys/fs/cgroup/memory.stat", "anon 900\ninactive_file 100\n");
        let m = root.read("0::/\n", MI_V2_ROOT).unwrap();
        assert_eq!(m.limit, Some(256 * MIB));
        assert_eq!(m.limit_kind, Some(LimitKind::Max));
        assert_eq!(m.usage, 1000);
        assert_eq!(m.working_set(), 900);
    }

    #[test]
    fn host_namespace_v2_walks_to_the_lowest_ancestor_limit() {
        let root = Root::new("host-ns");
        let scope = "sys/fs/cgroup/system.slice/docker-abc.scope";
        root.write(&format!("{scope}/memory.max"), "max\n")
            .write(&format!("{scope}/memory.current"), "5\n")
            .write("sys/fs/cgroup/system.slice/memory.max", "536870912\n")
            // The host's root cgroup has no memory.max; an unrelated
            // sibling's limit must not count.
            .write("sys/fs/cgroup/user.slice/memory.max", "1\n");
        let cg = root
            .locate("0::/system.slice/docker-abc.scope\n", MI_V2_ROOT)
            .unwrap();
        assert!(cg.dir.ends_with("system.slice/docker-abc.scope"));
        assert_eq!(cg.read().unwrap().limit, Some(512 * MIB));
    }

    #[test]
    fn kubernetes_pod_limit_above_the_container() {
        let root = Root::new("k8s");
        let pod =
            "sys/fs/cgroup/kubepods.slice/kubepods-burstable.slice/kubepods-burstable-pod1.slice";
        root.write(&format!("{pod}/memory.max"), "268435456\n")
            .write(
                &format!("{pod}/cri-containerd-x.scope/memory.max"),
                "1073741824\n",
            )
            .write(
                &format!("{pod}/cri-containerd-x.scope/memory.current"),
                "7\n",
            );
        let path = "0::/kubepods.slice/kubepods-burstable.slice/kubepods-burstable-pod1.slice/cri-containerd-x.scope\n";
        assert_eq!(root.read(path, MI_V2_ROOT).unwrap().limit, Some(256 * MIB));
    }

    #[test]
    fn memory_high_below_memory_max_is_the_effective_limit() {
        let root = Root::new("high");
        root.write("sys/fs/cgroup/memory.max", "536870912\n")
            .write("sys/fs/cgroup/memory.high", "402653184\n")
            .write("sys/fs/cgroup/memory.current", "1\n");
        let m = root.read("0::/\n", MI_V2_ROOT).unwrap();
        assert_eq!(m.limit, Some(384 * MIB));
        assert_eq!(m.limit_kind, Some(LimitKind::High));
    }

    #[test]
    fn a_bind_mounted_subtree_resolves_against_its_root() {
        // /sys/fs/cgroup is the container's own subtree, but the process's
        // path is printed from the hierarchy's root (no cgroup namespace).
        let root = Root::new("bind");
        root.write("sys/fs/cgroup/memory.max", "134217728\n")
            .write("sys/fs/cgroup/memory.current", "1\n");
        let mi = "35 24 0:30 /docker/abc /sys/fs/cgroup ro - cgroup2 cgroup rw\n";
        let cg = root.locate("0::/docker/abc\n", mi).unwrap();
        assert_eq!(cg.dir, root.0.join("sys/fs/cgroup"));
        assert_eq!(cg.read().unwrap().limit, Some(128 * MIB));
        // The namespace's root, printed as `/`, on a mount of a subtree.
        let cg = root.locate("0::/\n", mi).unwrap();
        assert_eq!(cg.dir, root.0.join("sys/fs/cgroup"));
        // A path that climbs out of the namespace resolves to nothing,
        // not to the mount point's (another) cgroup.
        assert_eq!(root.locate("0::/../../other\n", MI_V2_ROOT), None);
    }

    #[test]
    fn mount_points_with_escaped_characters() {
        let root = Root::new("escaped");
        root.write("cg roup/memory.max", "1048576\n")
            .write("cg roup/memory.current", "1\n");
        let mi = "35 24 0:30 / /cg\\040roup rw - cgroup2 cgroup rw\n";
        assert_eq!(root.read("0::/\n", mi).unwrap().limit, Some(MIB));
        assert_eq!(unescape("a\\134b\\011"), "a\\b\t");
        assert_eq!(unescape("trailing\\04"), "trailing\\04");
    }

    #[test]
    fn without_mountinfo_the_conventional_layout_is_assumed() {
        let root = Root::new("no-mountinfo");
        root.write("sys/fs/cgroup/x.slice/memory.max", "2097152\n")
            .write("sys/fs/cgroup/x.slice/memory.current", "1\n");
        assert_eq!(root.read("0::/x.slice\n", "").unwrap().limit, Some(2 * MIB));
    }

    #[test]
    fn v1_memory_controller() {
        let root = Root::new("v1");
        let dir = "sys/fs/cgroup/memory/docker/abc";
        root.write(&format!("{dir}/memory.usage_in_bytes"), "4096\n")
            .write(
                &format!("{dir}/memory.limit_in_bytes"),
                "9223372036854771712\n",
            )
            .write(
                &format!("{dir}/memory.stat"),
                "hierarchical_memory_limit 268435456\ntotal_inactive_file 96\ninactive_file 1\n",
            );
        let cgroups = "12:memory:/docker/abc\n4:cpu,cpuacct:/docker/abc\n";
        let cg = root.locate(cgroups, MI_V1).unwrap();
        assert_eq!(cg.version, CgroupVersion::V1);
        let m = cg.read().unwrap();
        assert_eq!(m.limit, Some(256 * MIB));
        assert_eq!(m.inactive_file, 96);
    }

    #[test]
    fn v1_unlimited_and_private_namespace() {
        let root = Root::new("v1-unlimited");
        root.write("sys/fs/cgroup/memory/memory.usage_in_bytes", "1\n")
            .write(
                "sys/fs/cgroup/memory/memory.limit_in_bytes",
                "9223372036854771712\n",
            );
        let m = root.read("12:memory:/\n", MI_V1).unwrap();
        assert_eq!(m.limit, None);
        assert_eq!(m.limit_kind, None);
    }

    #[test]
    fn hybrid_hierarchy_uses_v1_when_v2_has_no_memory_controller() {
        let root = Root::new("hybrid");
        root.write("sys/fs/cgroup/unified/init.scope/cgroup.procs", "1\n")
            .write("sys/fs/cgroup/memory/memory.usage_in_bytes", "1\n")
            .write("sys/fs/cgroup/memory/memory.limit_in_bytes", "1048576\n");
        let mi = format!("30 24 0:26 / /sys/fs/cgroup/unified rw - cgroup2 cgroup2 rw\n{MI_V1}");
        let cg = root.locate("12:memory:/\n0::/init.scope\n", &mi).unwrap();
        assert_eq!(cg.version, CgroupVersion::V1);
        assert_eq!(cg.read().unwrap().limit, Some(MIB));
    }

    #[test]
    fn unlimited_malformed_and_missing_values() {
        let root = Root::new("odd");
        root.write("sys/fs/cgroup/memory.max", "max\n")
            .write("sys/fs/cgroup/memory.high", "lots\n")
            .write("sys/fs/cgroup/memory.current", "1\n");
        let m = root.read("0::/\n", MI_V2_ROOT).unwrap();
        assert_eq!((m.limit, m.limit_kind), (None, None));
        // No memory controller files at all: no cgroup.
        let empty = Root::new("empty");
        assert_eq!(empty.locate("0::/\n", MI_V2_ROOT), None);
        assert_eq!(empty.locate("", MI_V2_ROOT), None);
        // Located, then the files disappear (the cgroup was removed).
        let cg = root.locate("0::/\n", MI_V2_ROOT).unwrap();
        drop(root);
        assert_eq!(cg.read(), None);
    }

    #[test]
    fn reads_v2_limits() {
        assert_eq!(parse_v2_limit("max\n"), None);
        assert_eq!(parse_v2_limit("268435456\n"), Some(268_435_456));
        assert_eq!(parse_v2_limit("garbage"), None);
        assert_eq!(parse_v2_limit("-1"), None);
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
            limit_kind: Some(LimitKind::Max),
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
    fn heap_budget_reserves_native_memory_first() {
        // 256 MiB container, one isolate, the default 32 MiB cache:
        // 64 + 32 + 48 reserved, 112 MiB of heap.
        let b = heap_budget(256 * MIB, 1, 32 * MIB);
        assert_eq!(b.reserved, 144 * MIB);
        assert_eq!(b.heap_limit, 112 * MIB);
        assert_eq!(b.sizing_memory, 256 * MIB);
        assert!(!b.clamped);
        assert!(b.heap_limit + b.reserved <= b.available);
        // low-memory's 8 MiB cache leaves more for the heap.
        assert_eq!(heap_budget(256 * MIB, 1, 8 * MIB).heap_limit, 136 * MIB);
    }

    #[test]
    fn heap_budget_divides_the_pool_not_the_container() {
        let one = heap_budget(1024 * MIB, 1, 32 * MIB);
        let four = heap_budget(1024 * MIB, 4, 32 * MIB);
        assert_eq!(one.heap_limit, 880 * MIB);
        // 1024 - 64 - 32 - 4 * 48 = 736, a quarter each.
        assert_eq!(four.heap_limit, 184 * MIB);
        assert_eq!(four.sizing_memory, 256 * MIB);
        assert!(four.heap_limit * 4 + four.reserved <= four.available);
    }

    #[test]
    fn heap_budget_clamps_to_the_minimum_and_says_so() {
        let b = heap_budget(256 * MIB, 2, 32 * MIB);
        assert!(b.clamped);
        assert_eq!(b.heap_limit, HEAP_BUDGET_MIN);
        let tiny = heap_budget(0, 0, 0);
        assert_eq!(tiny.isolates, 1);
        assert_eq!(tiny.heap_limit, HEAP_BUDGET_MIN);
        assert_eq!(tiny.sizing_memory, HEAP_BUDGET_MIN);
        assert!(tiny.clamped);
        let huge = heap_budget(u64::MAX, usize::MAX, u64::MAX);
        assert!(huge.clamped);
    }
}
