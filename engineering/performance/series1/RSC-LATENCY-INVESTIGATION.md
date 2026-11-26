# RSC navigation latency

Series 1 on Railway: `next start` had the lower RSC median in 6 of 6 pairs
(5.7–7.0 ms against 6.6–7.9 ms), and the lower TTFB p95 in 5 of 6. This
page traces where an RSC request spends its time in NextRS, names the
cause, and reports the change made for it.

- [Summary](#summary)
- [What Series 1 actually requested](#what-series-1-actually-requested)
- [The request path, verified](#the-request-path-verified)
- [Where the time went](#where-the-time-went)
- [The change](#the-change)
- [Results](#results)
- [Answers to the investigation questions](#answers-to-the-investigation-questions)
- [Remaining differences](#remaining-differences)

## Summary

- **The Series 1 RSC requests were never served by Rust alone.** All 14
  measured pages (`/`, `/destinations`, 12 destination pages) are
  Partial Prerendering pages with a revalidation time (`renderingMode:
  PARTIALLY_STATIC`, `revalidate: 3600`). NextRS hands every request for
  such a page to Next's own handler in a V8 isolate, which answers from the
  response cache (`x-nextjs-cache: HIT`). Measured, not inferred: with the
  new `nextrs_http_served_total{via}` counter, every one of these requests
  is `via="executor"`.
- **Cause (established):** NextRS's cache handler copied, transferred,
  JSON-parsed and base64-decoded the **whole stored cache entry** (HTML,
  RSC payload and every segment prefetch: 305–489 KiB per page) on
  **every** request, although the entry had not changed. `next start`
  serves the already-decoded object from `FileSystemCache`'s memory cache.
  On the real Velasia pages, the JavaScript half of that work alone costs
  1.7 ms per destination page and 2.9 ms for `/` (Node.js 24 V8, one CPU),
  in line with the 1–2 ms gap Series 1 saw.
- **Change:** each isolate keeps its decoded copy of a response-cache entry
  and reuses it while the store vouches that the entry is unchanged (a
  per-entry generation check), as `next start` reuses its memory-cache
  object. Measured locally on Series 1's RSC workload (one isolate, 2
  CPUs): closed-loop throughput 222 → 548 req/s, CPU per request 6.48 →
  2.98 ms, median at 50 req/s 7.5 → 5.6 ms, peak RSS 303 → 230 MiB.
  `next start` on the same build: 301 req/s, 5.02 ms, 6.2 ms, 230 MiB.
  Details in [OPTIMIZATION-RESULTS.md](OPTIMIZATION-RESULTS.md).
- RSC payload semantics are unchanged: byte-identical decoded bodies and
  identical caching headers to `next start` before and after.

## What Series 1 actually requested

From the build Series 1 deployed (Velasia Airways `d04127a`, Next.js
16.4.6, `cacheComponents` and `partialPrefetching` on), the prerender
manifest of every RSC path in the workload:

| Paths | `renderingMode` | `revalidate` | Postponed state |
|---|---|---|---|
| `/` | `PARTIALLY_STATIC` | 3600 s | none |
| `/destinations` | `PARTIALLY_STATIC` | 3600 s | none |
| `/destinations/{12 slugs}` | `PARTIALLY_STATIC` (from `/destinations/[slug]`) | 3600 s | none |

The requests carried `RSC: 1` and the `_rsc` cache-busting key the server
asks for (here an empty key, `?_rsc`, because no router-state headers are
sent), with `Accept-Encoding: gzip`. No proxy (`proxy.ts`) exists in this
build. Both runtimes answer `200`, `text/x-component`, `x-nextjs-cache:
HIT`, `cache-control: s-maxage=3600, stale-while-revalidate=82800`, and the
same `Vary`. Without postponed state, the answer is the stored RSC payload
as built: no rendering happens on either runtime.

## The request path, verified

Each step below was checked against the code at `80f6d80` (HEAD at the
start of this work; the serving path is unchanged since rc.2).

```text
client ──► axum handler (lib.rs handle)
           │  forwarded-header policy, Next internal headers removed
           ▼
           resolve::resolve (resolve.rs)                     ─┐
           │  next.config headers (Velasia: security headers)  │ "routing"
           │  redirects (2 rules, no match)                    │ Rust only
           │  no proxy                                         │ (µs)
           │  get_item(pathname) → Output::App { route, path } ─┘
           ▼
           serve_resolved (lib.rs)
           │  state.pages.get(path) → None: pages with ISR or a PPR shell
           │  are not held in memory (AppState::load skips them)
           │  RSC request on an App page → _rsc check (rsc_redirect)
           ▼
           dynamic::render → executor (nextrs-runtime)         ─┐
           │  queue for an isolate (1 isolate)                   │
           │  Next's compiled handler (base-server, app-page)    │ V8
           │   └ ResponseCache → IncrementalCache.get            │
           │      └ NextRSCacheHandler.get (js/03_server.js)     │
           │         └ op_nextrs_cache_get ─► CacheStore.lookup  │
           │            (Rust: copy of the stored envelope)      │
           │         └ JSON.parse + base64 decode                │
           │   └ writes rscData (the whole .rsc payload)        ─┘
           ▼
           response head + one body chunk back to Rust
           ▼
           compress::Negotiated::apply → GzipBody (streamed gzip,
           per request: executor output is never in the gzip cache)
           ▼
           hyper writes the response
```

Common with static assets: the axum handler, routing up to `get_item`,
and the compression step. Unique to these RSC requests: everything from
`dynamic::render` down, and per-request gzip (a static file's gzip copy is
cached).

`next start` runs the same Next.js handler code from base-server down, in
Node.js, with `FileSystemCache` as the cache handler: its `get` returns the
object held in a module-level LRU (`memoryCache.get(key)`, no copy), so a
cache hit costs it no parsing.

## Where the time went

### Measured per request (NextRS `instr`, before the change)

From NextRS's own metrics during the RSC runs (see
[BASELINE.md](BASELINE.md) for the run conditions):

| Metric | Value per RSC request |
|---|---|
| Served by | `executor` for 100 % of the RSC requests |
| Bytes the cache copied out to JavaScript (`nextrs_cache_value_bytes_total`) | about 323 KiB on average over the 14 paths |
| Streamed gzip frames | 1 (the payload arrives as one chunk) |
| Streamed gzip encoder time | 1.0–1.7 ms |
| Routing (mean, 50 req/s) | 0.19–0.23 ms |

### The cost of that copy, isolated

`benchmarks/series1/envelope-decode.mjs` rebuilds each page's stored
envelope exactly as NextRS's cache handler stores it (from the build's
`.html`, `.rsc`, `.segments/` and `.meta`) and times what every request
did with it, in Node.js 24.18.0's V8, on one CPU of the benchmark host, on
a quiet VM:

| Page | Envelope | Segments | String from bytes | String + `JSON.parse` + base64 decode |
|---|---:|---:|---:|---:|
| `/` | 489 KiB | 4 | 1.12 ms | 2.88 ms |
| `/destinations` | 326 KiB | 4 | 0.56 ms | 1.66 ms |
| `/destinations/tokyo` | 305 KiB | 4 | 0.56 ms | 1.69 ms |

This excludes the Rust side of the same lookup: a copy of the envelope
into a `String` while holding the store's mutex (`CacheStore::lookup`),
and its conversion into a V8 string by the op layer. It also leaves out the
garbage it creates: roughly half a megabyte of short-lived strings and
buffers per request, which the young-generation collector has to clear.

Only the RSC payload (48–77 KiB) is sent; the HTML and the segment
prefetches in the same entry were decoded and thrown away.

### Hypotheses examined and rejected

| Hypothesis | Evidence | Verdict |
|---|---|---|
| V8 slower than Node.js at running Next's handler | Same Next.js code on both; the measurable difference was NextRS's own cache-handler work, which `next start` does not do | Not the primary cause |
| Routing or header construction | `nextrs_http_routing_seconds` at 50 RSC req/s: mean 145–237 µs, median ≤ 250 µs, against a 5–9 ms response head (`nextrs_http_served_head_seconds{via="executor"}`) | Not disproportionate |
| Compression per request | Both runtimes gzip RSC payloads per request (`next start`'s `compression` middleware has no cache either). NextRS: one frame per response, 1.1–1.7 ms of encoder time for 51 KiB on average (`nextrs_compression_stream_seconds_total`) | Same work on both; a large cost, but not a difference |
| Response encoding differs | Same `Content-Encoding`, same headers, decoded bodies byte-identical; NextRS's gzip output is 0.6 % larger (flate2 vs zlib, both level 6) | Rejected |
| Prerendered RSC served from Rust memory with extra lookups | Not applicable: these pages never reach the in-memory path | Rejected |
| Delay external to NextRS (edge) | The same gap reproduced locally without any edge, and is explained by the work above | The difference is internal |

## The change

`runtime: reuse an isolate's decoded copy of an unchanged response-cache
entry` (commit `91a7729`):

- `op_nextrs_cache_get(key, known)`: the caller names the store generation
  of the copy it holds. The store answers `unchanged`, without the value,
  only while the entry is exactly that generation; a new value (from any
  isolate), an eviction marker or a miss returns what it returned before.
  An entry held only on disk is answered `unchanged` without reading its
  file.
- `#responseEntry` in the cache handler keeps the decoded value per key and
  serves it again on `unchanged`. A copy is only ever served after the
  store has vouched for it, so a stale copy cannot be served. Tags and
  times are judged on every request, as before.
- Bounded: per isolate, at most the store's memory budget and 16 MiB
  (counted as stored envelope bytes), least recently used first; an entry
  over a quarter of the budget is not kept.
- The value copy for a changed entry is now made after the store's lock is
  released.
- `nextrs_cache_reused_total` counts reused lookups.

Sharing one decoded object between requests is what `next start` does: its
`FileSystemCache.get` returns the same object from its memory cache to
every request, so Next's code already treats it as read-only.

## Results

See [OPTIMIZATION-RESULTS.md](OPTIMIZATION-RESULTS.md).

## Answers to the investigation questions

1. **Is the RSC path unnecessarily different from ordinary prerendered
   content?** For these pages, yes, by design: ISR and PPR pages go to
   Next's handler so that revalidation and resumption behave as under
   `next start`. The handler path itself was not the problem; its cache
   lookup was.
2. **Extra metadata lookups?** No. One cache lookup per request (plus the
   tag-manifest sync, a version check).
3. **Repeated allocation or copying of payload buffers?** Yes: the whole
   entry was copied twice (Rust, then V8) and decoded on every request.
   Fixed.
4. **Compression on each request?** Yes, on both runtimes. Executor output
   is not eligible for the gzip cache, as `next start` caches no gzip.
5. **Are compressed RSC responses cached?** Only for pages served from
   memory (no ISR, no PPR shell). Not for these pages, on either runtime.
6. **Routing or header construction disproportionately expensive?** No.
7. **Same response encoding as `next start`?** Yes (gzip, same headers;
   decoded bodies identical).
8. **Internal or external delay?** Internal: reproduced locally without an
   edge, and explained by the per-request decode.

## Remaining differences

- RSC answers still go through an isolate (one by default): unlike static
  files, they are bound by one JavaScript thread, as under `next start`.
- Per-request gzip of the RSC payload remains on both runtimes.
- The `"use cache"` handler (Cache Components) still parses its entries per
  read. It is used while a page renders, not when a cached page is served,
  so it did not affect these requests; the same reuse could apply there.
