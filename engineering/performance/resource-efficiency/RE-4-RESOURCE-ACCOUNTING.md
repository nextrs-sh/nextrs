# RE-4A: resource accounting

What holds memory in a NextRS process, how each part can be observed,
and which of them NextRS can control. This is the model the admission
controls ([RE-4-ADMISSION-CONTROL-DESIGN](RE-4-ADMISSION-CONTROL-DESIGN.md)),
native buffer budgets ([RE-4-NATIVE-BUFFER-BUDGETS](RE-4-NATIVE-BUFFER-BUDGETS.md))
and the external-memory limit ([RE-4-EXTERNAL-MEMORY-ANALYSIS](RE-4-EXTERNAL-MEMORY-ANALYSIS.md))
are built on. It was written before them.

Sources: the code at `main` `0d2a742` (file:line references below), and
the RE-1 to RE-23B measurements, which came from a Docker Desktop VM and
from processes on the Fedora host. No new container measurement was made
for this report ([RE-23B-LINUX-ENVIRONMENT](RE-23B-LINUX-ENVIRONMENT.md)).

## 1. Four kinds of number

Every quantity below is one of these. They are not interchangeable.

| Kind | Meaning | Example |
|---|---|---|
| **Measured** | Read from the kernel or V8 at a point in time | cgroup `memory.current`, V8 `used_heap_size` |
| **Reserved** | Bytes NextRS has counted against a budget before allocating them, and gives back when the owner is dropped | image `memory_budget` leases; the new native buffer budget; the new external-memory budget |
| **Estimated** | A per-request guess with no allocation behind it | the admission controller's per-request reserve |
| **Unobservable** | Allocations NextRS cannot see or attribute in time | V8-internal malloc, glibc retention per request, thread stacks in use |

Only reserved bytes can be enforced: an allocation that would cross a
budget can be refused before it happens. Measured bytes can only be acted
on after the fact. Estimated bytes are policy.

## 2. Inventory

Per-item and aggregate bounds are the defaults. "None" means no bound
other than the request, file or concurrency limits.

### 2.1 V8

| Consumer | Observed by | Bounded by | Control |
|---|---|---|---|
| JS heap, used and committed (per isolate) | `HeapStatistics` sampled once per second and after full GCs (`worker.rs` `event_loop`, `after_full_gc`) | `--js-heap-limit` (V8 terminates through the near-limit callback) | V8 heap constraint. Overshoot is not bounded ([RE-4-V8-HEAP-OVERSHOOT](RE-4-V8-HEAP-OVERSHOOT.md)) |
| Young generation (2 semi-spaces + new large-object space) | part of the heap stats | `heap_params` (`worker.rs:212`) | V8 constraint, inside the heap cap |
| ArrayBuffer backing stores made through the allocator (`new ArrayBuffer`, typed arrays, `Buffer.alloc`, `Buffer.concat`) | `external_memory()` (V8's own count) | **none** before RE-4: 512 MiB passed a 64 MiB heap limit (RE-23B) | RE-4: optional process-wide budget in a custom `ArrayBuffer::Allocator` ([RE-4-EXTERNAL-MEMORY-ANALYSIS](RE-4-EXTERNAL-MEMORY-ANALYSIS.md)) |
| Backing stores adopted from Rust (`#[buffer]` op returns, `ToJsBuffer`: request-body chunks, fetch chunks, `fs.readFileSync`, hashes) | `external_memory()` | the op's own limit, if any | not through the allocator: counted by V8, not by the budget |
| V8-internal malloc (compiler zones, code space, parser) | part of process anonymous memory only | none | unobservable per request |
| Code and metadata of loaded modules | heap stats (live floor 50–66 MiB for Velasia, RE-3) | the application | none |

### 2.2 Native memory controlled by NextRS (from the 31-site inventory, RE-4C)

| Consumer | Per item | Aggregate before RE-4 |
|---|---|---|
| Static and `public/` files: whole-file read (`lib.rs:2398`) | none | none (only `--max-connections` 1024) |
| Prerendered HTML / RSC held from startup (`lib.rs:821`, `pages.rs:82`) | none | all non-ISR prerenders, resident for the life of the process (fixed after startup) |
| Compression cache (`compress_cache.rs`) | 4 MiB identity body | `--compression-cache` (32 MiB; 8 low-memory). Fills in progress uncounted |
| Streaming gzip (`compress.rs:575`) | about one 32 KiB slice of output | one per compressed response |
| Request body with a proxy (middleware): `to_bytes` (`resolve.rs:1060`) | `min(proxyClientMaxBodySize, --max-request-body)`, 10 MiB | none |
| Request body without a proxy (`dynamic.rs:246`) | streamed, one hyper chunk (≤ 64 KiB) | concurrency |
| Response body from JS (`response.rs:148`) | `--response-buffer` 64 KiB + one chunk of any size, always copied | concurrency |
| Outbound fetch request body, "bytes" kind (`ops.rs:833`) | none | 256 in flight per isolate |
| Outbound fetch response (`net.rs:846`) | streamed; `--net-max-response` 32 MiB total | 256 in flight per isolate |
| External rewrite response (`net.rs:1143`) | streamed; no byte limit | none |
| Multipart parse (`multipart.rs:42`) | body (10 MiB / 1 MB actions) copied twice | none |
| Image source (`image/fetch.rs`) | `images.maximumResponseBody` 50 MB | `--image-source-budget` 512 MiB (reserved) |
| Image decode and encode (`nextrs-image`) | `--image-max-input-pixels` | `--image-memory-budget` 1 GiB (reserved), `--image-concurrency` |
| Image cache (memory) | 50 MiB | `--image-cache-max-memory` 64 MiB |
| Data / ISR cache (`cache.rs`) | 2 MiB fetch, 16 MiB response | `--cache-max-memory` 50 MiB; disk-write copies uncounted |
| Per-isolate decoded cache reuse | | `min(max_memory, 16 MiB)` per isolate |
| `fs.readFileSync` (`ops.rs:544`) | none | none |
| One-shot zlib (`ops.rs:656`) | `maxOutputLength` ≤ 1 GiB | none |
| Queued requests (`pool.rs:539`) | headers + URL; body not read | `--max-queued` 512 entries, no byte bound |
| Connection buffers (hyper) | 64 KiB | `--max-connections` 1024 |

### 2.3 Process and kernel

| Consumer | Observed by | Control |
|---|---|---|
| Thread stacks | `/proc/self/status` `VmStk`/`Threads`; 8 MiB virtual each for isolates, mostly untouched | thread count only |
| glibc retention (free memory kept in arenas) | `mallinfo2` (`metrics.rs:391`) | `malloc_trim` on idle/pressure (RE-2); arena cap |
| Executable and shared-library pages | cgroup `file`, `/proc/self/smaps` | file-backed, 37–42 MiB measured (RE-3); reclaimable only under pressure, and re-faulted at once when hot |
| Page cache of files read (static assets, build output) | cgroup `file` / `active_file` / `inactive_file` | the kernel |

## 3. The metrics, and how they overlap

| Metric | Includes | Use |
|---|---|---|
| cgroup `memory.current` | anon + all page cache + kernel memory charged to the cgroup | what `memory.max` is enforced against |
| working set = `memory.current − inactive_file` | anon + active page cache + kernel | what container platforms report; what the OOM killer effectively compares, because inactive page cache is reclaimed first |
| process `RssAnon` | heap + native + stacks touched | process-level fallback (no cgroup) |
| V8 `total_heap_size` | committed heap, per isolate | already inside anon |
| V8 `external_memory` | backing stores V8 knows about | already inside anon |
| `mallinfo2` in-use / free | the C heap | already inside anon |

**No sum.** V8 heap, V8 external and malloc in-use all live inside the
process's anonymous memory, which is inside the cgroup's usage. Adding
them double-counts. NextRS therefore uses **one** total for decisions
(the cgroup working set, or `RssAnon` without a cgroup) and uses the V8
figures only for per-isolate decisions (which isolate is near its heap
limit), never added to the total.

**File-backed memory is not free memory.** `inactive_file` is excluded
because the kernel reclaims it first. `active_file` is kept in the total:
it is reclaimable, but the binary's hot pages and the static files being
served are re-faulted at once, so treating them as free headroom would
over-admit. This makes the total conservative.

**Parent cgroups and neighbours.** `memory.rs` takes the lowest
`memory.max` / `memory.high` from the process's cgroup up to the mount
point, so a pod limit above the container cgroup is found. The **usage**
read is the process's own cgroup. When the binding limit belongs to an
ancestor shared with other containers (a pod with sidecars), their usage
counts against that limit but is invisible here. Inside a private cgroup
namespace, ancestors above the namespace root are invisible altogether.
The design therefore lets the operator set an explicit budget
(`--memory-admission-budget`) below the limit to leave room for
neighbours. NextRS cannot measure that room.

## 4. What NextRS can and cannot enforce

| Class | Examples | Can NextRS refuse the allocation? |
|---|---|---|
| **Hard, reserved** (new in RE-4, opt-in) | native buffer budget for whole-file reads and proxy body buffering; external-memory budget for allocator-made ArrayBuffers | yes: refused before allocating |
| **Hard, pre-existing** | image source and memory budgets; compression, image and data cache capacities; per-request limits (body, fetch response, response buffer) | yes |
| **Best-effort, admission** | memory-aware request admission (RE-4B) | no: it refuses *requests*, by measured working set and estimates, not allocations |
| **V8 heap constraint** | `--js-heap-limit` | V8 terminates the isolate after the limit is crossed; a large single allocation can still overshoot several times or abort the process |
| **Kernel** | cgroup `memory.max` | the OOM killer ends the process |
| **Not controllable** | V8-internal malloc, buffers adopted from Rust ops without a limit (`fs.readFileSync`), glibc retention between trims, thread stacks, file pages | no |

## 5. Decisions taken from this model

1. Admission acts on **one** measured total (working set), refreshed on
   demand (at most every 50 ms) rather than by a background poller, plus
   **reservations** for requests admitted since that reading. This
   handles bursts, which a sampled metric alone cannot.
2. Per-isolate V8 state is used only to pick or skip an isolate, never
   added to the total.
3. Missing or stale measurements make admission **more** conservative
   (a reduced concurrency cap). They do not stop all traffic, because a
   measurement failure must not become an outage.
4. Byte budgets are added only where the allocation can be reserved
   before it happens and released by RAII: native buffers NextRS
   allocates itself, and ArrayBuffers V8 allocates through the allocator.
5. Everything new is opt-in. No default changes.
