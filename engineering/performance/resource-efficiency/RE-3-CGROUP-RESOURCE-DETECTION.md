# RE-3: cgroup resource detection

Status: implemented
and unit-tested (branch `feat/re-2-3-memory`); validated in Docker
cgroup v2 only (every container in this milestone's matrix, 128 MiB to
1 GiB).

## What NextRS needs to know

The memory the whole process may use before the kernel kills it, to
budget V8 heaps (`--js-heap-limit auto`, [RE-3-V8-HEAP-BUDGETING](RE-3-V8-HEAP-BUDGETING.md))
and to judge memory pressure (`--memory-pressure-reclaim`). That is the
lowest limit on the path from the process's own cgroup to the top of the
hierarchy it can see, not necessarily `/sys/fs/cgroup/memory.max`.

## What changed from RE-1 (65397c6, `main`)

| | Before | Now |
|---|---|---|
| Path resolution | `/sys/fs/cgroup` + the path from `/proc/self/cgroup` | The path resolved against the cgroup mount's own root from `/proc/self/mountinfo`; fallbacks below |
| v2 limits | `memory.max` up the path | the lowest of `memory.max` **and `memory.high`** up the path, with which one set it |
| v1 | `/sys/fs/cgroup/memory` + path | the v1 memory mount from mountinfo, same resolution |
| Hybrid v1/v2 | worked only if the v2 path lacked `memory.current` at the conventional place | explicit: v2 if its cgroup directory has `memory.current`, else v1 |
| Reclaimer | re-located the cgroup every second | located once at start (`Cgroup::locate`), read every second (`Cgroup::read`) |
| Tests | 1 v2 layout | 12 layouts (below) |

## Resolution rules (`nextrs_runtime::Cgroup::locate`)

1. **v2:** the `0::PATH` line of `/proc/self/cgroup`. For each `cgroup2`
   mount in `/proc/self/mountinfo` (octal escapes decoded), if `PATH` lies
   inside the mount's root, the directory is `MOUNT_POINT/(PATH − root)`;
   it is used if it has `memory.current`. A path that climbs out of the
   root (`..`, what a process moved outside its cgroup namespace sees) does
   not resolve.
2. Otherwise a mount point that itself has `memory.current` (a private
   cgroup namespace: the container's cgroup *is* the namespace root).
3. No readable mountinfo: the conventional `/sys/fs/cgroup`.
4. **v1** (`N:...memory...:PATH`, `cgroup` mounts with the `memory` super
   option), the same way, marker `memory.usage_in_bytes`.
5. Nothing found: no cgroup. `auto` then uses physical memory; pressure
   reclamation logs that it has no limit to act on.

**Effective limit (v2):** the lowest `memory.max` or `memory.high` from
the cgroup up to the mount point. `memory.high` counts because above it
the kernel throttles allocation and reclaims aggressively: no workload
stays above it for long, so it is the practical budget. The kind that won
is reported (`LimitKind::{Max, High}`) and logged at startup.
**v1:** `hierarchical_memory_limit` from `memory.stat` (already folds in
ancestors), else `memory.limit_in_bytes`; values ≥ 2^60 are "unlimited".

**`constrained memory`** = min(effective limit, physical memory), as
libuv's `uv_get_constrained_memory` does for Node.js.

## Failure handling

| Situation | Behaviour |
|---|---|
| Not Linux, no `/proc/self/cgroup` | no cgroup; `auto` uses physical memory |
| `max` / empty | no limit at that level |
| Malformed value (`lots`, `-1`) | treated as no limit at that level (never as 0) |
| Unreadable file (permissions) | skipped, as missing |
| cgroup removed after locating | `read()` returns `None`; the reclaimer reads no pressure |
| Limit 0 or tiny | budget clamps every heap to 64 MiB and warns ([budgeting](RE-3-V8-HEAP-BUDGETING.md)) |
| Limit above physical memory | physical memory wins |

## Known limits

- **Ancestors above a private cgroup namespace are invisible.** Inside a
  Docker/containerd container with `cgroupns=private` (the cgroup v2
  default), the namespace root is the container's cgroup; a pod-level
  limit above it cannot be read. Kubernetes puts the container's own
  limit on that cgroup, so this matters only when a pod limit is lower
  than the sum of its containers' limits (sidecars). Documented, not
  worked around.
- `memory.high` set by systemd-oomd or a PSI-based controller can move at
  runtime; it is read once for budgeting (at start) and every second for
  pressure.
- Swap (`memory.swap.max`) is not counted: a container with swap may
  exceed `memory.max` in RAM + swap terms. The benchmark containers have
  `--memory-swap` equal to `--memory` (no swap).
- Not validated live on cgroup v1 or on Kubernetes; the layouts below are
  synthetic directory trees.

## Tests (`crates/nextrs-runtime/src/memory.rs`)

| Test | Layout |
|---|---|
| `docker_private_namespace_v2` | `0::/`, cgroup2 at `/sys/fs/cgroup` root `/`, `memory.max` 256 MiB |
| `host_namespace_v2_walks_to_the_lowest_ancestor_limit` | `0::/system.slice/docker-abc.scope`, parent slice 512 MiB, sibling's limit ignored |
| `kubernetes_pod_limit_above_the_container` | burstable pod slice 256 MiB above a 1 GiB container scope |
| `memory_high_below_memory_max_is_the_effective_limit` | max 512, high 384 → 384, kind `High` |
| `a_bind_mounted_subtree_resolves_against_its_root` | mount root `/docker/abc`; and a `/../../other` path falls back to the mount point |
| `mount_points_with_escaped_characters` | `\040` in the mount point; unescape edge cases |
| `without_mountinfo_the_conventional_layout_is_assumed` | empty mountinfo |
| `v1_memory_controller` | v1 with `hierarchical_memory_limit` below an unlimited `limit_in_bytes` |
| `v1_unlimited_and_private_namespace` | v1 at the mount point, unlimited |
| `hybrid_hierarchy_uses_v1_when_v2_has_no_memory_controller` | v2 at `/sys/fs/cgroup/unified` without the memory controller + v1 memory |
| `unlimited_malformed_and_missing_values` | `max`, malformed `memory.high`, no files, removed cgroup |
| `reads_v2_limits`, `reads_stat_fields_exactly`, `working_set_excludes_inactive_page_cache` | parsers |

Run in Linux (rust 1.98, the CI toolchain): all pass; clippy clean with
`-D warnings`. Live check: the benchmark containers (Docker 29.7, cgroup
v2, private namespaces) report the limit given by `--memory` in the
startup log line `V8 heaps budgeted from the available memory` (see
[RE-23-CONSTRAINED-CONTAINER-VALIDATION](RE-23-CONSTRAINED-CONTAINER-VALIDATION.md)).
