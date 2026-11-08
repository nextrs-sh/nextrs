# Benchmarks

Measurements of NextRS against the official Next.js server (`next start`),
with their method, conditions and raw data.

> [!IMPORTANT]
> **These are historical, preliminary results.** The figures below were
> measured on 2026-11-03, at the Phase 0 and Phase 1A stage of NextRS. They
> have **not** been re-measured on the current commit, and NextRS has changed
> a lot since then (isolate pool, caching, Server Actions, ISR, proxy,
> images). They describe one small fixture on one machine. They are not a
> measure of production performance, and they do not show that NextRS is
> faster than `next start` for other applications or workloads.

- [Summary](#summary)
- [Environment](#environment)
- [Method](#method)
- [Dynamic rendering results](#dynamic-rendering-results)
- [Static serving results](#static-serving-results)
- [Limitations](#limitations)
- [Reproducing](#reproducing)
- [Other benchmark data](#other-benchmark-data)
- [Benchmark drivers](#benchmark-drivers)

## Summary

The README chart shows one workload: dynamic server-side rendering of
`/books/42` (an App Router page with a route parameter) at 32 concurrent
keep-alive requests, median of 3 runs.

| Metric | `next start` | NextRS | Source field |
|---|---:|---:|---|
| Throughput | 453.4 req/s | 647.9 req/s | `median.requests_per_second` |
| p95 latency | 79.89 ms | 58.49 ms | `median.p95_ms` |
| Process-tree RSS after load | 268.8 MiB | 222.8 MiB | `median.rss_mib_after` |
| Server CPU per request | 2.38 ms | 1.88 ms | `median.server_cpu_ms_per_request` |

Source: [`benchmarks/results/dynamic-20261103T233743.json`](../benchmarks/results/dynamic-20261103T233743.json),
entries with `"path": "/books/42"` and `"concurrency": 32`. The charts
([`nextrs-benchmarks.svg`](../.github/assets/nextrs-benchmarks.svg), and
[`nextrs-benchmarks-mobile.svg`](../.github/assets/nextrs-benchmarks-mobile.svg)
for narrow screens) are drawn from these values, with bars starting at zero.

## Environment

| | |
|---|---|
| Date | 2026-11-03 |
| NextRS | Phase 1A, commit `b43a255` (results committed with it); one V8 isolate (the isolate pool did not exist yet) |
| Next.js | 16.4.3, React 19.3.1 ([`fixtures/next-app`](../fixtures/next-app/)) |
| Node.js | v22.20.0 |
| Rust | rustc 1.98.0, release build |
| Load generator | ApacheBench 2.3 (`ab -k`), on the same host |
| CPU | Intel Core i5-9400F, 6 cores / 6 logical CPUs |
| Memory | 16 GB |
| OS | Fedora, Linux 7.2.14 |

Each result file records its own environment under `"environment"`.

## Method

Both servers serve **the same `.next` build** of `fixtures/next-app`, on the
same machine, one after another.

- **Equivalence gate:** before measuring, every path is fetched from every
  server. A path is measured only if each server answers `200` and the body
  is byte-identical to `next start`'s. `/dynamic` depends on timing even
  under `next start`, so it is checked for status and content markers
  instead. The script refuses to benchmark otherwise.
- **Requests:** `ab -k` with keep-alive, a fixed `Cookie` and a probe header,
  no compression. 4,000 requests per run (400 for `/dynamic` at concurrency
  1, which waits about 60 ms per request on timers).
- **Warmup:** one unmeasured `ab` run of 1/8 of the requests (at least 50)
  before each measurement.
- **Repetitions:** 3 measured runs per path, concurrency and server. Tables
  report the median, and the range of throughput across the runs.
- **Concurrency:** 1 and 32.
- **Latency:** percentiles from `ab -e`, which reports sub-millisecond
  values.
- **CPU per request:** `utime + stime` of the server's whole process tree
  (from `/proc/<pid>/stat`), sampled before and after the measured run,
  divided by the number of requests. The load generator's CPU is not
  included.
- **Memory:** resident set size of the server's whole process tree (from
  `/proc/<pid>/statm`, every descendant of the started process). Three
  points are reported:
  - **After first render (cold start):** a freshly started server, 1 second
    after its first dynamic response; median of 5 starts.
  - **Idle** (static benchmark only): a freshly started server, 1 second
    after it first answers `/`; median of 5 starts.
  - **After load:** after the last measured run of a path. Servers are not
    restarted between paths, so this includes everything allocated by
    earlier paths.

Reference hosts (a minimal Node.js host and the Deno CLI calling the same
compiled handlers) were measured in the same run and are in the JSON file.
They are omitted here because they are not deployable servers.

## Dynamic rendering results

NextRS with `--js-runtime embedded-v8` (one isolate), against `next start`.
Same build, same equivalence gate.

| Path | Conc. | Server | req/s (median) | req/s range | p50 ms | p95 ms | p99 ms | CPU ms/req | RSS after (MiB) |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|
| `/books/42` | 32 | `next start` | 453.4 | 435.7–459.2 | 69.74 | 79.89 | 88.27 | 2.38 | 268.8 |
| `/books/42` | 32 | NextRS | 647.9 | 569.6–656.5 | 48.26 | 58.49 | 69.20 | 1.88 | 222.8 |
| `/request-info` | 32 | `next start` | 458.3 | 448.0–459.1 | 68.40 | 79.08 | 90.92 | 2.34 | 269.1 |
| `/request-info` | 32 | NextRS | 681.2 | 635.1–710.9 | 45.92 | 52.77 | 64.42 | 1.76 | 214.6 |
| `/api/health` | 32 | `next start` | 1,497.2 | 1,383.5–1,531.2 | 21.37 | 22.45 | 24.66 | 0.70 | 312.1 |
| `/api/health` | 32 | NextRS | 3,705.9 | 2,885.1–3,933.8 | 8.29 | 13.22 | 14.98 | 0.42 | 205.8 |
| `/dynamic` | 32 | `next start` | 412.4 | 403.0–413.9 | 76.75 | 82.56 | 98.62 | 2.71 | 299.3 |
| `/dynamic` | 32 | NextRS | 466.5 | 465.2–467.8 | 66.60 | 77.89 | 96.28 | 2.45 | 208.7 |
| `/books/42` | 1 | `next start` | 418.8 | 365.4–440.4 | 2.24 | 3.52 | 4.03 | 2.56 | 267.4 |
| `/books/42` | 1 | NextRS | 438.9 | 437.7–453.2 | 2.09 | 3.02 | 3.98 | 2.43 | 172.7 |
| `/request-info` | 1 | `next start` | 426.3 | 392.2–441.7 | 2.13 | 3.44 | 5.31 | 2.67 | 198.2 |
| `/request-info` | 1 | NextRS | 395.5 | 391.1–429.8 | 2.35 | 3.22 | 4.96 | 2.66 | 181.8 |
| `/api/health` | 1 | `next start` | 1,320.2 | 1,126.6–1,377.0 | 0.68 | 0.91 | 1.78 | 0.77 | 305.1 |
| `/api/health` | 1 | NextRS | 1,876.0 | 1,752.2–1,961.1 | 0.48 | 0.61 | 0.77 | 0.54 | 184.8 |
| `/dynamic` | 1 | `next start` | 14.8 | 14.7–14.9 | 67.46 | 68.90 | 69.93 | 17.18 | 317.8 |
| `/dynamic` | 1 | NextRS | 14.6 | 14.6–14.7 | 68.32 | 69.64 | 70.29 | 12.92 | 177.0 |

`/books/42` and `/request-info` are server-rendered pages, `/api/health` is a
Route Handler, and `/dynamic` streams a Suspense boundary that resolves after
a timer (its throughput at concurrency 1 is bounded by that timer). At
concurrency 1, `/request-info` was slower under NextRS. All runs had 0
failed requests.

**Cold start** (median of 5 starts):

| Server | Ready (ms) | First `/books/42` response (ms) | RSS after first render (MiB) |
|---|---:|---:|---:|
| `next start` | 381 | 442 | 122.9 |
| NextRS | 113 | 219 | 61.7 |

## Static serving results

A separate workload: prerendered and hashed static responses, which NextRS
answers from Rust without JavaScript. Measured on 2026-11-03 at Phase 0
(commit `2aa011e`), when NextRS had no JavaScript runtime. 20,000 requests
per run at concurrency 32, 2,000 warmup requests, 3 runs, median. In this
benchmark `ab` reported whole-millisecond percentiles.

| Path | Server | req/s (median) | req/s range | p50 ms | p99 ms | CPU s per 1k req | RSS after (MiB) |
|---|---|---:|---:|---:|---:|---:|---:|
| `/` (prerendered HTML) | `next start` | 2,448 | 2,213–2,462 | 13 | 25 | 0.42 | 256.4 |
| `/` (prerendered HTML) | NextRS | 139,438 | 129,874–140,810 | 0 | 1 | 0.01 | 9.3 |
| `/_next/static/…js` | `next start` | 7,211 | 6,948–7,212 | 4 | 7 | 0.17 | 257.7 |
| `/_next/static/…js` | NextRS | 42,380 | 37,654–43,869 | 1 | 2 | 0.09 | 12.2 |

| Server | Startup to first 200 (ms) | Idle RSS (MiB) |
|---|---:|---:|
| `next start` | 383 | 120.8 |
| NextRS (no JavaScript runtime) | 6 | 7.2 |

Source: [`benchmarks/results/baseline-20261103T222608.json`](../benchmarks/results/baseline-20261103T222608.json).
At these rates the single-host load generator is a large share of the
measurement; treat the static figures as an upper bound on serving overhead,
not as a comparable throughput number. The Phase 0 binary did not include
V8 at all; a current NextRS with `--js-runtime embedded-v8` uses more memory.

## Limitations

- **Historical:** NextRS has gained an isolate pool, a shared cache, ISR,
  Server Actions, `proxy.ts` and image optimization since these runs. Their
  effect on these numbers has not been measured against `next start` with
  this benchmark.
- **One machine, one small fixture:** a developer desktop, and pages far
  simpler than a real application. Results do not transfer
  to other hardware, applications or traffic patterns.
- **Shared host:** `ab` and the server run on the same 6 CPUs, so the load
  generator competes with the server.
- **Single process each:** one `next start` process and one NextRS process
  with one isolate. Neither was tuned, and neither ran behind a reverse proxy
  or with compression.
- **Few repetitions:** 3 runs per point. The throughput ranges above show the
  spread; NextRS's `/books/42` runs varied by about 13%.
- **Not production performance:** no TLS, no real network, no database or
  upstream services, no long-running traffic.

## Reproducing

The scripts are unchanged since these results were recorded. They need
`ab` (ApacheBench), Python 3, Node.js and a release build of NextRS.

```bash
cargo build --release -p nextrs-cli
(cd fixtures/next-app && npm ci && npx next build)

# Dynamic rendering (writes benchmarks/results/dynamic-<timestamp>.json)
python3 benchmarks/dynamic_bench.py --project fixtures/next-app --out benchmarks/results
```

`--deno PATH` adds the Deno reference host; `--repeats`, `--requests` and
`--concurrency` change the load. To reproduce the historical figures exactly,
check out `b43a255` (dynamic) or `2aa011e` (static) first.

`benchmarks/bench.py` starts NextRS without the JavaScript runtime. Since the
fixture gained Server Actions and ISR, current NextRS refuses to start it that
way, so the static benchmark only runs at its original commit.

Run benchmarks on an otherwise idle machine, and compare results only within
one run.

## Other benchmark data

Raw results in [`benchmarks/results`](../benchmarks/results/), all from the
same machine:

| File | Date | Contents |
|---|---|---|
| [`baseline-20261103T222608.json`](../benchmarks/results/baseline-20261103T222608.json) | 2026-11-03 | Static serving, Phase 0 (above) |
| [`dynamic-20261103T233743.json`](../benchmarks/results/dynamic-20261103T233743.json) | 2026-11-03 | Dynamic rendering, Phase 1A (above) |
| [`pool-20261104T094436.json`](../benchmarks/results/pool-20261104T094436.json), [`pool-20261104T095506.json`](../benchmarks/results/pool-20261104T095506.json) | 2026-11-04 | Isolate pool sizes (1–6 isolates) at concurrency 1–128 |
| [`actions-20261105T143955.json`](../benchmarks/results/actions-20261105T143955.json) | 2026-11-05 | Server Actions: small and multipart calls, uploads, no-JavaScript forms |
| [`phase2b-20261107T135425.json`](../benchmarks/results/phase2b-20261107T135425.json) | 2026-11-07 | Real-world proxies (`jose`, `next-intl`, external rewrites) |

Each file contains its configuration and every individual run.

## Benchmark drivers

Drivers in [`benchmarks/`](../benchmarks/) run `next start`, NextRS and
reference hosts against the same build. Each path's responses must be
equivalent before it is measured.

| Driver | Measures |
|---|---|
| [`bench.py`](../benchmarks/bench.py) | Static and prerendered serving |
| [`dynamic_bench.py`](../benchmarks/dynamic_bench.py) | Dynamic rendering |
| [`pool_bench.py`](../benchmarks/pool_bench.py) | Isolate pool sizes |
| [`phase1c_bench.py`](../benchmarks/phase1c_bench.py) | Outbound fetch and the data cache, warm and cold |
| [`actions_bench.py`](../benchmarks/actions_bench.py) | Server Actions |
| [`phase1e_bench.py`](../benchmarks/phase1e_bench.py) | Cache hits, ISR regeneration under load, revalidation, `"use cache"` |
| [`phase2a_bench.py`](../benchmarks/phase2a_bench.py), [`phase2b_bench.py`](../benchmarks/phase2b_bench.py), [`phase2c_bench.py`](../benchmarks/phase2c_bench.py) | Proxy, routing rules and real-world proxy applications |
| [`image_bench.mjs`](../benchmarks/image_bench.mjs), [`image_faults.mjs`](../benchmarks/image_faults.mjs), [`image-bench.sh`](../benchmarks/image-bench.sh) | `/_next/image` throughput, latency, fault handling and memory-admission accounting, against `next start` or NextRS from an empty cache |
| [`image_memory.mjs`](../benchmarks/image_memory.mjs), [`image-memory.sh`](../benchmarks/image-memory.sh) | Peak and retained memory (RSS, malloc statistics, reservations) during bursts of large and mixed image work |
| [`image-corpus.mjs`](../benchmarks/image-corpus.mjs), [`estimate-calibration.sh`](../benchmarks/estimate-calibration.sh) | A codec corpus, and the memory estimate checked against measured peaks |
| [`image_soak.mjs`](../benchmarks/image_soak.mjs), [`image-soak.sh`](../benchmarks/image-soak.sh), [`image_soak_summary.py`](../benchmarks/image_soak_summary.py) | Sustained image traffic beside SSR, Server Actions, ISR and `proxy.ts`, with injected restarts and a failing cache disk |
| [`cold_bench.mjs`](../benchmarks/cold_bench.mjs) | The cold data-cache path: every request misses, fetches upstream and stores |
| [`young_gen_bench.py`](../benchmarks/young_gen_bench.py) | V8 young-generation sizing under sustained SSR |
| [`slow_clients.py`](../benchmarks/slow_clients.py) | Memory with paused clients |
| [`soak.mjs`](../benchmarks/soak.mjs), [`soak-matrix.sh`](../benchmarks/soak-matrix.sh), [`proxy-soak.sh`](../benchmarks/proxy-soak.sh), [`soak_summary.py`](../benchmarks/soak_summary.py) | Sustained mixed traffic with injected isolate stalls and restarts; RSS, swap and V8 heap after full GC |
| [`recovery.mjs`](../benchmarks/recovery.mjs) | Recovery time after an isolate failure |
