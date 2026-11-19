# Benchmarks

Measurements of NextRS against the official Next.js server (`next start`),
with their method, conditions and raw data.

> [!IMPORTANT]
> Results are specific to the tested workloads, hardware, and runtime
> configurations. They are not guarantees of production performance.

- [README chart (NextRS f5c480d, 2026-11-18)](#readme-chart-nextrs-f5c480d-2026-11-18)
- [Full matrix (Phase 3A, 2026-11-16)](#full-matrix-phase-3a-2026-11-16)
  - [Environment](#environment)
  - [Method](#method)
  - [Results](#results)
  - [Where NextRS is not ahead](#where-nextrs-is-not-ahead)
  - [Reproducing](#reproducing)
- [Historical results (Phase 1A, 2026-11-03)](#historical-results-phase-1a-2026-11-03)

## README chart (NextRS f5c480d, 2026-11-18)

The README chart comes from the Phase 3C final run: dynamic App Router SSR
(`/books/[id]`). It compares `next start` with NextRS at `f5c480d` (a pre-release build that
reported version 0.0.0; the first release candidate is v0.1.0-rc.1) at 1
and 4 isolates. Next.js is 16.4.6; the environment and method are those of
the full matrix below. Each figure is the median of 3 interleaved rounds.

| | `next start` | NextRS · 1 isolate | NextRS · 4 isolates |
|---|---:|---:|---:|
| Throughput, closed loop | 611 req/s | 736 req/s | 1,898 req/s |
| p95, open loop at 305 req/s | 6.26 ms | 3.30 ms | |
| Anonymous memory after load | 376 MiB | 141 MiB | |
| CPU per request | 1.92 ms | 1.75 ms | |

Raw data: [`benchmarks/results/phase3c/final-bench-f5c480d/readme/`](../benchmarks/results/phase3c/final-bench-f5c480d/readme/).
The same run also re-measured ten of the matrix workloads against the
Phase 3A and 3B builds: [`fixtures/`](../benchmarks/results/phase3c/final-bench-f5c480d/fixtures/)
(`compare-*.md`).

The chart was drawn with:

```sh
node benchmarks/phase3a/chart.mjs benchmarks/results/phase3c/final-bench-f5c480d/readme \
  --workload app/ssr --configs next-start,nextrs-i1,nextrs-i4 \
  --raw-url benchmarks/results/phase3c/final-bench-f5c480d/readme
```

## Full matrix (Phase 3A, 2026-11-16)

All 24 workloads, measured at Phase 3A (`6c1101d`, Next.js 16.4.3). The
earlier README charts came from this run.

Raw data: [`benchmarks/results/phase3a/matrix-final-6c1101d/`](../benchmarks/results/phase3a/matrix-final-6c1101d/)
(`rows.jsonl` holds one row per cell, with every round;
`environment.json` records the conditions; `summary.md` and the
`compare-*.md` files hold the tables with bootstrap intervals).

### Environment

| | |
|---|---|
| Date | 2026-11-16 |
| NextRS | `6c1101d` (release profile, default allocator) |
| Next.js | 16.4.3, the same `.next` build served by both servers |
| Node.js | 24.18.0 (for `next start`) |
| CPU | Intel Core i5-9400F, 6 cores, `performance` governor, turbo on |
| Memory | 16 GiB, no cgroup limit |
| Kernel | Linux 7.3 |
| Placement | server on CPUs 0–3, load generator on CPUs 4–5, one host |
| Load generator | `benchmarks/loadgen` (Rust, HTTP/1.1 keep-alive); checks every response body |

### Method

- **Configurations.**
  - `next start`.
  - NextRS with 1, 2 and 4 V8 isolates.
  - NextRS at `964995c` (the Phase 2G release) with 1 isolate, as the
    baseline.

  Isolates are parallelism within the same 4 CPUs. NextRS with 1 isolate
  runs JavaScript on one thread, as `next start` does.
- **Runs.**
  - 3 rounds per workload.
  - Configurations run interleaved, in a shuffled order each round.
  - Each run uses a new server process.
  - Medians are reported. Differences count only when the 95 % bootstrap
    interval of the ratio excludes 1.
- **Closed loop:** 32 connections, 5 s warm-up, 10 s measured. Used for
  throughput, CPU per request and memory.
- **Open loop:** a fixed arrival rate, the same for every configuration,
  equal to half of the slowest configuration's closed-loop throughput.
  Latency is coordinated-omission-aware. Used for p95 latency.
- **Correctness checks.** Each response is checked against an expected
  body. A run with any failed check, or failed oracle preflight, is
  invalid. All 720 cells of this run were valid.
- **Load-limited cells (⚑).** On the three cheapest workloads the load
  generator saturates before NextRS, so those figures are lower bounds.
- **CPU** is process-tree user plus system time, divided by the number of
  requests.
- **Anonymous RSS** is the process tree's anonymous memory after load. It
  excludes file-backed pages of the binary and of `node_modules`.

### Results

Medians of 3 rounds. ⚑: load generator limited, so req/s is a lower bound.

| Workload | `next start` req/s | NextRS 1 isolate req/s | NextRS 4 isolates req/s | CPU ms/req `next start` → NextRS 1 | open-loop p95 ms `next start` → NextRS 1 | anon RSS MiB `next start` → NextRS 1 |
|---|---:|---:|---:|---|---|---|
| app/static-file | 9,831 | 68,368 | 68,014 | 0.127 → 0.053 | 0.41 → 0.22 | 268 → 25 |
| app/prerendered | 3,117 | 121,896 ⚑ | 122,034 ⚑ | 0.335 → 0.022 | 0.64 → 0.30 | 294 → 23 |
| app/ssr | 654 | 781 | 2,074 | 1.800 → 1.642 | 5.25 → 2.89 | 348 → 156 |
| app/ssr-identity | 705 | 886 | 2,256 | 1.650 → 1.473 | 4.37 → 2.88 | 359 → 146 |
| app/rsc | 1,022 | 1,485 | 4,434 | 1.098 → 0.830 | 1.52 → 1.35 | 400 → 146 |
| app/streaming | 499 | 481 | 503 | 1.953 → 1.954 | 64.35 → 64.42 | 416 → 140 |
| app/action | 1,260 | 2,587 | 4,756 | 0.949 → 0.634 | 4.72 → 1.39 | 426 → 145 |
| app/action-multipart | 1,387 | 2,585 | 6,009 | 0.809 → 0.564 | 1.27 → 1.06 | 430 → 147 |
| app/isr-hit | 2,753 | 4,841 | 10,916 | 0.379 → 0.293 | 0.68 → 0.79 | 432 → 140 |
| app/isr-regenerating | 2,999 | 5,782 | 12,631 | 0.350 → 0.253 | 0.64 → 0.73 | 432 → 144 |
| cache-components/use-cache-hit | 477 | 614 | 1,396 | 2.516 → 2.150 | 6.62 → 6.81 | 369 → 165 |
| proxy/proxy-matched-ssr | 509 | 666 | 1,476 | 2.327 → 2.028 | 3.56 → 3.37 | 373 → 153 |
| proxy/proxy-matched-static | 1,485 | 6,244 | 11,482 | 0.739 → 0.312 | 1.14 → 0.73 | 413 → 141 |
| proxy/proxy-unmatched | 2,789 | 124,465 ⚑ | 121,663 ⚑ | 0.354 → 0.028 | 0.67 → 0.32 | 415 → 49 |
| auth/jwt-valid | 456 | 598 | 1,428 | 2.707 → 2.239 | 4.10 → 3.25 | 581 → 155 |
| auth/jwt-rejected | 3,383 | 7,532 | 15,579 | 0.315 → 0.249 | 0.61 → 0.56 | 922 → 147 |
| pages/pages-ssr | 1,484 | 1,722 | 3,645 | 0.780 → 0.820 | 3.12 → 5.11 | 292 → 120 |
| pages/pages-ssr-proxied | 947 | 1,261 | 2,900 | 1.192 → 1.152 | 1.55 → 1.91 | 333 → 138 |
| pages/pages-api-get | 4,012 | 8,056 | 17,011 | 0.267 → 0.187 | 0.57 → 0.54 | 356 → 126 |
| pages/pages-api-post | 1,870 | 3,909 | 7,354 | 0.606 → 0.411 | 0.95 → 0.90 | 356 → 132 |
| pages/pages-data | 1,629 | 2,823 | 6,418 | 0.675 → 0.544 | 1.01 → 1.01 | 357 → 135 |
| image/image-hit | 7,700 | 76,303 ⚑ | 94,202 ⚑ | 0.141 → 0.036 | 2.15 → 0.19 | 273 → 21 |
| image/image-transform | 85 | 56 | 57 | 45.523 → 36.370 | 50.53 → 36.58 | 396 → 33 |
| mixed/mixed | 1,200 | 2,093 | 2,259 | 0.997 → 1.257 | 6.00 → 7.48 | 397 → 156 |

Against the Phase 2G baseline (`964995c`), with 1 isolate:
- Pages SSR throughput: +21.5 %.
- App SSR throughput: +11.9 %.
- ISR throughput: +54–65 %.
- Instructions per request on JavaScript workloads: 10–53 % lower.

Significant regressions against the baseline (95 % interval excludes no
change):
- **Pages SSR p99, open loop.**
  - Behind the proxy: 4.9 → 6.5 ms (+34 %), in all three rounds. The p95
    did not get worse (2.04 → 1.91 ms). The cause is not yet known.
  - Without the proxy: +7.9 %.
- **Small throughput regressions, closed loop.**
  - Streaming: −1.3 %.
  - Image transformation: −2.4 %, with unchanged instructions per request.
    At an equal offered rate both are unchanged.
- **Memory.**
  - Peak RSS is 2–10 % higher on several workloads. End-of-run anonymous
    memory is within noise on most, and +1.5–3.4 % on Pages data and API
    POST.
  - The open-loop peak on proxy-unmatched is about 70 MiB higher, during
    the run only. End-of-run anonymous memory is unchanged (66–81 vs
    64–73 MiB).
- **Mixed closed-loop p99.** It doubles. See
  [the mixed workload](#where-nextrs-is-not-ahead) below.

All comparisons are in
[`compare-base-vs-current-i1-closed.md`](../benchmarks/results/phase3a/matrix-final-6c1101d/compare-base-vs-current-i1-closed.md)
and the `-open` variant.

### Where NextRS is not ahead

- **Image transformation (`image-transform`).** `next start` completes more
  transforms per second (85 vs 56). NextRS limits concurrent
  transformations to bound memory: anonymous RSS is 33 MiB against
  396 MiB. At an equal offered rate, NextRS has the lower p95 (36.6 vs
  50.5 ms).
- **Pages SSR latency at low load.** `/ssr/whoami` awaits
  `setTimeout(resolve, 0)` in `getServerSideProps`. At 708 req/s, NextRS's
  p50 is about 1 ms higher (3.5 vs 2.5 ms), and its p95 is 5.1 vs 3.1 ms,
  with equal CPU per request. The extra time comes before the first byte.
  The likely cause, not yet confirmed, is that a NextRS timer fires about
  a millisecond later than Node's. The Pages SSR page behind the proxy
  does not wait on a timer and is within 0.1 ms of `next start` at p50.
  Its p95 is higher (1.91 vs 1.55 ms).
- **Small p95 differences.** ISR hits, `use cache` hits and Pages SSR
  behind the proxy have a p95 0.1–0.4 ms higher than `next start` at equal
  load.
- **Mixed workload.**
  - CPU per request in the closed loop is higher: 1.26 vs 1.00 ms.
  - NextRS completes more of everything, including image work that
    `next start` serves more slowly.
  - Closed-loop p99 is dominated by image transforms waiting for the two
    transformation slots.
  - At an equal offered rate, image-miss p99 is unchanged from the
    baseline: 14.2 ms on `964995c`, 14.4–15.0 ms on `6c1101d`.
- **Streaming.** The page waits on timers for about 64 ms, so throughput is
  equal across servers.
- **Isolate scaling.**
  - 4 isolates reach 2.1–3.6× the 1-isolate throughput on rendering
    workloads, not 4×.
  - The isolates share 4 CPUs with the HTTP server threads and with V8's
    GC helpers.
  - Scavenge pauses roughly double at 4 isolates.

### Reproducing

Requires Linux, Rust, Node.js 24, `perf` and `taskset`. Run from the
repository root.

```sh
# 1. Build the NextRS binary under test and the load generator
cargo build --release -p nextrs-cli
cargo build --release --manifest-path benchmarks/loadgen/Cargo.toml

# 2. Build the benchmark fixtures (same .next output for both servers)
benchmarks/phase3a/build-fixtures.sh

# 3. Run the matrix
#    Optional: add a baseline build with --nextrs-b PATH --label-b NAME --isolates-b 1
node benchmarks/phase3a/run.mjs --out benchmarks/results/phase3a/my-run \
  --nextrs target/release/nextrs --label current --isolates 1,2,4 \
  --rounds 3 --seed 21

# 4. Tables and charts
node benchmarks/phase3a/summarize.mjs benchmarks/results/phase3a/my-run
node benchmarks/phase3a/compare.mjs benchmarks/results/phase3a/my-run --a next-start --b current-i1
node benchmarks/phase3a/chart.mjs benchmarks/results/phase3a/my-run \
  --workload app/ssr --configs next-start,current-i1,current-i4 --out /tmp
```

The full matrix takes about 3.5 hours. Use `--only ssr,pages-ssr` to run a
subset. Set the CPU governor to `performance` and keep the machine
otherwise idle.

## Historical results (Phase 1A, 2026-11-03)

> [!NOTE]
> Kept for the record. These figures were measured at the Phase 0 and Phase
> 1A stage (commit `b43a255`) and are superseded by the results above. Do
> not compare them with the current numbers: the load generator and the
> method differ.

### Summary

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

### Environment

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

### Method

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

### Dynamic rendering results

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

### Static serving results

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

### Limitations

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

### Reproducing

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

### Other benchmark data

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

### Benchmark drivers

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
