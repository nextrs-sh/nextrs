# RE-5E: compression efficiency

Response compression (`crates/nextrs-server/src/compress.rs`) and the
gzip cache of immutable responses (`compress_cache.rs`). This report
audits where compression costs CPU and memory, evaluates the RE-4.1
trade-off between the native buffer budget and cached gzip copies, and
gives process-level measurements. **No compression code changed.**

## 1. What runs today

| Response | Compression | Memory |
|---|---|---|
| `/_next/static` files, prerendered HTML/RSC/segments (immutable) | gzip once, copy kept in a bounded LRU (`--compression-cache`, 32 MiB; 8 with `low-memory`), at most 4 MiB per entry; concurrent fills coalesced; below 64 KiB inline, above on the blocking pool | the gzip copy (counted in the cache) and, during a fill, the identity bytes (counted in the native budget since RE-4.1A) |
| Dynamic SSR / RSC / route handlers | streamed gzip, zlib level 6 (`Compression::default()`, the `compression` middleware's default), a sync flush after each chunk | the encoder state (about 256 KiB for zlib at level 6: window + hash tables) plus the chunk in hand, per response |
| Streamed static files (above the native budget's whole-read limit) | gzip per request, streamed in budgeted chunks | the encoder state plus at most 64 KiB chunks |

Content negotiation, the 1 KiB threshold, `no-transform`, Vary, ETag
weakening and the HEAD/206 rules follow next start's `compression`
middleware. They are unchanged.

## 2. The RE-4.1 trade-off: budgets below 16 MiB

A file is read whole, and so can fill the compression cache, only up to a
quarter of `--native-buffer-budget`, and at most 4 MiB. Below that limit
it is streamed and compressed on every request.

| Budget | Whole-read limit | Velasia static assets above it |
|---|---|---|
| 1 MiB | 256 KiB | 0 (largest compressible asset: 234 KB) |
| 4 MiB | 1 MiB | 0 |
| 16 MiB (proposed 256 MiB profile) | 4 MiB | 0 |

Measured on the pinned Velasia build (`apps/web/.next/static`):

- 59 files, 1.75 MB in total;
- median 15 KB, p99 165 KB, maximum 234 KB;
- the largest `public/` files are 450 KB JPEGs, which are not compressible
  types and are never gzipped.

**Conclusion:**

- For Velasia, and for applications whose text assets stay below a
  quarter of the budget, the trade-off does not arise: every asset is
  read whole and its gzip copy is cached.
- It arises only for multi-MiB JavaScript or JSON assets under a small
  budget. There, each request streams and recompresses the file. That
  costs CPU, not memory: the budget bounds what is held.
- A bounded cache fill, streaming the identity bytes once through gzip
  into the cache, would remove the repeat. It is not implemented: no
  measured workload needs it.

## 3. Measurements (process level)

The Phase 3A harness, one isolate, closed loop, every response checked.
These are the workloads added for this report (`benchmarks/phase3a/workloads.mjs`):

- **`static-gzip`:** a `/_next/static` chunk from the cached gzip copy.
- **`prerendered-gzip`:** a prerendered page from its cached gzip copy.
- **`ssr-gzip`:** dynamic SSR compressed per request.

The load generator checks that their plaintext markers are absent, so the
bodies really are compressed.

The results are in [RE-5-BASELINE-AND-PROFILING](RE-5-BASELINE-AND-PROFILING.md) §4,
next to the identity variants, for the baseline and the candidate.

## 4. Options considered

| Option | Evaluation | Decision |
|---|---|---|
| Lower zlib level for dynamic responses (level 1–3) | less CPU per response, more bytes on the wire; next start sends level 6 | not changed: it is a default (and wire-size) change that needs owner approval and container CPU data |
| Brotli/zstd | a new dependency and format, not offered by next start's middleware | not adopted |
| Bounded cache fill for streamed large files | removes repeated compression above the whole-read limit | not needed by any measured workload (§2) |
| Smaller zlib window/memory level for streamed responses | less encoder memory per concurrent response (each about 256 KiB); slightly worse ratio | candidate for the coordinated profiles (RE-5I) under memory pressure; not measured in a container |
| Blocking-pool threshold (64 KiB) | fills above it go to the blocking pool; one dispatch per fill, once per immutable body | unchanged |

## 5. Slow clients and backpressure

- A streamed gzip response polls the route's body only when the client
  has taken the previous output.
- A slow client therefore holds one encoder and one chunk.
- Concurrency multiplies the encoder state: 100 slow gzip clients hold
  about 25 MiB of zlib state. That memory is not counted in any budget
  today, and is listed in [RE-5-RESOURCE-PROFILE-DESIGN](RE-5-RESOURCE-PROFILE-DESIGN.md).
