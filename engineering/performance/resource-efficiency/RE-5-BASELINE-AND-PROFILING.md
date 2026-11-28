# RE-5A: baseline and profiling

This report pins the RE-4.1 baseline, profiles where native allocation
and CPU go per route class, and compares the RE-5 candidate. **Every
number is process level on one Fedora host**, not a container
measurement. The native Linux campaign stays the authority for
end-to-end CPU and latency.

## 1. Identity

| | Baseline | Candidate |
|---|---|---|
| Commit | `8d83d03` (main, RE-4.1 merged as PR #57) | `8017043` (`feat/re-5-allocation-cpu-efficiency`) |
| Binary sha256 (`--release`, `nextrs`) | `54daca719a771f788979837c5ee0d498e34ed9624c8ece95209186c9049a2c79` | `0b2340ec37dd7e26726b43f0f2dff06fabafc57f1b702f899b65d014856ebe2e` |
| Profiling build (line tables) | `target/re5-baseline-prof`, same commit, `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` | – |

**Intermediate candidates.** Two earlier candidate builds are named in
the reports that used them:

- `2845852…` (through `1ddb050`): RE-5B;
- `d0bc4b6…` (`1ddb050` + the heartbeat): RE-5F.

**Toolchain:** rustc 1.98.0 (builds and tests); clippy also on 1.100.0.

**Pinned dependencies (Cargo.lock):**

- deno_core 0.413.0, serde_v8 0.322.0;
- v8 crate 150.4.0 (`deno_v8` 0.5.0);
- tokio 1.54.1, hyper 1.12.0, axum 0.8.9, reqwest 0.13.5;
- flate2 1.1.10 with zlib-rs 0.6.8, multer 3.1.0.

**Host:**

- Fedora 45, kernel 7.3.4;
- Intel i5-9400F (6 cores, no SMT), 15.9 GiB RAM;
- inside the VS Code Flatpak sandbox. `perf` cannot open events there
  (`perf_event_paranoid` 2), so no sampling profiler was available.

**Node.js:** 24.18.0 for fixture builds and drivers (CI pins 22.20.0).

**Fixtures:**

- fixtures/next-app, rebuilt for RE-5: one Server Action added;
- the Phase 3A fixtures;
- fixtures/next-prerender-scale (RE-5B).

**RE-4.1 CI:** run 46130319082 on main `8d83d03`, every job passed: fmt,
clippy, tests, the RustSec audit, the compatibility suites pinned at 1
and 4 isolates, and 16.4.5. The PR's own compatibility jobs were still
running when it was merged; the post-merge run on main covers them.

## 2. Allocation per request

`benchmarks/phase3a/alloc.mjs`:

- an `LD_PRELOAD` counter (`alloc-count.c`) over glibc's allocation
  entry points, so it counts Rust and C++ (V8) heap allocations, not the
  V8 JavaScript heap;
- 1 isolate, 8 connections, 3 000 requests after as many warm-up, every
  response checked;
- counting slows malloc, so these runs are never used for timing;
- one run per build.

| Workload | Allocations / request (baseline → candidate) | Bytes / request |
|---|---|---|
| app/static-file | 45.1 → 45.1 | 23 946 → 24 100 |
| app/static-gzip | 53.1 → 53.1 | 24 510 → 24 668 |
| app/prerendered | **31 → 27 (−12.9 %)** | 16 949 → 16 350 (−3.5 %) |
| app/prerendered-gzip | **40 → 36 (−10.0 %)** | 17 355 → 16 756 (−3.5 %) |
| app/ssr | 521.7 → 520.9 | 463 818 → 465 061 |
| app/ssr-gzip | 523.8 → 529.1 | 877 254 → 835 326 |
| app/ssr-identity | 468.2 → 462.0 | 397 482 → 351 210 |
| app/rsc | 396.1 → 406.2 | 259 622 → 311 950 |
| app/streaming | 524.7 → 524.8 | 503 512 → 497 211 |
| app/action | 344.6 → 343.4 | 126 555 → 122 338 |
| app/action-multipart | 255.5 → 254.7 | 119 983 → 115 605 |
| app/isr-hit | 201.9 → 195.2 | 69 573 → 76 341 |
| app/isr-regenerating | 160.6 → 159.5 | 76 126 → 76 080 |
| cache-components/use-cache-hit | 591.1 → 603.3 | 472 484 → 473 070 |
| proxy/proxy-matched-ssr | 933.3 → 929.7 | 465 137 → 460 203 |
| proxy/proxy-matched-static | 489.9 → 486.0 | 68 002 → 67 532 |
| proxy/proxy-unmatched | 109 → 105 | 20 977 → 20 378 |
| auth/jwt-valid | 904.0 → 915.6 | 458 380 → 472 441 |
| auth/jwt-rejected | 265.3 → 265.7 | 48 934 → 48 054 |
| pages/pages-ssr | 1 029.8 → 1 028.8 | 155 515 → 158 159 |
| pages/pages-ssr-proxied | 1 373.1 → 1 372.4 | 200 381 → 199 876 |
| pages/pages-api-get | 167.0 → 166.7 | 45 996 → 45 505 |
| pages/pages-api-post | 386.1 → 385.9 | 93 562 → 95 586 |
| pages/pages-data | 484.0 → 483.3 | 86 421 → 85 250 |
| image/image-hit | 176 → 176 | 21 435 → 21 595 |
| image/image-transform | invalid in both builds (the load generator's checks failed; harness, not measured) | |
| mixed/mixed | 402.7 → 402.2 | 535 171 → 536 424 |

**Reading the table:**

- **Prerendered pages:** 4 allocations fewer per request (`c721cb2`: the
  Vary and content-type values made once, and the header map sized
  once).
- **Everything else** is within the run-to-run variation of a single
  run. On JavaScript routes the byte counts are dominated by V8's own
  activity and vary with GC and JIT timing (§2.1).
- **Not visible in these workloads:**
  - the multipart change (`38e5659`): their bodies are a few hundred
    bytes;
  - the cache change (`287bf3e`): it needs `--cache-dir`.

### 2.1 Run-to-run variation

A second run of three JavaScript workloads, per build (`results/re5/alloc-repeat/`):

| Workload | Baseline run 1 → run 2 (bytes / calls) | Candidate run 1 → run 2 |
|---|---|---|
| ssr | 463 818 → 458 646 / 521.7 → 508.9 | 465 061 → 465 588 / 520.9 → 520.4 |
| rsc | 259 622 → 259 715 / 396.1 → 381.1 | 311 950 → 265 261 / 406.2 → 396.7 |
| isr-hit | 69 573 → 79 658 / 201.9 → 196.9 | 76 341 → 79 695 / 195.2 → 195.7 |

**Variation:**

- On JavaScript routes, the same build varies by up to ±15–20 % in
  bytes and ±4 % in calls between runs. This is V8's GC and JIT timing.
- The apparent +20 % RSC bytes of §2 is that variation: the second
  candidate run is within 2 % of the baseline.
- The prerendered-page reduction (−4 calls, exactly) is deterministic:
  the path has no JavaScript.

## 3. Where native allocation comes from

`benchmarks/phase3a/alloc-sites.mjs`:

- sampled backtraces of every 16th allocation call
  (`alloc-trace.c`, glibc `backtrace`, which stops at JIT frames);
- 2 000 requests;
- attributed to the first NextRS frame on the stack.

The baseline profiling build was used. Raw data: `results/re5/alloc-sites/`
(text reports, JSON, folded stacks).

| Workload | Calls / request | No NextRS frame (V8 internal) | Largest NextRS frames |
|---|---|---|---|
| static-file | 37.2 | – | `file_response` 6.6 calls (17.6 %), `static_files::resolve` 3.7 |
| prerendered | 30.3 | – | **`prerendered` 12.5 calls (41.3 %)**, `decode_uri_component` 3.1 |
| ssr | 515.7 | **393.5 calls, 417 KB of 464 KB (76 % of calls)** | `dynamic::render` 32.3 calls / 2 KB; the isolate's poll loop 16.2 / 7.8 KB; `JsRequest` serialization 10.1; `op_nextrs_response_head` 8.7; `ResponseSender::write` 5.1 / 4.8 KB |
| rsc | 404.8 | 280.5 calls (69 %), 266 KB | `dynamic::render` 27.4 |
| action | 339.3 | 91.4 calls (27 %), 67 KB | the outbound fetch to the test upstream (`net::send`) 44.1 / 10.5 KB |

**Conclusions:**

- **Static and prerendered paths** allocate about 30–45 times per
  request, spread across hyper, axum and http. The one concentrated site,
  `prerendered`, was reduced (§2).
- **JavaScript routes:** 70–76 % of allocation calls and about 90 % of
  bytes come from V8 itself: compiler zones, GC worklists, remembered
  sets, code space. NextRS's own share is about 120 calls and 30 KB per
  SSR request, spread over request conversion, op serialization and
  response chunks.
- **The brief's target** of 15–30 % fewer native bytes per representative
  request is therefore **not reachable from NextRS code on JavaScript
  routes**. On prerendered pages calls fell 13 % and bytes 3.5 %.
- **No speculative rewrite was made.** The candidates are listed in RE-5C
  §2 and RE-5D §2.

## 4. CPU per request

`benchmarks/phase3a/run.mjs`, `results/re5/cpu-ab/`:

- **Builds:** baseline `54daca7…` against candidate `0b2340e…`.
- **Run:** 1 isolate, closed loop, 32 connections, 5 s warm-up then
  10 s measured.
- **Rounds:** 3, the two builds interleaved in a shuffled order each
  round.
- **Pinning:** server on CPUs 0–3, load generator on 4–5.
- **Checks:** every response checked; 72 of 72 runs valid.
- **Measures:** CPU from the process tree (`/proc`), instructions and
  cycles from `perf stat` attached by the harness. Instructions are much
  less sensitive to host noise than CPU time.
- **Statistic:** medians of the 3 rounds. `lg-limited`: the load
  generator, not the server, was the limit (latency there is not the
  server's).

| Workload | CPU ms/req (baseline → candidate) | Δ CPU | Instructions/req | Δ instr | req/s | p99 ms | Δ p99 |
|---|---|---|---|---|---|---|---|
| static-file | 0.058 → 0.058 | −0.4 % | 62k → 62k | +0.1 % | 60 741 → 60 792 | 1.39 → 1.41 | +1.3 % |
| static-gzip | 0.060 → 0.060 | 0.0 % | 71k → 71k | −0.5 % | 58 792 → 58 389 | 1.40 → 1.45 | +3.8 % |
| prerendered (lg-limited) | 0.022 → 0.022 | −0.3 % | 39k → 38k | −2.4 % | 124 562 → 122 697 | 0.58 → 0.56 | −3.3 % |
| prerendered-gzip (lg-limited) | 0.022 → 0.021 | −2.3 % | 44k → 43k | −2.2 % | 137 287 → 136 968 | 0.49 → 0.53 | +7.9 % |
| ssr | 1.641 → 1.628 | −0.8 % | 6.67M → 6.64M | −0.4 % | 768 → 784 | 63.6 → 61.7 | −2.9 % |
| ssr-gzip | 2.003 → 1.892 | −5.6 % | 8.08M → 7.56M | −6.4 % | 707 → 733 | 65.5 → 64.8 | −1.1 % |
| rsc | 0.966 → 0.942 | −2.4 % | 3.67M → 3.62M | −1.4 % | 1 357 → 1 395 | 33.5 → 33.2 | −1.1 % |
| action | 0.668 → 0.638 | −4.6 % | 2.44M → 2.39M | −2.1 % | 2 448 → 2 551 | 20.9 → 20.4 | −2.2 % |
| action-multipart | 0.577 → 0.554 | −3.9 % | 2.39M → 2.39M | 0.0 % | 2 432 → 2 576 | 18.3 → 16.9 | −7.4 % |
| isr-hit | 0.297 → 0.289 | −2.5 % | 1.27M → 1.27M | −0.4 % | 4 883 → 5 009 | 9.88 → 9.89 | +0.2 % |
| pages-ssr | 0.838 → 0.838 | 0.0 % | 3.35M → 3.35M | −0.2 % | 1 726 → 1 723 | 26.7 → 27.0 | +1.1 % |
| pages-api-get | 0.189 → 0.190 | +0.6 % | 760k → 760k | 0.0 % | 8 050 → 7 970 | 7.07 → 6.95 | −1.6 % |

Per-round CPU ranges (min–max over the 3 rounds) overlap between the
builds on every workload, for example:

- SSR: 1.608–1.645 against 1.605–1.661;
- action: 0.655–0.674 against 0.618–0.664.

**Reading:**

- **No CPU regression on any path.** Instructions per request are
  unchanged or lower everywhere.
- **JavaScript routes are 1–6 % lower in median CPU.** This is
  consistent with the removed heartbeat: 40 wakeups per second, each a
  turn of the isolate's runtime, competed with requests on the isolate
  thread. With 3 rounds and overlapping ranges, it is "slightly lower",
  not a decisive improvement.
- **Gzip compression of dynamic SSR** costs 0.25–0.36 ms of CPU per
  request (22 % of an SSR request). Cached gzip of static and
  prerendered bodies costs about nothing (RE-5E).
- **p99 is within ±8 %** everywhere. The largest increase (+7.9 %) is on
  a load-generator-limited workload, so its latency is not the server's.
- **Against the brief's targets:**
  - CPU per successful request 10–20 % lower: **not met**;
  - p99 no worse than +5 %: met on every server-limited workload.

## 5. Measurement limits

- One host, a desktop sandbox, process level. CPU-set pinning (server
  0–3, load generator 4–5) and interleaved, shuffled configurations
  reduce, but do not remove, host noise.
- No `perf`: CPU attribution per function was not possible. CPU is
  measured per process (`/proc`), and allocation sites by sampled
  backtraces.
- Lock contention, blocking-pool dispatches and GC: lock contention was
  not measurable without `perf` or a lock profiler. The cache lock change
  (RE-5D) is structural. GC frequency and pauses are in RE-5G.
