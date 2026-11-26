# Series 1: baseline

What Velasia Airways' first Railway series measured, what it could not
show, and the controlled local baseline this investigation started from.

- [Series 1, reviewed](#series-1-reviewed)
- [Versions under comparison](#versions-under-comparison)
- [Local reproduction: environment](#local-reproduction-environment)
- [Correctness and parity](#correctness-and-parity)
- [Local baseline results](#local-baseline-results)

## Series 1, reviewed

Source: `nextrs-sh/velasia-airways`,
`benchmarks/railway/RAILWAY-CLOUD-BENCHMARK-REPORT.md` and
`benchmarks/railway/results/2026-11-25-series-1/` (summary CSV, charts,
per-run Railway service metrics). Run on 2026-11-25, 15:52–17:02 UTC.

| Setting | Value |
|---|---|
| Application | Velasia Airways `d04127a`, Next.js 16.4.6, same build fingerprint on both services |
| Runtimes | `next start`'s server (standalone `server.js`) on Node.js 24.18.0; NextRS 0.1.0-rc.2, one isolate |
| Railway | region `iad`, 1 replica, 2 vCPU / 1 GB each |
| Load | k6 2.3.0 from an in-region runner, through the public domains (Railway's edge included), constant arrival rate 2, 5, 10 requests/s, 30 s warm-up, 60 s measured, 2 rounds per level, order alternated; `Accept-Encoding: gzip` |
| Workloads | `static-assets` (the build's `/_next/static` JS and CSS referenced by `/` and `/destinations`), `rsc` (RSC payloads of 14 pages) |
| Validity | 12 of 12 pairs comparable; API idle; no failed checks; k6 ≤ 0.12 cores |

### What the data supports

- **Static assets:** NextRS had the lower p50 in 6 of 6 pairs (3.9–4.7 ms
  against 5.3–6.2 ms) and the lower p95 in 6 of 6.
- **RSC payloads:** `next start` had the lower p50 in 6 of 6 pairs
  (5.7–7.0 ms against 6.6–7.9 ms) and the lower TTFB p95 in 5 of 6
  (NextRS TTFB p95 9.8–12.6 ms against 8.2–10.7 ms). The bytes received
  were within 0.3 % in every pair, so both sent the same payloads.
- The direction is the same in both orders at every rate.

### What it cannot support

- **Any capacity or efficiency claim.** At 2–10 requests/s both used under
  3 % of 2 vCPU; Railway's CPU samples are 30 s apart (two per run).
- **Memory comparisons.** Railway's memory figure is the container's
  (including file cache), sampled every 30 s; NextRS rose from about 129
  to 230 MiB and `next start` from about 89 to 220 MiB over the series.
  Neither the split (heap, allocator, file cache) nor whether it would
  plateau is in the data.
- **The 343.8 ms p99.** One NextRS static run (10/s, round 2, NextRS
  first): 603 requests, so p99 is the 6th-slowest request. Its p95 (11.0
  ms) and p50 (4.1 ms) were normal. No NextRS metrics were collected, so
  the series cannot say whether it was a cache fill, a stall, or the
  network.
- **Statistical strength.** Two rounds, 60 s each.

### Not stated by Series 1, but checked here

The RSC workload was assumed to exercise "prerendered RSC payloads". In
this build every one of the 14 pages is a Partial Prerendering page with
a revalidation time, which NextRS hands to Next's handler in V8 (see
[RSC-LATENCY-INVESTIGATION.md](RSC-LATENCY-INVESTIGATION.md)). Series 1's
RSC numbers therefore compared `next start` with NextRS's **executor**
path, not with its Rust static path.

## Versions under comparison

| Label | What | Commit | Build |
|---|---|---|---|
| `node` | `next start`'s server: Next.js 16.4.6 standalone `server.js`, Node.js 24.18.0 | Velasia `d04127a` | `Dockerfile.node` |
| `rc2` | NextRS 0.1.0-rc.2 from npm (binary SHA-256 `87d9d871…`) | `fe7a53b` (tag `v0.1.0-rc.2`) | release workflow, `dist` profile, Rust 1.98.0, Ubuntu 22.04 |
| `head` | NextRS at the start of this work (version 0.1.0-rc.3, not released) | `80f6d80` | `dist` profile, Rust 1.98.0, Debian 12 (this investigation) |
| `instr` | `head` + serving-stage metrics | `2f43289` | as `head` |
| `optA` | `instr` + reuse of decoded cache entries | `91a7729` | as `head` |

Between `rc2` and `head` the runtime changed in one place on the serving
path: `If-Match` / `If-Unmodified-Since` on static and public files
(`2705d87`). The rest is `nextrs doctor` (CLI only), documentation and
dependency patch updates (`url` 2.5.9). Performance work merged after
rc.2: none on the serving path (the rc.3 compression-cache work was
measurement and documentation, `PERF-1`).

## Local reproduction: environment

| Item | Value |
|---|---|
| Host | Intel Core i5-8259U (4 cores, 8 threads, 2.3 GHz), 8 GB, macOS 15 (Darwin 24.6.0) |
| Containers | Docker Desktop 29.7.2, Linux VM with 8 vCPUs and 3.8 GiB, kernel 7.0.12-linuxkit |
| Storefront under test | `--cpus 2`, `--cpuset-cpus 2,3`, 1 GiB, no swap (Series 1: 2 vCPU, 1 GB); one at a time |
| Load generator | `nextrs-loadgen` (this repository), CPUs 4–5, HTTP/1.1 keep-alive |
| Shared API | Velasia retailing API `d04127a`, CPU 6; idle (neither workload calls it) |
| Application | Velasia Airways `d04127a`, `next build` once, BUILD_ID `gvWJrvYSM71x059u5PMn2`, served by every runtime |
| Node.js | 24.18.0 (`node:24.18.0-bookworm-slim`), Next.js 16.4.6 standalone `server.js` |
| NextRS builds | Rust 1.98.0, `cargo build --locked --profile dist` (fat LTO, one codegen unit, as the release), Debian 12; rc.2 is the npm binary |
| Isolates | 1 (as Series 1) unless stated |
| Compression | gzip (`Accept-Encoding: gzip` on every request), as Series 1 |
| Load models | open loop at fixed rates (latency from the scheduled time) and closed loop (8 or 16 connections, capacity); reported separately |
| Rounds | 3, runtime order rotated (round 1 as listed, round 2 reversed, …) |

The host was **shared with other work** during this investigation (other
builds and test suites). The host's 1-minute load average ranged from
about 3 to over 300. Two measures keep the comparisons fair:

1. Every window records the host and VM load around it. The full matrix
   (`matrix-20261126`) is summarized with windows at a host load of 15 or
   more left out (57 of 180 windows, listed in its `summary.md`).
2. A second, focused series (`gated-20261126`) started each measured
   window only once the host's load average was below 10.

Where the two series disagree, the gated series is reported. Latency
tails (p99 and above) remain sensitive to this host and are not ranked.
`perf` is not available in this VM (no PMU events), so attribution comes
from NextRS's own metrics and from A/B builds, not from profiles.

## Correctness and parity

`benchmarks/series1/capture.mjs` recorded every request of both workloads
(19 static files and 14 RSC paths, gzip and identity), three HTML pages,
HEAD, `If-None-Match` and range requests, from each runtime. Results in
`benchmarks/results/series1/capture-20261126/`.

| Against `next start` | Decoded bodies equal | Status equal | Content-Encoding equal | Other differences |
|---|---|---|---|---|
| `rc2`, `head`, `instr`, `optA` | 77 of 77 | 77 of 77 | 77 of 77 | `Accept-Ranges: bytes` absent on gzip static responses (18) |

- The one header difference is deliberate and predates this work: once a
  response is gzip-encoded, NextRS removes `Accept-Ranges` (a byte range
  of the identity file cannot apply to the gzip representation);
  `next start` keeps it.
- NextRS's gzip output is 0.6–0.8 % larger than `next start`'s (zlib-rs
  against zlib, both at level 6).
- `instr` against `optA`: no difference at all.
- RSC responses carry the same `x-nextjs-cache: HIT`,
  `cache-control: s-maxage=3600, stale-while-revalidate=82800`, `Vary`
  and `x-nextjs-stale-time` on both runtimes.

## Local baseline results

Before any change (`rc2`, `head`), gated series unless stated, medians
across rounds. The after-change results are in
[OPTIMIZATION-RESULTS.md](OPTIMIZATION-RESULTS.md).

| | `next start` | NextRS `rc2` (matrix) | NextRS `head` |
|---|---:|---:|---:|
| Static, closed loop (req/s) | 391 | 3,653 | 4,156 |
| Static CPU per request (ms) | 4.57 | 0.51 | 0.45 |
| Static p50 / p95 at 100 req/s (ms) | 5.23 / 33.7 | 2.23 / — | 2.34 / 38.4 |
| RSC, closed loop (req/s) | 301 | 208 | 244 |
| RSC CPU per request (ms) | 5.02 | 7.00 | 6.01 |
| RSC p50 / p95 at 50 req/s (ms) | 6.23 / 47.7 | — | 7.61 / 29.4 |
| Idle RSS (MiB) | 102 | 67.1 | 68.0 |
| Peak RSS (MiB) | 230 | 284 | 302 |
| RSS after 60 s idle (MiB) | 230 | 132 | 134 |
| Idle CPU (millicores) | 42 | 20 | 19 |

The local baseline reproduces Series 1's direction on both workloads:
static files faster on NextRS, RSC payloads slower. It also shows what
Series 1 could not: static serving costs NextRS about a tenth of `next
start`'s CPU, and RSC payloads cost it about 20–40 % more, which
[RSC-LATENCY-INVESTIGATION.md](RSC-LATENCY-INVESTIGATION.md) explains.

`rc2` and `head` are indistinguishable within this host's noise on every
measure, as expected from the code: no serving-path change other than
the static-file preconditions was made between them.
