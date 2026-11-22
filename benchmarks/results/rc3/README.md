# rc.3 benchmark results: compression cache under churn (PERF-1)

These runs investigate the rc.2 observation that a 1 MiB compression cache
facing a ~3.4 MB gzip working set was slower than a disabled cache, and that
memory grew during the 15-minute churn soak. The question was whether
rc.2's churn result is expected cache behaviour or a resource-management
problem.

## Setup

- **Host:** Intel i5-8259U (4 cores, 8 threads), Docker Desktop VM with 8
  vCPUs and 3.9 GiB, Debian 12 container, kernel 7.0.12-linuxkit. rc.2's
  results came from a different machine (i5-9400F, 6 cores), so compare
  ratios between the two, not absolute numbers.
  - The server is pinned to CPUs 0-3 and the load generator to CPUs 4-5.
    These are hyperthreads of one laptop CPU, so the run-to-run spread is
    large; see the interleaved runs below.
- **Binaries** (`binaries.sha256`): the published `v0.1.0-rc.2` release
  binary, and the CI `dist` build of rc.3 (PR #44, whose tree is identical
  to `2705d87`).
  - `compress.rs` and `compress_cache.rs` are byte-identical between rc.2
    and rc.3.
- **Projects:**
  - `fixtures/next-app` (Next 16.4.6, built in `node:22.20.0`): 21 static
    files, 183 KiB gzip.
  - The churn project is the same fixture with
    `benchmarks/rc3/make-churn-project.mjs` applied: 141 static files,
    11.6 MB identity, 3.59 MB gzip (`{"static_files":141,"identity_bytes":11623618,"gzip_bytes":3594279}`).
    That matches the working set rc.2's churn run reported.
- **Soak:** `benchmarks/rc3/churn-soak.mjs`.
  - The request mix is rc.2's full mix (`spec-churn-full.json`): every
    static JS/CSS file, plus prerendered, SSR, RSC, JSON API, the largest
    chunk, a range of it, and a streamed page. All requests send
    `Accept-Encoding: gzip`.
  - 64 keep-alive connections, closed loop, 60 s segments.
  - Every 5 s the harness samples the process (RSS, threads, CPU, faults)
    and `/metrics.json`. The metrics include the compression cache and
    glibc's `mallinfo2`.

## Short A/B runs

`benchmarks/rc2/compression.mjs`, 3 rounds of 10 s, medians. µs/req is server
CPU time per request.

`cache-variants/` uses `fixtures/next-app`, whose working set fits in
1 MiB:

| Workload | rc.3, 32 MiB | rc.3, `--compression-cache 0` | rc.3, 1 MiB |
|---|---|---|---|
| static-mix req/s (µs/req) | 6,623 (406) | 1,052 (3,333) | 5,200 (536) |
| prerendered | 40,842 (72) | 6,306 (581) | 24,048 (115) |
| large-static | 2,451 (1,262) | 285 (13,976) | 2,700 (997) |

`cache-variants-churn/` uses the churn project:

| Workload | rc.2, 32 MiB | rc.3, 32 MiB | rc.3, `0` | rc.3, 1 MiB |
|---|---|---|---|---|
| static-mix req/s (µs/req, peak RSS) | 11,510 (288, 90 MiB) | 12,689 (263, 93 MiB) | 1,136 (3,310, 91 MiB) | **526** (6,408, **265 MiB**) |
| prerendered | 41,191 | 46,272 | 5,033 | 43,582 |
| large-static | 6,351 | 6,656 | 402 | 5,147 |

Notes:

- In `compression.mjs` the cache counters are cumulative per server process
  across workloads. The "hit%" in its summary is therefore not the
  static-mix hit ratio. Use the soaks for that.
- On static files alone, the undersized cache was 54% slower than a
  disabled one in these short rounds. rc.2 measured 9% on its machine.
- In a 70 s run after a 30 s warm-up, the gap was 32% (`perf/`: 924 vs
  1,356 req/s, p99 130 vs 52 ms).

## Soaks (`soak/`)

All rows use the full mix, 64 connections. Throughput and latency are the
median of the last 5 segments. CPU is cores used over the last 5 minutes.

| Run | Duration | req/s | p50 / p99 ms | µs/req | RSS at 1 min → end | Threads | malloc in use / free at end | Hit ratio | Failures |
|---|---|---|---|---|---|---|---|---|---|
| B0: rc.2, 1 MiB | 15 min | 1,416 | 43.8 / 96.0 | 2,527 | 511 → 744 MiB | 137 → 165 | 29 / 590 MiB | 0.9% | 0 |
| B1: rc.3, 1 MiB | 90 min | 1,309 | 47.4 / 106.8 | 2,693 | 479 → 869 MiB | 127 → 169 | 29 / 730 MiB | 1.7% | 0 |
| B2: rc.3, 1 MiB, `MALLOC_ARENA_MAX=2` | 20 min | 1,338 | 46.8 / 101.9 | 2,587 | 454 → 586 MiB | 116 → 130 | 27 / 437 MiB | 0.6% | 0 |
| B3: rc.3, 32 MiB (default) | 20 min | 10,709 | 1.4 / 217.1 | 342 | 203 → 218 MiB | 82 → 108 | 21 / 37 MiB | 100% | 0 |
| B4: rc.3, cache off (`0`) | 20 min | 1,160 | 52.2 / 151.0 | 3,213 | 171 → 188 MiB | 54 → 72 | 11 / 45 MiB | – | 0 |
| B5: rc.3, 1 MiB, `MALLOC_MMAP_THRESHOLD_=65536` | 15 min | 782 | 75.1 / 246.1 | 4,236 | 302 → 459 MiB | 118 → 135 | 23 / 312 MiB | 1.1% | 0 |

B3's p99 comes from the deliberately slow streamed page in the mix, not
from static files (p50 1.4 ms).

B1's resident memory, by period:

| Period | RSS | Rate | Threads |
|---|---|---|---|
| 1–20 min | 479 → 727 MiB | 13.1 MiB/min | 127 → 161 |
| 20–45 min | 727 → 782 MiB | 2.2 MiB/min | 161 → 164 |
| 45–70 min | 782 → 836 MiB | 2.2 MiB/min | 164 → 168 |
| 70–90 min | 836 → 867 MiB | 1.6 MiB/min | 168 → 169 |

Through all 90 minutes:

- Throughput did not decline: 829–1,423 req/s per segment, with no downward trend.
- No request failed.
- The memory malloc reports in use stayed between 13 and 36 MiB, and was 29 MiB at the end.
- The cache recorded 6,063,394 misses and 107,238 hits. Evictions matched
  misses.

## Findings

1. **Not a regression.** The compression code is unchanged since rc.2, and
   rc.2 under the same churn (B0) shows the same memory curve.
2. **Not a leak in the live heap.** Live heap stays flat at about 30 MiB.
   What grows is memory the allocator has freed and keeps: glibc
   `free_bytes` rose from 319 to 730 MiB. `malloc_trim` cannot report it,
   because `releasable_bytes` measures only the top of the main arena.
3. **Why:** an undersized cache misses on almost every gzip request.
   - A miss over 64 KiB compresses the whole body on the blocking thread
     pool. With 64 connections that pool reaches about 160 threads, each
     allocating zlib state and buffers in its own glibc arena.
   - With the cache off (B4), compression runs streaming on the 4 runtime
     workers, and memory stays flat at about 185 MiB.
   - Limiting glibc to 2 arenas (B2) cuts the growth and the page faults
     (about 60× fewer), but free memory still reaches 437 MiB.
   - Mapping allocations of 64 KiB and more directly (B5) still leaves
     312 MiB free, with fewer requests and more faults. The retained memory
     is therefore small allocations fragmented across many threads.
4. **CPU.** Both profiles (`perf/`) are about 80% zlib-rs deflate. The
   undersized cache adds kernel time (16.7% vs 8.8%, mostly thread
   wake-ups, which fits the handoff to the blocking pool).
   - On the full mix, the undersized cache costs no more CPU per request
     than the cache off (2.7 vs 3.2 ms). On static files alone it is
     32–54% slower.
5. **No plateau within 90 minutes.** Growth slowed from 13 MiB/min to
   about 1.6 MiB/min, but it had not stopped. Whether it would stop was not
   established.
6. **The default configuration is unaffected.** With 32 MiB (B3) the set
   fits, the hit ratio is 100%, and memory stays flat at about 218 MiB.

No code change was made for rc.3. `engineering/configuration.md`
(Response compression, "Sizing the cache") and `engineering/operations.md`
describe how to recognise an undersized cache and size it. A possible
follow-up is to bound concurrent cache fills: past a limit, a miss would
compress streaming, as with the cache off. That would need its own
measurement.
