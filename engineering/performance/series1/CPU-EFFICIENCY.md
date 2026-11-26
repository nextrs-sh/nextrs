# CPU efficiency

How much CPU each runtime spends per request on the two Series 1
workloads, measured from the storefront container's own cgroup. Local
measurements only (see [BASELINE.md](BASELINE.md#local-reproduction-environment)).

- [Method](#method)
- [Results](#results)
- [Where NextRS's CPU goes](#where-nextrss-cpu-goes)
- [Cost modelling](#cost-modelling)
- [Limits of these numbers](#limits-of-these-numbers)

## Method

- **CPU time:** `cpu.stat` (`usage_usec`, `user_usec`, `system_usec`,
  `nr_throttled`) of the storefront container, read immediately before
  and after each measured window. Everything the container ran in that
  window is counted (garbage collection, timers, compression), which is
  what a host bills for.
- **Per request:** CPU time divided by successful requests. In a
  **closed** loop the server is kept busy, so this is close to the marginal
  cost. In an **open** loop at a modest rate it also carries the process's
  background work divided over fewer requests.
- **Idle:** 30 s with no traffic, 15 s after readiness.
- Series: `benchmarks/results/series1/gated-20261126` (3 rounds, every
  window started below a host load average of 10). `optA` is NextRS with
  the change of [OPTIMIZATION-RESULTS.md](OPTIMIZATION-RESULTS.md); `instr`
  is its parent commit.
- Context switches are not reported: `/proc/1/status` counts the main
  thread only, which says nothing about a multi-threaded server.

## Results

### Static assets (`/_next/static`, gzip)

| | `next start` | NextRS `instr` | NextRS `optA` |
|---|---:|---:|---:|
| CPU per request, closed loop (ms) | 4.57 (3.34–4.93) | 0.459 | 0.438 |
| of which user / system (ms) | 4.12 / 0.45 | 0.24 / 0.22 | 0.23 / 0.21 |
| Requests per CPU-second | 219 | 2,177 | 2,285 |
| Response MiB per CPU-second | 3.6 | 36.4 | 38.2 |
| CPU per request at 100 req/s (ms) | 6.41 | 1.94 | 1.84 |
| CPU used at 100 req/s (cores) | 0.62 | 0.19 | 0.18 |

`next start` compresses every static response again (its `compression`
middleware has no cache); NextRS compresses each file once and serves the
gzip copy afterwards. This is the largest efficiency difference measured
in this work, about **ten times** less CPU per static request. Series 1
could not see it: at 10 requests/s both used almost no CPU.

### RSC payloads (14 pages, gzip)

| | `next start` | NextRS `instr` | NextRS `optA` |
|---|---:|---:|---:|
| CPU per request, closed loop (ms) | 5.02 (4.63–5.04) | 6.48 (6.37–7.06) | **2.98 (2.68–3.58)** |
| of which user / system (ms) | 4.44 / 0.59 | 5.78 / 0.88 | 2.70 / 0.29 |
| Requests per CPU-second | 199 | 154 | **336** |
| CPU per request at 50 req/s (ms) | 8.46 | 9.00 | 6.80 |
| CPU used at 50 req/s (cores) | 0.41 | 0.43 | 0.33 |
| Gzip per response, NextRS metric (ms) | n/a | 1.68 | 1.43 |

Before the change NextRS spent about 29 % **more** CPU than `next start`
per RSC request; after it, about 41 % **less**.

### Idle

| | `next start` | NextRS (`head`, `instr`, `optA`) |
|---|---:|---:|
| Idle CPU (millicores) | 42.0 (41.4–62.8) | 17.6–18.7 |
| Time to ready from container start (ms) | 3,288 | 2,031–2,893 |

### Throttling

No CFS throttling of note in any measured window (0–2 periods out of
thousands): no result above was shaped by the 2-CPU quota.

## Where NextRS's CPU goes

From NextRS's metrics during the gated RSC runs (`optA`):

| Part | Per RSC request |
|---|---|
| Served by | Next's handler in the isolate (100 % `via="executor"`) |
| Cache lookup | One store lookup answered `unchanged`; no bytes copied |
| Gzip of the payload (51 KiB on average, one frame) | 1.3–1.7 ms (`nextrs_compression_stream_seconds_total`) |
| Everything else (HTTP, routing, isolate dispatch, Next's handler) | the rest of the 2.7–3.6 ms |

Gzip is now about half of NextRS's CPU per RSC request. `next start`
pays the same compression per request with C zlib; NextRS uses the
zlib-rs backend at the same level.

For static files NextRS's 0.44 ms is split about evenly between user and
system time: one blocking-pool task per request opens, stats and reads the
file, and hyper writes 17 KiB of gzip. The file read is wasted on a cached
gzip copy (see [OPTIMIZATION-RESULTS.md](OPTIMIZATION-RESULTS.md#not-changed)).

## Cost modelling

This is a method, not a forecast. Cloud CPUs, networks and noisy
neighbours differ from this host; repeat the measurements on the target
platform before using any number.

- **CPU needed at a target rate** ≈ rate × closed-loop CPU per request +
  idle CPU. Example on this host, 100 RSC req/s plus 300 static req/s:
  `next start` 0.50 + 1.37 + 0.04 ≈ 1.9 cores; NextRS `optA` 0.30 + 0.13 +
  0.02 ≈ 0.45 cores.
- **Sustainable rate in a fixed allocation:** the closed-loop throughput
  at 2 CPUs (RSC 301 against 548 req/s; static 391 against 4,252 req/s),
  read together with the latency the open-loop runs show below it.
- **Memory needed:** see [MEMORY-EFFICIENCY.md](MEMORY-EFFICIENCY.md):
  peak and post-load RSS, not idle RSS, size a container.
- **Price:** only with the provider's stated rates applied to measured
  usage. Railway's published rates used in Series 1 were $20 per
  vCPU-month and $10 per GB-month of actual use; lower RSS is not lower
  billing unless the platform bills that memory.

No savings percentage is claimed for any cloud deployment from these
numbers.

## Limits of these numbers

- One host (Intel Core i5-8259U, 4 cores / 8 threads, in Docker Desktop's
  Linux VM) with other work on it; contended windows were gated out or
  left out, not corrected.
- One application build and two workloads. Dynamic SSR, Server Actions and
  route handlers were not measured here.
- One isolate (as Series 1). More isolates add throughput for executor
  routes (Phase 3A), not for static files.
