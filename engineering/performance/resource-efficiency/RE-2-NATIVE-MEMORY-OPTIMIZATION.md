# RE-2: native memory optimization

Status: implemented on `feat/re-2-3-memory`, opt-in; measured in the
screening, the 3-round key matrix, the wide matrix and the final binary's confirmation
([RE-23-COMBINED-BENCHMARKS](RE-23-COMBINED-BENCHMARKS.md)). Preliminary:
one Docker Desktop VM on a shared Mac. Soak tests run last (30 min, 2 h and 6 h, on a dedicated
Fedora Linux machine,
[RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md)).

Mechanism and allocator choice: [RE-2-ALLOCATOR-RETENTION-ANALYSIS](RE-2-ALLOCATOR-RETENTION-ANALYSIS.md),
[RE-2-ALLOCATOR-COMPARISON](RE-2-ALLOCATOR-COMPARISON.md).

## What changed

PR #50 (`65397c6`, on `main` since rc.3) introduced the reclamation
mechanism: an isolate-thread reclaim request served between event-loop
polls (V8 low-memory notification, then `malloc_trim(0)`), a server-side
reclaimer with idle and pressure triggers, `--malloc-arena-max` and the
`balanced` / `low-memory` profiles. This milestone changes when it fires
and what it reports, not how it collects.

| Change | Where | Why |
|---|---|---|
| **Idle means quiet, not silent.** Idle after at most 0.3 requests/s over the window (≥ 3), counted on arrival; re-armed only above twice that rate; the first period is measured from startup | `nextrs-server/src/reclaim.rs`, `http_metrics.rs` (`arrivals`) | Velasia's image `HEALTHCHECK` (every 10 s) kept the RE-1 reclaimer from ever seeing 10 s of silence; every real platform probes. Probes must neither prevent nor repeat reclamation (review M2) |
| **Pressure backoff** only when a reclamation did not bring the working set below the threshold; doubling up to 8× (80 s), reset below the threshold | `reclaim.rs` | A full container of live data must not trigger a collection every 10 s (review L1) |
| **cgroup located once** and read through the resolved path (`memory.max` / `memory.high` up the hierarchy) | `nextrs-runtime/src/memory.rs` | the pressure trigger and the heap budget share one definition of the limit ([cgroup report](RE-3-CGROUP-RESOURCE-DETECTION.md)) |
| **Watchdog heartbeat** before and after a reclamation's collection | `nextrs-runtime/src/worker.rs` | a long low-memory GC must not look like a stalled isolate (review L5) |
| **Arena cap removed from `low-memory`** (was 2) | `nextrs-cli/src/main.rs` | two arenas retained more memory, not less |
| **Metrics:** resident anonymous memory before, after the V8 collections, after the trim; pressure backoffs | `reclaim.rs` (Prometheus and `/metrics.json`) | `mallinfo` counts trimmed pages as free, so only resident memory shows what a trim returned |

No allocator was added or replaced, nothing is collected per request, and
the defaults are unchanged: reclamation stays off unless
`--memory-reclaim-idle`, `--memory-pressure-reclaim` or a profile turns
it on.

## Results (candidate = `--js-heap-limit auto --memory-reclaim-idle 10 --memory-pressure-reclaim 90`)

Single-factor runs (one round, `full` workload) separate the two RE-2/3
mechanisms; MiB:

| 256 MiB / 1 vCPU | Peak | Anon after 300 s | Outcome |
|---|---:|---:|---|
| `base` | 254.7 | 137.8 | OOM-killed in 2 of 3 rounds |
| heap budget only | 169.4 | 103.5 | survived |
| **idle trim only** | 252.6 | – | **OOM-killed** |
| both | 167.5 | 86.5 | survived (3 of 3) |

| 512 MiB / 1 vCPU | Peak | After 10 s | Anon after 300 s | Closed-loop req/s |
|---|---:|---:|---:|---:|
| `base` | 296.4 | 167.3 | 156.4 | 106 (r1) |
| heap budget only | 181.1 | 140.3 | 106.2 | 104 |
| idle trim only | 293.9 | 91.0 | **78.9** | 120 |
| both | 182.4 | 113.9 | 77.1 | 113 |

- **The trim is the after-load fix:** with or without the heap budget it
  brings the process down to 73–80 MiB anonymous at every size measured
  (128 MiB to 1 GiB), against 135–163 for `base` (allocator retention
  63–88 MiB).
- **It does nothing for the peak.** It runs only after the load, so
  without the heap budget the process still OOMs at 256 MiB. The peak is
  the heap budget's job ([RE-3](RE-3-V8-HEAP-BUDGETING.md)).
- **Cost:** 0.24–0.43 s of one core per idle period at 1 vCPU, 0.6–1.5 s
  at 0.5 vCPU, twice per run (after startup, after the load). The first
  request after a reclamation ran normally; no request waited for one in
  these runs (it happens only when quiet).
- **Pressure reclamation** fired once in the 13 runs of the combined candidate: in the 128 MiB
  container, where it is the only margin. It does not substitute for the
  budget: without one (`re2-native`, screening) it ran 3 times under load
  at 256 MiB for 7.2 s of collection and halved throughput.
- **Idle memory** improves as a side effect: the post-startup reclamation
  collects build-loading garbage, 20–21 MiB anonymous at idle against
  30–31 for `base` and 46–48 for Node.js.

## Not done, and why

| Option | Decision |
|---|---|
| Replacing glibc (jemalloc, mimalloc) | rejected on memory ([comparison](RE-2-ALLOCATOR-COMPARISON.md)) |
| Arena cap by default | rejected: worse retention |
| Periodic trimming under load | not needed (retention is returned when quiet; under load the heap budget bounds the peak) and it would add latency to requests |
| Lower `M_MMAP_THRESHOLD` / `M_TRIM_THRESHOLD` | not pursued: the trim already returns ~all retained memory; changing thresholds changes every allocation's cost |
| Buffer reuse in static file and cache paths | the source of the churn (audit), but a code change beyond what the result needs; candidate for RE-5 |
| Reclamation per request or GC per request | excluded by the milestone rules |

## Open points

- Regrowth after the trim at 2 vCPU (79 → 101 MiB over 290 s with only
  probes, 1 GiB / 2 vCPU, one run): the load/idle cycles and soaks show
  whether it accumulates.
- Idle detection is a rate, so a site with steady low traffic (above
  0.6 requests/s) never counts as idle and never trims; pressure
  reclamation is its safety net.
- glibc only. musl or other libcs: the functions are no-ops (`cfg`).
