# Optimization results

One optimization was made, for one established cause. Every number here
is local (see [BASELINE.md](BASELINE.md#local-reproduction-environment));
none is a Railway result.

- [Change discipline](#change-discipline)
- [RSC: reuse of decoded cache entries](#rsc-reuse-of-decoded-cache-entries)
- [Instrumentation overhead](#instrumentation-overhead)
- [Not changed](#not-changed)

## Change discipline

| Step | Done |
|---|---|
| Bottleneck documented before changing code | [RSC-LATENCY-INVESTIGATION.md](RSC-LATENCY-INVESTIGATION.md): per-request copy and decode of the whole cache entry, measured in metrics (323 KiB copied to JavaScript per RSC request) and in isolation (1.7–2.9 ms per request in V8) |
| Pre-change measurement preserved | `instr` = `2f43289`, the exact parent commit of the change; also `head` (`80f6d80`) and `rc2` |
| One focused change | `91a7729` only (cache handler and store; no serving-path change) |
| Correctness tests | Store unit tests (unchanged only while current; eviction and disk cases) and executor tests (reuse, never stale, a new value from any isolate wins, tags still expire a reused copy) |
| Re-measured under equivalent conditions | Same images, same build, same CPU sets; alternating order; two independent series (full matrix and a load-gated re-run) |
| CPU, memory, latency, throughput together | Below |

## RSC: reuse of decoded cache entries

Commit `91a7729`. Series: `benchmarks/results/series1/gated-20261126`
(3 rounds, each measured window started only below a host load average
of 10) unless marked. Medians, (min–max) across rounds.

### Closed loop, 8 connections (capacity and CPU)

| | `next start` | `instr` (before) | `optA` (after) | Change |
|---|---:|---:|---:|---:|
| Throughput (req/s) | 301 (298–315) | 222 (204–230) | **548 (464–605)** | **×2.47** |
| CPU per request (ms) | 5.02 (4.63–5.04) | 6.48 (6.37–7.06) | **2.98 (2.68–3.58)** | **−54 %** |
| Requests per CPU-second | 199 | 154 | **336** | ×2.18 |
| p50 (ms) | 23.0 | 33.2 | 12.9 | −61 % |
| p95 (ms) | 44.9 | 64.9 | 26.0 | −60 % |
| p99 (ms) | 81.9 | 124 | 45.6 | −63 % |
| Cache bytes copied to JavaScript per request | n/a | 323 KiB | 0 | |
| Lookups served from a reused copy | n/a | 0 | 1.00 per request | |
| V8 heap used at the end of the run | n/a | 104 MiB | 67.5 MiB | −35 % |

The full matrix (`matrix-20261126`, windows with host load ≥ 15 left out)
agrees: `optA` 622 req/s (542–703) at 2.67 ms against `instr` 206 req/s at
7.33 ms, `head` 218 req/s at 6.70 ms, `rc2` 208 req/s at 7.00 ms and
`next start` 338 req/s at 4.51 ms.

### Open loop (latency at a fixed rate)

| Rate | | `next start` | `instr` | `optA` |
|---|---|---:|---:|---:|
| 50 req/s (gated) | p50 / p95 / p99 (ms) | 6.23 / 47.7 / 336 | 7.54 / 24.0 / 88.3 | **5.62 / 16.0 / 81.5** |
| | CPU per request (ms) | 8.46 | 9.00 | 6.80 |
| 10 req/s (matrix) | p50 / p95 (ms) | 9.19 / 12.9 | 8.38 / 15.4 | **5.80 / 11.0** |
| 100 req/s (matrix) | p50 (ms) | 5.54 | 6.41 | **4.72** |

At these rates the CPU per request includes the process's background
work (timers, garbage collection), spread over few requests; the closed
loop shows the marginal cost.

At low and moderate load, the median RSC latency is now below `next
start`'s, the reverse of Series 1. The tails (p99 and above at 50 and 100
req/s) vary widely between rounds on both runtimes on this host and are
not ranked here.

### Memory

| | `next start` | `instr` | `optA` |
|---|---:|---:|---:|
| Peak RSS over the run (MiB) | 230 | 303 | **230** |
| RSS after 60 s idle (MiB) | 230 | 134 | 130 |
| Anonymous RSS after 60 s idle (MiB) | 172 | 93.1 | 88.3 |
| V8 heap used after 60 s idle (MiB) | n/a | 34.1 | 37.1 |

The reused copies cost about 3 MiB of V8 heap for the 14 pages
(`/` and 13 others, 4.4 MiB of stored envelopes). Removing half a
megabyte of garbage per RSC request lowered the peak by 73 MiB.

### Unchanged

- Static assets: same throughput and CPU within noise (4,252 against 4,067
  req/s closed loop; 0.44 against 0.46 ms per request).
- Response bytes: identical decoded bodies and headers to `instr` (0
  differences, `capture.mjs`).
- The first request for each page (cache seeding from the build): a few
  hundred milliseconds of CPU on both NextRS and `next start`, varying
  widely between rounds; see [FINAL-RECOMMENDATIONS.md](FINAL-RECOMMENDATIONS.md).

## Instrumentation overhead

`head` (`80f6d80`) against `instr` (`2f43289`, the metrics added), gated
series:

| | `head` | `instr` |
|---|---:|---:|
| RSC closed loop (req/s) | 244 (196–277) | 222 (204–230) |
| RSC CPU per request (ms) | 6.01 (5.24–7.63) | 6.48 (6.37–7.06) |
| Static closed loop (req/s) | 4,156 (3,602–5,213) | 4,067 (2,925–5,575) |
| Static CPU per request (ms) | 0.447 (0.365–0.506) | 0.459 (0.346–0.583) |

The ranges overlap; three rounds on this host cannot separate them. By
construction the added work is a handful of relaxed atomic additions and
two to four clock reads per request, plus one per streamed gzip slice:
well under a microsecond against 0.45 ms (static) and 6 ms (RSC). A small
overhead cannot be excluded by these measurements.

## Not changed

| Candidate | Why not in this milestone |
|---|---|
| Skip reading a static file when its gzip copy is cached | The read is wasted on every gzip hit (a new buffer of the file's size, then dropped). But a correct shortcut must reproduce every condition under which the compression step would not apply, and must never answer an internal subrequest (the image optimizer reads sources through the router) with gzip bytes. Static serving already costs about a tenth of `next start`'s CPU per request; the remaining gain is small against that risk. Candidate for a later, separately measured change. |
| Bound concurrent compression-cache fills | See [COMPRESSION-CACHE-INVESTIGATION.md](COMPRESSION-CACHE-INVESTIGATION.md). |
| Faster gzip | NextRS already uses flate2's zlib-rs backend (`nextrs-server` selects it; flate2 prefers it over miniz_oxide). Gzip of an RSC payload costs about 1.1–1.5 ms per response, as `next start` also pays it. Lowering the level would change wire bytes against `next start`. |
| Reuse in the `"use cache"` handler | Not on the measured path (used while rendering, not when serving a cached page). |
