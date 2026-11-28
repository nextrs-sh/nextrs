# RE-4C: native allocation and buffer budgets

An audit of the buffers NextRS's own Rust code holds whole, which of
them now have an aggregate byte budget, and what remains bounded only
per request. Code at `main` `0d2a742` plus this branch.

## 1. Audit

31 sites were inventoried, each by file:line, what is held, per-item and
aggregate bounds, and whether it is copied. Grouped:

| Group | Sites | Per item | Aggregate before RE-4 | Copies |
|---|---|---|---|---|
| **Static and `public/` files read whole** (`lib.rs` `load_file`), incl. ranges | 2 | none | **none** | moved into `Bytes`, shared with the compression cache |
| **Request body buffered for the proxy** (`resolve.rs` `buffer_body`) | 1 | `min(proxyClientMaxBodySize, --max-request-body)` = 10 MiB | **none** | joined once; then copied on every JS read (shared `Bytes`) |
| Prerendered HTML / RSC resident from startup | 1 | none | all non-ISR prerenders (fixed after startup) | shared per request |
| Compression cache and fills | 2 | 4 MiB identity | `--compression-cache`; fills in progress uncounted | new gzip buffer |
| Streaming gzip | 1 | ~32 KiB out per slice; input frame held | per response | moved |
| Request body to JS (no proxy) | 2 | streamed (≤ 64 KiB chunks) | concurrency | copied out of `Bytes` if shared |
| Response body from JS | 2 | `--response-buffer` 64 KiB + one chunk of any size | concurrency × that | **always copied** out of V8 |
| Outbound fetch request / response | 3 | request "bytes" body: none; response: `--net-max-response` 32 MiB, streamed | 256 in flight per isolate | copied out of V8 |
| Next's own-server fetches (`internal.rs`) | 1 | none on the body | 64 in flight | shared |
| External rewrites (`external.rs`, `net.rs`) | 2 | request ≤ 64 KiB buffered, larger streamed; **response: no byte limit**, streamed | none | — |
| Multipart (`formData()`) | 1 | body limit upstream | none | **copied twice** (≈ 3× the body) |
| Image source, decode, encode, cache | 5 | `maximumResponseBody` 50 MB; pixels; 50 MiB entry | `--image-source-budget` 512 MiB, `--image-memory-budget` 1 GiB, `--image-cache-max-memory` 64 MiB | several (decode, ICC, flatten, encoder output, disk read 2×) |
| Data / ISR cache | 3 | 2 MiB / 16 MiB | `--cache-max-memory` 50 MiB; disk-write copies uncounted | `String` copy in, `from_utf8_lossy` copy out |
| `fs.readFileSync`, one-shot zlib | 2 | none / ≤ 1 GiB output | none | moved into V8 |
| Queue, connections | 2 | headers; 64 KiB hyper buffer | `--max-queued` 512 (bytes now bounded with admission), `--max-connections` 1024 | — |

Two buffers had **no aggregate bound** and are held while slow clients
read: whole static files and proxy-buffered request bodies. At 1024
connections, a 2 MiB static chunk can hold 2 GiB. A slow-reading client
keeps its whole response buffer alive for up to `--write-timeout` (30 s).

## 2. Implemented: `--native-buffer-budget`

`crates/nextrs-server/src/native_budget.rs`, off by default.

- **One byte budget** over all requests, for the two unbounded sites:
  - static and `public/` file bodies (full file or range);
  - proxy-buffered request bodies (the declared length, else the limit).
- **Reserved before allocating.** For files, the blocking read task tries
  a non-blocking reservation once it knows the size. Without room it
  returns "need N bytes"; the async side waits for room, then reads again
  holding the permit. A permit from an earlier attempt that still covers
  the size is reused. Proxy bodies reserve before `to_bytes`.
- **RAII release.** The permit sits inside the response body (a wrapper
  that delegates frames and keeps the exact size hint, so
  `Content-Length` is unchanged). For a proxy, an `Arc` permit is shared
  by the proxy's and the route's copy of the body. It is released when
  the last owner is dropped: completion, handler error, cancellation,
  request timeout, client disconnect or write timeout (hyper drops the
  body), and graceful shutdown (connections closed). There is no
  separate release path to forget.
- **FIFO, bounded wait.** A tokio semaphore in KiB units, the same
  mechanism as the existing image memory budget, so a large reservation
  is not overtaken by small ones. After `--native-buffer-wait` (2 s), the
  response is **503** `native-buffer-budget` with `Retry-After: 1`.
- **No reservation** for HEAD, 304, 412 or 416 (nothing is read).
- **A file larger than the whole budget takes all of it** and is served
  alone. The aggregate bound is therefore `max(budget, largest file)`,
  and no file becomes unservable. *Superseded in RE-4.1: larger files
  are streamed in budgeted chunks, proxy bodies that cannot fit get 413,
  and the bound is the budget
  ([RE-4.1-NATIVE-BUFFER-HARDENING](RE-4.1-NATIVE-BUFFER-HARDENING.md)).*
- Metrics: `nextrs_native_buffer_{budget,reserved,peak}_bytes`, and per
  kind `_reserved_bytes_total`, `_waits_total`, `_refused_total`.

Tests (`crates/nextrs-server/tests/native_budget.rs`, real Next.js 16.4.3
build snapshot; unit tests in `native_budget.rs`):

| Test | Shows |
|---|---|
| `a_response_holds_its_bytes_until_its_body_is_dropped` | reserved while the body exists, 0 after it is read |
| `no_room_waits_then_answers_503_and_room_again_serves` | 503 + `Retry-After` + code after the wait; a waiter is served when the holder's body is dropped; refusal counted |
| `head_and_not_modified_reserve_nothing` | HEAD and 304 succeed with the budget fully held |
| `a_range_reserves_only_its_bytes` | a 1000-byte range fits beside a held 10 KiB file in 12 KiB |
| `a_file_larger_than_the_budget_is_served_alone` | 10 KiB file, 4 KiB budget: served, holding all of it |
| `many_concurrent_requests_stay_within_the_budget_and_release_everything` | 64 concurrent requests in a 40 KiB budget: all 200, never above budget, 0 after |
| `without_a_budget_nothing_changes` | default path unchanged |
| unit: `a_permit_body_releases_its_bytes_when_dropped` | body length kept; release on drop |

**Not covered end to end:** the proxy-body permit. No local build
snapshot has a proxy that reads request bodies, and the fixture app has
no proxy. The wrapper is the same `PermitBody`, unit-tested, and the
default path (budget off) is covered by the compatibility suites in CI.

## 3. Not budgeted, and why

| Buffer | Why not | Existing bound |
|---|---|---|
| Response chunks from JS (copied out of V8) | Already flow-controlled per response. A chunk can be large (`res.end(buf)`), but the copy is made inside an op on the isolate thread, where waiting for a budget would block JavaScript | `--response-buffer` + one chunk, × concurrency (× admission when on) |
| Request body to JS (no proxy) | Streamed, one hyper chunk at a time | ≤ 64 KiB × concurrency |
| Outbound fetch bodies | Inside JS ops; a refusal would need a new JS-visible error | 32 MiB response, 256 in flight |
| External-rewrite response | Streamed through a channel of 1 | idle timeouts; no byte cap (noted as a gap) |
| Multipart | JS-facing op; its two copies could be reduced to one (follow-up) | request body limit (10 MiB, 1 MB for actions) |
| Image pipeline | Has its own reserved budgets | unchanged |
| Caches | Capacity-bounded | unchanged; disk-write copies uncounted (small, transient) |
| Prerenders resident from startup | Fixed size after startup; not request-driven | — |
| `fs.readFileSync` | Inside the JS op; bounded by the files in the build | none |

## 4. Copies that could be removed (not done here)

- Multipart: `Bytes::copy_from_slice(&body)` and then `to_vec()` per
  part. Parsing from the borrowed slice would remove one full copy.
- Proxy-buffered bodies are copied on every JavaScript read because the
  `Bytes` is shared. Handing the route a unique copy would avoid that.
- Image cache disk reads hold the raw file and a copy of the body.

They are not unbounded aggregates, so they were left for a profiling pass
with numbers.

## 5. What is enforced

With `--native-buffer-budget`, the aggregate of whole static file bodies
and proxy-buffered request bodies is bounded by `max(budget, largest
single buffer)`, released by RAII on every path. Every other native
buffer keeps its per-request or per-cache bound. None of this changes
the default build: with the flag absent, the code paths are as before.
