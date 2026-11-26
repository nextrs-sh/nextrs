# Memory efficiency

What NextRS's memory consists of on the Series 1 workloads, how it moves
under load and after it, and what each V8 isolate costs. Local
measurements only (see [BASELINE.md](BASELINE.md#local-reproduction-environment)).

- [What is measured](#what-is-measured)
- [Series 1's memory observation](#series-1s-memory-observation)
- [Idle, peak and after recovery](#idle-peak-and-after-recovery)
- [Without V8, and per isolate](#without-v8-and-per-isolate)
- [Where the memory is](#where-the-memory-is)
- [Findings](#findings)

## What is measured

| Measure | Scope | Includes file cache? |
|---|---|---|
| cgroup `memory.current`, `memory.peak` | the whole container | yes (page cache of files it read) |
| cgroup `memory.stat` `anon` / `file` | the whole container, split | `file` only |
| `VmRSS`, `VmHWM` of the main process | the server process (PID 1 in both images) | file-backed **mapped** pages (the binary, libraries), not the page cache |
| `RssAnon` | the server process | no |
| NextRS `executors[].heap_used_bytes` | each isolate's V8 heap | no |
| NextRS `nextrs_malloc_*` | glibc's view of the native heap (`mallinfo2`) | no |

`next start`'s process RSS and NextRS's process RSS have the same scope
and are compared directly. Railway's memory figure is a container figure
and includes file cache.

## Series 1's memory observation

Railway recorded `next start` rising from about 89 to 220 MiB and NextRS
from about 129 to 230 MiB during the series. Locally, both reach a
similar level under load: **neither is a leak**. Each run below ends with
60–90 s idle; NextRS returns most of its peak, `next start` keeps more.
Series 1 measured only under load and never read memory after it.

## Idle, peak and after recovery

Gated series (`gated-20261126`, 3 rounds; one isolate):

| | `next start` | NextRS `instr` | NextRS `optA` |
|---|---:|---:|---:|
| Idle RSS, 15 s after ready (MiB) | 102 | 67.8 | 68.3 |
| of which anonymous (MiB) | 49.7 | 31.1 | 31.0 |
| Container memory idle (MiB) | 55.3 | 35.1 | 35.1 |
| Peak RSS over the run (MiB) | 230 | 303 | **230** |
| Container peak (MiB) | 221 | 274 | **196** |
| RSS after 60 s idle (MiB) | 230 | 134 | 130 |
| of which anonymous (MiB) | 172 | 93.1 | 88.3 |
| V8 heap used after 60 s idle (MiB) | n/a | 34.1 | 37.1 |

- **Idle:** NextRS uses about two thirds of `next start`'s RSS.
- **Peak:** before the RSC change NextRS peaked higher than `next start`
  (303 against 230 MiB): every RSC request created about half a megabyte
  of garbage strings and buffers. With reuse, the peak is the same as
  `next start`'s.
- **After recovery:** NextRS returns to 130 MiB; `next start` stays at
  its peak (V8's heap and Node's buffers are not given back within a
  minute).

## Without V8, and per isolate

`memory-20261126` (2 rounds; NextRS `optA`): idle 20 s after ready, then
static assets at 200 req/s for 30 s and RSC payloads on 4 connections per
isolate for 30 s (no warm-up: these runs measure memory, not speed), then
60 s idle.

| NextRS | Threads idle | Idle RSS (anon) | RSS after load (anon) | RSS after 60 s (anon) | Peak RSS |
|---|---:|---:|---:|---:|---:|
| No executor (static and in-memory responses only) | 3 | **17.1 MiB** (3.4) | 22.0 (8.3) | 21.9 (8.3) | 22.4 |
| 1 isolate | 11 | 67.9 (30.9) | 226 (184) | 131 (89.5) | 256 |
| 2 isolates | 13 | 88.2 (51.0) | 369 (328) | 204 (162) | 431 |
| 4 isolates | 17 | 131 (93.2) | 689 (647) | 312 (270) | 728 |
| `next start` (for reference) | 11 | 128 (72.5) | 230 (171) | 180 (121) | 231 |

- **V8 is the bulk of NextRS's idle footprint.** Without an executor,
  NextRS serves the build's static files in 17 MiB (3.4 MiB anonymous).
  The executor is started whenever `--js-runtime embedded-v8` is given,
  whether or not a request ever needs JavaScript; for Velasia it is needed
  (its pages use ISR and Partial Prerendering).
- **Each additional isolate** costs about 20 MiB idle, 150–175 MiB at
  peak under this RSC load, and 55–75 MiB after recovery. Each
  isolate has its own heap, its own copy of Next's modules and its own
  decoded cache entries.
- **Four isolates in a 1 GB container peaked at 728 MiB** on this
  workload. More isolates than CPUs (4 isolates on a 2-CPU quota) also
  did not add RSC throughput in these unwarmed runs; size isolates to
  CPUs and memory together.

## Where the memory is

After recovery, one isolate (`optA`, `memory-20261126`):

| Part | MiB |
|---|---:|
| Process RSS | 131 |
| File-backed (binary, libraries) | about 41 |
| Anonymous | about 89 |
| of which V8 heap used (one isolate) | 37 |
| of which free memory glibc keeps (`nextrs_malloc_free_bytes`) | 32–35 |

About a third of the anonymous memory after recovery is free memory glibc
retains in its arenas, not live data; the rc.3 compression-cache churn
soak documented the same allocator behaviour (`PERF-1`). With four
isolates the retained free memory grows to 88–93 MiB.

## Findings

| Finding | Evidence | Status |
|---|---|---|
| Per-request garbage on RSC requests raised NextRS's peak above `next start`'s | Peak 303 against 230 MiB before the change; 230 after | Fixed (`91a7729`) |
| No leak on these workloads | Memory returns after load on NextRS; stable heap after GC | Established locally |
| V8 costs ~50 MiB idle even for static-only traffic | 17 against 68 MiB idle | By design; an application needing no JavaScript can run without `--js-runtime embedded-v8` |
| Per-isolate cost is large under load | +150–175 MiB peak per isolate | Documented; sizing guidance |
| glibc retains freed memory | 32–93 MiB free in arenas after recovery | Known (rc.3 `PERF-1`); a follow-up could bound arenas or trim after load, with its own measurements |
| Reused cache copies cost little | +3 MiB V8 heap for 14 pages (4.4 MiB of envelopes) | Bounded per isolate (16 MiB or the cache budget) |
