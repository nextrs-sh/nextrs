# Series 1: conclusions and recommendations

**Verdict: IMPROVED — VALIDATED (locally).** One reproducible bottleneck
was found on the path Series 1 measured, fixed with one focused change,
and re-measured under equivalent conditions, with byte-identical
responses and the repository's checks passing
([REGRESSION-VALIDATION.md](REGRESSION-VALIDATION.md)). Nothing here is a
cloud result: Railway Series 2 must confirm it.

- [Before and after](#before-and-after)
- [What was established](#what-was-established)
- [What remains](#what-remains)
- [Railway Series 2](#railway-series-2)

## Before and after

Local, one isolate, 2 CPUs and 1 GiB per server, the same Velasia build
for every runtime (`gated-20261126`, 3 rounds, medians; ranges in the
linked pages). "NextRS baseline" is the development HEAD at the start of
this work (`80f6d80`, rc.2's serving code); "NextRS optimized" is
`91a7729`.

| Metric | `next start` (Node.js 24.18.0) | NextRS baseline | NextRS optimized |
|---|---:|---:|---:|
| Static throughput, closed loop, 16 connections | 391 req/s | 4,156 req/s | 4,252 req/s |
| RSC throughput, closed loop, 8 connections | 301 req/s | 244 req/s | **548 req/s** |
| Static p95 at 100 req/s | 33.7 ms | 38.4 ms | 6.1 ms ¹ |
| RSC p95 at 50 req/s | 47.7 ms | 29.4 ms | **16.0 ms** |
| CPU per request, static (closed loop) | 4.57 ms | 0.447 ms | 0.438 ms |
| CPU per request, RSC (closed loop) | 5.02 ms | 6.01 ms | **2.98 ms** |
| Idle memory (process RSS) | 102 MiB | 68.0 MiB | 68.3 MiB |
| Peak memory (process RSS) | 230 MiB | 302 MiB | **230 MiB** |
| Memory after 60 s recovery (process RSS) | 230 MiB | 134 MiB | 130 MiB |
| Idle CPU | 42 millicores | 19 millicores | 18 millicores |

¹ The static path did not change; static p95 varied between 3.5 and 38.8
ms across rounds on both NextRS builds with host contention. It is not an
improvement of this work.

## What was established

1. **Series 1's RSC requests ran JavaScript on NextRS.** Velasia's 14
   pages are Partial Prerendering pages with ISR; NextRS sends them to
   Next's handler in V8 (`nextrs_http_served_total{via="executor"}` = 100 %
   of RSC requests). The Series 1 comparison was `next start` against
   NextRS's executor, not against its Rust static path.
2. **Root cause of the RSC gap:** NextRS's cache handler copied,
   transferred, parsed and base64-decoded the whole stored entry (305–489
   KiB) on every request; `next start` reuses a decoded object. Fixed by
   reusing each isolate's decoded copy while the store vouches for its
   generation: RSC throughput ×2.47 against the parent commit, CPU per
   request −54 %, peak memory −24 %, now ahead of `next start` on all
   three locally.
3. **Static serving costs NextRS about a tenth of `next start`'s CPU,**
   because NextRS compresses each file once and `next start` compresses
   every response. Series 1's static latency advantage is a consequence of
   that; at 10 requests/s it could not show the CPU difference.
4. **No memory leak.** Both runtimes grow under load; NextRS gives most of
   it back after load (to about 130 MiB), `next start` keeps its peak.
5. **Compression cache:** correct, coalesces concurrent misses, and does no
   duplicate work. It is not a plausible cause of the 343.8 ms p99, which
   stays unexplained.
6. **Observability:** NextRS now reports what answered each request,
   routing and head time per source, streamed-gzip work, cache bytes
   copied to JavaScript, reused entries and compression-cache fills.

## What remains

Ordered by expected value. Each needs its own measured change.

| # | Issue | Evidence | Suggested next step |
|---|---|---|---|
| 1 | **Isolates multiply memory, and more isolates than CPUs hurt** | +150–175 MiB peak per isolate under RSC load; 4 isolates peaked at 728 MiB in a 1 GB container; unwarmed 4-isolate RSC runs on 2 CPUs were far slower than 1 isolate | Document sizing (isolates ≤ CPUs; memory per isolate), and measure warmed multi-isolate throughput on 2 and 4 CPUs before recommending more than one isolate on small instances |
| 2 | **Gzip dominates RSC CPU** | 1.1–1.7 ms of encoder time per 51 KiB RSC response (about half of NextRS's CPU per RSC request); `next start` pays the same | Out of NextRS's hands while matching `next start`'s level; an application behind a compressing CDN can turn it off (`compress: false`) |
| 3 | **Static requests make two blocking-pool hops** | Routing (with `canonicalize` + `metadata` on the blocking pool) is 0.6–1.9 ms of a 0.9–3.1 ms static response head at 100 req/s; then the file is opened and read in a second hop | Resolve and read in one blocking task, keeping the routing order and the containment checks; measure CPU and p50 |
| 4 | **A gzip hit still reads the file** | The identity bytes are read and dropped on every cache hit | Only together with (3), with the internal-subrequest and header conditions handled explicitly |
| 5 | **First request per page is expensive** | Cold RSC requests cost 30–450 ms of CPU each on NextRS (seeding the shared store from the build, first decode, JIT), and 13–320 ms on `next start`; highly variable | Measure per isolate and per page after start; consider seeding response entries at startup |
| 6 | **glibc retains freed memory** | 32–93 MiB free in malloc arenas after recovery | Bound arenas (`M_ARENA_MAX`) or trim after load; rc.3 `PERF-1` follow-up |
| 7 | **Unbounded concurrent compression fills for different files** | Only matters under eviction churn (rc.3 `PERF-1`); not reproducible with Velasia | Cap concurrent fills and fall back to streaming compression when the cap is reached; test with the rc.3 churn soak |
| 8 | **`"use cache"` entries are parsed per read** | Same pattern as the fixed handler, on the render path | Same reuse, if a render-heavy workload shows it |

## Railway Series 2

Validate, with the optimized build (`91a7729` or a release containing it)
against `next start`, on the same deployment pair as Series 1:

1. **Same workloads, more evidence:** `static-assets` and `rsc`, at least
   5 rounds per level, 120 s per run, order alternated. Keep 2, 5, 10
   requests/s for continuity and add 50 and 100 requests/s: the RSC
   difference is a CPU difference and shows more clearly with load.
2. **Collect NextRS's metrics around every run** (the operations listener
   is container-local: scrape it from inside the container, as Series 1
   did for the API): `nextrs_http_served_total{via}`,
   `nextrs_http_served_head_seconds`, `nextrs_http_routing_seconds`,
   `nextrs_cache_reused_total`, `nextrs_cache_value_bytes_total`,
   `nextrs_compression_*`, `nextrs_process_*`. Expected: RSC
   `via="executor"` with reused lookups ≈ requests and ≈ 0 value bytes per
   request after warm-up.
3. **CPU and memory from inside each container** at 1 s resolution
   (cgroup `cpu.stat`, `memory.current`, `memory.stat`), not Railway's
   30 s samples; report CPU per request.
4. **A recovery window** (≥ 60 s idle) after the series, then memory.
5. **A capacity step** for each workload in a fixed allocation (open-loop
   rates stepped up until p95 degrades), still far below the API's limit
   and with the API idle.
6. **Tail attribution:** for any p99 outlier, compare the client's latency
   with `nextrs_http_served_head_seconds` from the same window; a tail
   absent from the server's head times is outside NextRS.
7. **Version skew:** confirm both storefronts serve the same build
   fingerprint before and after every pair, as Series 1 did.

What Series 2 should not claim without its own evidence: a general
"faster than Node.js" result (one application, two workloads), any cost
saving in dollars, or results for dynamic SSR, Server Actions or route
handlers, which none of these series measured.
