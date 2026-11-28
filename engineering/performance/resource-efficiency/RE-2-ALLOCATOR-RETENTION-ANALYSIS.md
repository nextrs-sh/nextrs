# RE-2: native allocator retention analysis

Status: mechanism from code audit and glibc behaviour; measurements from
the screening series ([RE-2-ALLOCATOR-COMPARISON](RE-2-ALLOCATOR-COMPARISON.md),
results `re23-screening`). Preliminary: one Docker Desktop VM on a shared
Mac. Soak tests run last (30 min, 2 h and 6 h, on a dedicated
Fedora Linux machine) and are not part of this report.

## Question

RE-1 measured 85–101 MiB of free memory held by glibc 120 s after load
(55–60 % of the process's anonymous memory). How much of that is
retention the allocator could return, how much is fragmentation, and what
in NextRS produces it?

## What the numbers mean (and do not)

Memory figures in these reports come from different layers that overlap.
They are never added together:

| Figure | Source | Contains | Does not contain |
|---|---|---|---|
| Container `memory.current` | cgroup | anonymous + page cache (file) + kernel memory charged to the cgroup | – |
| Anonymous (`memory.stat anon`) | cgroup | every resident private page: V8 heap pages, malloc'd pages, stacks | file pages (the mapped binary, libraries) |
| File-backed | cgroup `file` | the binary and libraries (37–42 MiB), files read | – |
| V8 heap committed / used | `/metrics.json` executors | pages V8 obtained with mmap for its heap / bytes of live+dead objects in them | ArrayBuffer backing stores (malloc), zone memory |
| V8 external | V8 | bytes V8 was told are held outside the heap (ArrayBuffers, op buffers) | – (it is a count of malloc'd memory, not separate memory) |
| malloc in use | mallinfo2 `uordblks` + `hblkhd` | Rust allocations, V8 C++ (zones, ArrayBuffers via calloc), libraries | V8 heap pages |
| malloc free | mallinfo2 `fordblks` | free chunks in all arenas, **including pages already returned to the kernel with `madvise(DONTNEED)`** | – |
| Virtual size (~18 GiB) | `/proc` | V8's pointer-compression cage and sandbox reservations | not resident, not a cost |

Two consequences:

- **V8 external is inside "malloc in use"**, not on top of it.
- **"malloc free" is not resident memory.** After `malloc_trim(0)`, glibc
  keeps the trimmed ranges in its free lists (still counted in
  `fordblks`) but their pages are gone from RSS. So whether retained free
  memory is reclaimable can only be judged from resident anonymous memory
  before and after a trim — which is why the reclaimer now records
  `last_anon_before`, `last_anon_after_gc` (after V8's collections) and
  `last_anon_after` (after the trim) separately.

## Where NextRS allocates native memory (code audit)

Paths relative to `crates/` at `79e4fe5` + this branch.

**Threads, and therefore arenas.** glibc gives a thread that finds the
existing arenas locked a new arena, up to `8 × CPUs` (glibc counts the
CPUs it sees: the VM's 8, not the container's quota: up to 64 arenas). An
arena's free chunks stay resident until trimmed; per-thread arenas
(`mmap`'d 64 MiB heaps) shrink on their own only from the top.

| Thread(s) | Count | Allocates |
|---|---|---|
| Tokio workers (`nextrs-cli/src/main.rs:1042`) | `available_parallelism` (follows the cgroup quota: 1–2 here) | HTTP parsing, routing, response assembly, streaming gzip |
| Tokio blocking pool | up to 512, 10 s keep-alive | **every static file read in full** (`server/src/lib.rs:2399`), image transforms, cache-fill gzip (`compress_cache.rs:377`), data-cache disk I/O, `malloc_trim` |
| Isolate threads `nextrs-v8-N` (`runtime/src/worker.rs:54`) | `--isolates` | V8 C++ (compiler zones, ArrayBuffer backing stores via `calloc`), op buffers, cache envelopes |
| Supervisors `nextrs-supervisor-N` (`runtime/src/pool.rs:747`) | one per isolate | little |
| V8 platform workers (deno_core `setup.rs:199`) | `min(available_parallelism, 4)` | concurrent marking/sweeping, background compilation (zones) |
| Outbound fetch runtime `nextrs-net` (`runtime/src/net.rs:725`) | 2 workers, only with `--net-allow` | fetch bodies (up to 32 MiB each) |

No thread sets a stack size: 2 MiB virtual each, resident only as used.

**Bounded caches (live data, not retention):** compression cache 32 MiB
(8 with `low-memory`; static files and non-ISR prerenders), data/ISR store
50 MiB of values (entries ≤ 2 MiB fetch / ≤ 16 MiB responses), per-isolate
decoded-entry reuse `min(cache, 16 MiB)` on the V8 heap, image cache
64 MiB. **Unbounded except by the build:** prerendered HTML/RSC bodies
loaded at startup (`server/src/lib.rs:615-688`).

**Per-request native memory:** a zlib-rs deflate state of ~256–300 KiB
per concurrent gzip stream (`server/src/compress.rs:590`, level 6) plus
32 KiB of buffer; a copy of each `res.write` chunk (`runtime/src/response.rs:161`),
64 KiB response budget; hyper's per-connection buffers up to 64 KiB;
request bodies streamed (10 MiB limit) with each chunk copied into a V8
backing store; multipart bodies copied whole, then per part.

**Transient large buffers (fragmentation sources):** whole static files
(`Vec::with_capacity(len)`, on blocking threads), cache hits copied twice
(`String::from_utf8_lossy` + serde_v8, `runtime/src/cache.rs:578,656`),
cache sets copied into a `String` and again when persisted, the
cache-envelope base64 Buffers in JavaScript (`runtime/js/03_server.js`).
Anything below the 1 MiB mmap threshold (NextRS sets
`M_MMAP_THRESHOLD = 1 MiB` at start for images; that also fixes glibc's
trim threshold at 128 KiB instead of letting it grow) comes from an arena;
above it, from `mmap`, returned on `free`.

**Mechanism, as far as the code shows:** under load, the blocking pool
grows threads for static files and cache fills; each thread allocates
whole-file buffers and gzip copies from whichever arena it gets; when the
load ends those threads exit after 10 s, and their arenas' freed chunks
stay resident. The isolate thread's arena keeps the freed ArrayBuffer and
zone memory of the load's peak. V8's own heap is not in malloc at all and
shrinks by itself (memory reducer), which is why RE-1 saw V8 back at
52 MiB while malloc free stayed at 85–101 MiB.

## Measured

Sources: `results/re23-screening` (`memory` workload: mixed 30/s for 60 s,
30 s closed-loop, recovery to 120 s) and `results/re23-matrix`,
`results/re23-wide` (`full` workload, recovery to 300 s). The reclaimer
records resident anonymous memory before a reclamation, after V8's
low-memory collection, and after `malloc_trim(0)`.

### What a trim returns (the reclaimer's own readings, MiB anonymous)

| Run | Before | After V8 low-memory GC | After `malloc_trim` | Returned by the trim | Time (all reclamations) |
|---|---:|---:|---:|---:|---:|
| `re2b-trim` 512 MiB / 1 vCPU (no heap budget) | 250.2 | 153.7 | **77.7** | 76.0 | 0.36 s |
| `re2-trim` 512 MiB / 1 vCPU (screening) | 155 | 151 | **77** | 74 | 0.39 s |
| combined, 256 MiB / 1 vCPU, r1–r3 | 97–114 | 95–107 | **74–77** | 19–31 | 0.24–0.28 s |
| combined, 256 MiB / 0.5 vCPU, r1–r3 | 92–104 | 90–101 | **73–75** | 17–26 | 0.59–1.50 s |
| combined, 512 MiB / 1 vCPU, r1–r3 | 103–125 | 101–105 | **75–80** | 25–27 | 0.26–0.43 s |
| combined, 1 GiB / 1 vCPU | 160.0 | 131.7 | **78.6** | 53.1 | 0.33 s |
| combined, 1 GiB / 2 vCPU | 169.2 | 142.7 | **79.0** | 63.7 | 0.32 s |
| combined, 128 MiB / 0.5 vCPU | 84.5 | 84.4 | **78.8** | 5.6 | 1.71 s (incl. one pressure reclamation) |

Every candidate run made exactly two idle reclamations: one after startup
(start-up counts as activity) and one 10 s after the load. Pressure
reclamation ran once, in the 128 MiB container, and never with more room.

### After load, with and without trimming (MiB anonymous, 300 s after load, median of 3 where available)

| Configuration | `base` (no trim) | `malloc free` in `base` | combined (budget + trim) |
|---|---:|---:|---:|
| 256 MiB / 1 vCPU | 137.8 (the one survivor) | 64.6 | 87.0 |
| 256 MiB / 0.5 vCPU | 134.9 (the one survivor) | 63.0 | 78.3 |
| 512 MiB / 1 vCPU | 156.4 | 80.6 | 83.2 |
| 512 MiB / 0.5 vCPU | 154.9 | 77.7 | 80.1 |
| 1 GiB / 1 vCPU | 160.8 | 84.5 | 79.2 |
| 1 GiB / 2 vCPU | 162.5 | 88.3 | 100.6 |

### Arena count

`MALLOC_ARENA_MAX=2` made retention **worse**, not better, at 512 MiB:
peak 290 against 262, 164 MiB 120 s after load against 150, `malloc free`
83 against 68 (one round each; `re2-native` with the cap and no timely
trim: 178 MiB). Fewer arenas mean more threads sharing each arena, so a
freed chunk is less often at an arena's top, where glibc can shrink it;
the main arena only shrinks from the top of its `sbrk` heap. The cap was
removed from the `low-memory` profile.

## Answers

1. **How much of the after-load memory was allocator retention?** 63–88
   MiB of `malloc free` 300 s after load in every `base` run, i.e. 40–55 %
   of the anonymous memory. V8's own heap was back to 51–54 MiB committed
   in the same runs (RE-1's finding, repeated).
2. **How much was actually reclaimable?** Nearly all of it. One
   `malloc_trim(0)` returned 74–76 MiB of resident memory in 0.4 s at
   512 MiB without a heap budget, and every run, at every size, ended at
   **73–80 MiB anonymous** after the trim, whatever it held before (84–250).
   That floor is V8's heap (~51 MiB committed) plus ~25 MiB of live native
   memory, stacks and caches: there is no large unreturnable remainder.
3. **How much is fragmentation?** At most a few MiB in these runs: after
   the trim, `malloc free` still reads 25–60 MiB, but those pages are no
   longer resident (trimmed ranges stay in glibc's free lists). Resident
   anonymous memory does not exceed the live-data floor by more than
   ~10 MiB. Fragmentation that pins whole pages would show up as a higher
   post-trim floor in longer runs; the soaks test that.
4. **What in NextRS produces it?** The code audit above points at
   transient whole-file and cache-copy buffers allocated across many
   short-lived blocking-pool threads and the isolate thread's ArrayBuffer
   and zone churn. The measurement is consistent with that (retention
   scales with the load's peak, not with uptime, and is returned by a
   trim) but did not instrument per-arena ownership; buffer reuse in
   those paths is a code change for a later milestone, not needed for the
   result here.
5. **Does memory come back after the trim?** Partly, at 2 vCPU: in the
   1 GiB / 2 vCPU run anonymous memory went from 79 MiB after the trim to
   101 MiB at 300 s with only health probes arriving (at 1 vCPU it stayed
   at 79–83). The idle reclaimer does not repeat while the process stays
   quiet (by design: one reclamation per idle period), so such regrowth
   stays until the next load-and-idle cycle. The load/idle cycles and the
   soaks measure whether it accumulates.
