# Compression cache and tail latency

Series 1 recorded one NextRS `static-assets` run at 10 requests/s with a
p99 of 343.8 ms (about 6 slow requests out of 603); the other five NextRS
static runs had p99 between 11.5 and 21.8 ms. This page reviews the gzip
cache and the static-file path, tests the tail-latency hypotheses
locally, and states what is and is not explained.

- [Summary](#summary)
- [How static files are served](#how-static-files-are-served)
- [Code review](#code-review)
- [Experiments](#experiments)
- [The 343.8 ms p99](#the-3438-ms-p99)

## Summary

- **No internal cause for a 343.8 ms tail was found in the compression
  cache.** Concurrent misses for one file compress once (1 miss, 18–31
  waiters coalesced, max 44 ms); filling the other 17 compressible files at
  once takes 39–61 ms of compression in total; a warm cache serves 400 requests/s with
  zero misses.
- **The cache is the main reason NextRS's static serving is cheap:** about
  0.44 ms of CPU per gzip request against 4.6 ms for `next start`, which
  compresses every response again ([CPU-EFFICIENCY.md](CPU-EFFICIENCY.md)).
- **Not reproduced with this application:** eviction churn. Velasia's whole
  gzip set is about 330 KiB and fits the smallest cache setting (1 MiB),
  so the rc.3 `PERF-1` behaviour (allocator retention under churn) does
  not occur here.
- **Remaining issues, not changed:** concurrent fills for different files
  are not bounded (only relevant under churn), and a gzip hit still reads
  the file. Both are follow-ups with their own measurements
  ([FINAL-RECOMMENDATIONS.md](FINAL-RECOMMENDATIONS.md)).
- **Added observability:** `nextrs_compression_cache_fills_in_flight`,
  `nextrs_compression_cache_fill_micros`, and `nextrs_compression_stream_*`
  for responses compressed as they stream.

## How static files are served

For `GET /_next/static/...` with `Accept-Encoding: gzip`
(`lib.rs` `file_response`, `compress.rs`, `compress_cache.rs`):

1. One blocking-pool task opens the file, takes its metadata from the
   open handle, evaluates the conditional headers and the range, reads the
   whole file into a new buffer, and stats the handle again (the file must
   not have changed while it was read).
2. The response carries a `Cacheable` marker: the compression-cache key
   (path, size, modification and change time, device, inode) and the
   identity bytes.
3. The compression step looks the key up. A hit swaps the body for the
   cached gzip bytes. A miss compresses once (inline up to 64 KiB, on the
   blocking pool above), stores the result and serves it. Concurrent
   misses for one key wait for that one compression (`OnceCell`).

`next start` (Next.js 16.4.6) serves the same files with `send` and its
`compression` middleware, which compresses **every** response again: it
has no cache.

## Code review

| Question | Finding |
|---|---|
| Cache-key correctness | Keyed by file identity as stat'ed per request; a file replaced or rewritten gets a new key; a fill whose file changed during the read is not stored. Correct. Prerendered bodies are keyed by the identity of their in-memory copy, never replaced. |
| Duplicate work for one key | None: the `OnceCell` per slot makes concurrent misses wait for one fill (`coalesced` counts them). A failed fill stores nothing; waiters retry. |
| Concurrent fills for **different** keys | Unbounded: every distinct missing key above 64 KiB starts its own blocking-pool compression. With a cache that fits, this happens once per file after start. With an undersized cache (eviction churn) it happens continuously and is the rc.3 `PERF-1` memory finding (allocator retention across many blocking threads). |
| Waiting | Only for a fill of the same key, which is bounded by one compression of one file. No global lock: the map is an `RwLock` held only to find or insert a slot, never across a compression. |
| Eviction | Least recently used down to 7/8 of capacity, batched; entries being filled cost nothing until filled. |
| Memory accounting | Compressed bytes plus a fixed per-entry overhead and the key path length. Identity bytes are never held. |
| Per-request work on a hit | The whole file is still read into a new buffer (step 1) and dropped once the cached gzip copy is chosen. See [OPTIMIZATION-RESULTS.md](OPTIMIZATION-RESULTS.md#not-changed). |
| Observability | Counters existed (hits, misses, coalesced, bypassed, evictions, entries, held bytes). Added: `fills_in_flight` (a gauge) and `fill_micros`. |

## Experiments

`benchmarks/series1/run.mjs fills`, results in
`benchmarks/results/series1/fills-20261126` (2 rounds; every window
started below a host load average of 10). Each run starts a fresh
container, so its first requests are misses. NextRS `optA` (its static
path is unchanged from `head`).

| Experiment | Cache | Misses | Coalesced | Hits | p50 | p99 | Max |
|---|---|---:|---:|---:|---:|---:|---:|
| 32 simultaneous requests for the largest file (229 KiB) | default | 1 | 18–31 | 31–44 | 27–30 ms | 38–44 ms | 44 ms |
| same | 1 MiB | 1 | 22–31 | 31–40 | 29–33 ms | 41–53 ms | 53 ms |
| every file at once, 32 connections | default | 17 | 2–4 | 27–29 | 16–20 ms | 28–34 ms | 34 ms |
| 400 requests/s for 60 s (warm) | default | 0 | 0 | 22,739–22,748 | 1.46–1.81 ms | 26–28 ms | 94–209 ms |
| same | 1 MiB | 0 | 0 | 22,747–22,753 | 1.45–1.79 ms | 82–95 ms | 179–227 ms |
| `next start`, 400 requests/s (no cache) | n/a | | | | 14 ms; 1,000 ms in the round where it fell behind | 280–1,898 ms | 535–2,126 ms |

One of the 19 files (855 bytes) is below the 1 KiB compression
threshold, and the largest was already cached by the burst before, so 17
fills is every other compressible file. Compression time for them: 39–61
ms in total (`fill_micros`).

- **Coalescing works:** one compression however many requests miss at once.
- **Fill time is small:** the largest file compresses in about 20–28 ms
  on this host; the burst's latency is that fill plus queueing on 32
  connections.
- **The cache never evicted** in these runs (the set fits); the "1 MiB"
  rows measure a smaller budget, not churn.
- **Threads:** NextRS's blocking pool grew from 11 to 20–28 threads during
  the bursts and to 25–42 at 400 requests/s (file reads run there), and
  shrinks again when idle; RSS stayed between 73 and 79 MiB.
- **Tails with zero misses:** at 400 requests/s NextRS's maximum was
  94–227 ms although nothing was compressed in those windows; p99 was
  26–95 ms. These outliers are not compression; on this shared host they
  are consistent with scheduling delays, and they were also seen with
  `next start`.

## The 343.8 ms p99

**Unexplained.** What is known:

- It was one run out of six NextRS static runs (10 requests/s, round 2,
  NextRS first), p99 over 603 requests: about 6 slow requests. Its p50
  and p95 were normal.
- The compression cache cannot explain it from what was measured: a fresh
  NextRS fills all compressible files in under 70 ms in total, and in a
  warm series nothing is compressed. By round 2 at 10 requests/s, the
  Railway container had served those files for more than 50 minutes.
- Local tails of 100–230 ms appear with zero misses, on both runtimes,
  correlated with host contention.
- Series 1 collected no NextRS metrics, so the cause on Railway (edge,
  network, host scheduling, the container itself) cannot be determined.

Series 2 should collect `nextrs_http_served_head_seconds{via="static"}`
and the compression-cache counters before and after every run: a tail
present in the client's numbers but absent from the server's head times
is outside NextRS.
