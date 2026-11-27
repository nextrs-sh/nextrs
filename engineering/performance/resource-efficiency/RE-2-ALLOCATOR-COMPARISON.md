# RE-2: allocator comparison

Status: screening complete (one round, `memory` workload); preliminary.
Soak tests run last (30 min, 2 h and 6 h, on a dedicated Fedora Linux
machine) and are not covered here.

## Method

`benchmarks/resource/run.mjs profile --variants benchmarks/resource/variants/re23-screening.json
--workload memory --configs 256m:1,512m:1 --rounds 1 --recovery 10,30,60,120`,
2026-11-27 01:39–03:24 UTC. Per run: a fresh container, 30 s idle, the
body-checked correctness pass (SSR, route handlers, one Server Action),
a cold pass, the mixed workload at 30 requests/s for 60 s, then 30 s
closed-loop on 16 connections, then recovery. Raw data:
`results/re23-screening/` (26 runs, none excluded; the VM recorded 0 %
steal in every phase; host load 2–125, highest during
`re2-balanced-256m` and `re2-512m`).

| Variant | Binary | Allocator | Notes |
|---|---|---|---|
| `base` | `af994c15…` (RE-1 baseline) | glibc 2.36 | |
| `base-arena2` | same | glibc, `MALLOC_ARENA_MAX=2` (environment: read once at malloc initialisation) | |
| `base-jemalloc` | same | jemalloc 5.3 (Debian `libjemalloc2`), `LD_PRELOAD` | mapped: `libjemalloc.so.2` (verified in `/proc/PID/maps`) |
| `base-mimalloc` | same | mimalloc 2.0.9 (Debian `libmimalloc2.0`), `LD_PRELOAD` | mapped: `libmimalloc.so.2.0` |
| `re2-trim` | `2d70295d…` (this branch) | glibc + `--memory-reclaim-idle 10` (V8 low-memory GC, then `malloc_trim(0)`) | |
| `re2-native` | same | glibc + arena cap 2 + idle trim 10 s + pressure trim at 85 % | |

`LD_PRELOAD` replaces `malloc` for **every** caller in the process: Rust,
V8's C++ (zones, ArrayBuffer backing stores via `calloc`) and libraries.
A Rust `#[global_allocator]` would replace only Rust's allocations; V8
would stay on glibc. With a preloaded allocator, glibc's `mallinfo2`
reports glibc's own (empty) arenas, so the "malloc free" figures for
those runs are meaningless (0); compare resident anonymous memory.

## Results

MiB; container `memory.current` (anonymous in brackets).

### 256 MiB, 1 vCPU

| Variant | Outcome | Idle | Peak | Notes |
|---|---|---:|---:|---|
| base | **OOM-killed** in the closed-loop phase (246 failed) | 35.5 (30.3) | 256 | reproduces RE-1 |
| base-arena2 | **OOM-killed** (112 failed) | 35.3 (30.1) | 256 | |
| base-jemalloc | **OOM-killed** (4 failed) | 76.1 (63.2) | 256 | +33 MiB anonymous at idle |
| base-mimalloc | **OOM-killed** (135 failed) | 48.6 (41.0) | 256 | +11 MiB anonymous at idle |
| re2-trim | **OOM-killed** (1 failed) | 34.0 (30.5) | 254 | the trim only runs when idle |
| re2-native | survived; 3 pressure reclamations under load (7.2 s of collection on 1 vCPU) | 33.4 (29.8) | 225 | closed-loop throughput 62/s vs 120/s for the heap budget |

**No allocator change prevents the 256 MiB OOM.** The kill is V8 heap
growth ([RE-3](RE-3-V8-MEMORY-OPTIMIZATION.md)); the allocator decides only
how much of the remaining room is retained. Pressure reclamation without a
heap limit kept the process alive at a large CPU cost.

### 512 MiB, 1 vCPU

| Variant | Peak | After 10 s | After 60 s | After 120 s (anon) | malloc free at 120 s | Closed-loop req/s | CPU ms/req (mixed 30/s) |
|---|---:|---:|---:|---:|---:|---:|---:|
| base | 262 | 165 | 150 | 149.5 (137.7) | 67.6 | 106 | 21.4* |
| base-arena2 | **290** | 162 | 164 | 164.1 (155.7) | 83.0 | 114 | 10.9 |
| base-jemalloc | **363** | 180 | 149 | 144.4 (108.7) | n/a | 148 | 10.5 |
| base-mimalloc | **358** | 309 | 155 | 155.7 (117.3) | n/a | 113 | 14.2 |
| re2-trim | 299 | 165 | **86** | **86.8 (78.9)** | 81.3 (trimmed, not resident) | 142 | 10.4 |
| re2-native | 316 | 176 | 178 | 177.6 (168.5) | 95.9 | 141 | 10.3 |

\* `base` ran first in each configuration; its CPU and p99 (1.4 s) at a
fixed rate are out of line with every other glibc run and with RE-1
(10.2 ms); treat as noise until the 3-round matrix.

**How much did the trim return?** The reclaimer's own readings
(`last_anon_before / after_gc / after`): `re2-trim` 512 MiB **155 → 151
(V8 collection) → 77 MiB (`malloc_trim`)**: 74 MiB of resident memory
returned by the trim in 0.39 s, V8's collection 4 MiB (V8 had shrunk on
its own already). `re2-lowmem` (heap budget + trim): 122 → 119 → 78 at
512 MiB, 112 → 109 → 75 at 256 MiB. After the trim, mallinfo still
reports 81 MiB "free": those pages were returned to the kernel and are
counted as free chunks, not resident memory.

`re2-native` at 512 MiB never ran its idle trim within 120 s: see the
idle-detection defect below. Its 178 MiB at 120 s is untrimmed retention
with only 2 arenas, which is *higher* than the default's 150.

## The idle-detection defect (found here, fixed)

The idle trims fired 30–120 s after the load instead of 10 s (and not at
all for the 30 s `balanced` setting). Cause: Velasia's image has a Docker
`HEALTHCHECK` that requests `/api/internal/health/live` every 10 s, and the
RE-1 reclaimer (65397c6, on `main`) treated any request as activity, so a
10 s idle period occurred only when a probe was late. Every real platform
probes like this (Kubernetes liveness, Railway health checks, load
balancers). Fix (`crates/nextrs-server/src/reclaim.rs`): idle means
*quiet*: at most 0.3 requests per second over the idle window (at least
3 per window), counted on arrival; and only traffic above twice that
re-arms the reclaimer, so probes (including jittery Kubernetes liveness
plus readiness at 0.2/s) neither prevent nor repeat it
(`health_probes_do_not_prevent_or_repeat_idle_reclamation`). The first
version of the fix (one request per 5 s) was tightened after the
independent review ([RE-23-INDEPENDENT-REVIEW](RE-23-INDEPENDENT-REVIEW.md), M2). The
candidate was rebuilt as `re2b` (matrix, wide, envelope, cycles) and, with
the review fixes, as `re2c` (confirmation).

## Verdict

| Strategy | Memory | CPU / throughput | Cost | Decision |
|---|---|---|---|---|
| glibc, default | baseline | baseline | none | **keep** |
| glibc, 2 arenas | worse: +28 MiB peak, +15 MiB after load at 512 MiB | no gain | none | **reject** (drop from `low-memory`) |
| glibc + idle `malloc_trim` | **−63 MiB after load at 512 MiB** (149 → 87); returns ~90 % of the retained free memory | 0.3–0.4 s of one core per idle period | none | **adopt** (with the fixed idle detection) |
| glibc + pressure trim | keeps a heap-unbounded process alive at 256 MiB | expensive on 1 vCPU (≈2.4 s per reclamation under load) | none | keep as a safety net behind the heap budget, not as the fix |
| jemalloc 5.3 | worse: +33 MiB idle, +101 MiB peak; better decay after 60 s | higher closed-loop throughput (148/s, one round) | a second allocator to ship (`.so` in the npm package or static link), covers V8 only via preload | **reject** for this milestone |
| mimalloc 2.0 | worse: +11 MiB idle, +96 MiB peak | no clear gain | as jemalloc | **reject** |

The system allocator plus controlled reclamation gives the best overall
result. jemalloc's throughput figure is worth a controlled repeat on
dedicated hardware before it is dismissed for CPU-bound deployments; for
memory it is the wrong direction.
