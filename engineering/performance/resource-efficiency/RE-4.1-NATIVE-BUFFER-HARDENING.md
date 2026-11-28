# RE-4.1A: native buffer hardening

`--native-buffer-budget` after RE-4 bounded the bytes it covered by
`max(budget, largest buffer)`. A static file larger than the whole budget
took all of it and was read whole, and so was a proxy request body larger
than the budget. This milestone makes the budget itself the bound, for
the same buffers. Branch `fix/re-4.1-resource-controls`, commit `1ca0a56`.
With the flag absent, no code path changed (§5).

## 1. Policy

| Buffer | RE-4 | RE-4.1 |
|---|---|---|
| Static / `public/` file or range up to the whole-read limit | read whole, reserved first | unchanged |
| Larger than the whole-read limit | read whole; above the budget, took the whole budget and was served alone | **streamed** in chunks, each reserved before it is read |
| Proxy-buffered request body, declared length ≤ budget | its declared length reserved before the first byte | **reserved as it arrives**, frame by frame (each step ≤ one hyper frame, ≤ 64 KiB) |
| Declared length > budget (and ≤ the body limit) | clamped: took the whole budget | **413** `native-buffer-budget-exceeded`, `Connection: close`, nothing read |
| Unknown length (chunked) | reserved the body limit (10 MiB), clamped to the whole budget, before the first byte | reserved as it arrives; growing past the budget gets the same 413; no room within the wait: 503 with `Connection: close` |

Limits, from the budget `B` (`native_budget.rs:166-183`):

- whole-read limit `W = max(chunk, min(B/4, 4 MiB))`. 4 MiB is the
  compression cache's entry limit, so every file the cache can hold keeps
  its cached gzip copy whenever `B ≥ 16 MiB`;
- chunk `min(B/4, 64 KiB)`, at least 1 KiB (Node's `send` streams with
  the same 64 KiB high-water mark).

Every reservation is therefore at most `B/4` (1 KiB for budgets under
4 KiB), and the aggregate of the covered buffers is at most `B`. The
`units_for` clamp stays as a guard, with a `debug_assert` that no caller
asks for more than the budget.

**413, not 503.** A body that cannot fit in the whole budget would be
refused again on retry, so a 503 would invite a retry loop. The 413
carries its own error code, so operators can tell it from
`request-body-too-large`.

**Incremental proxy reservations** (independent review F1, §4.1 of
[RE-4.1-INDEPENDENT-REVIEW](RE-4.1-INDEPENDENT-REVIEW.md)). Reserving a
body's maximum before reading it let one client trickling a chunked
body hold the whole budget. tokio's semaphore is FIFO, so that large
reservation also blocked every smaller one queued behind it: static
files got 503. A body now holds only the bytes it has sent
(`NativeBufferBudget::grow`, `OwnedSemaphorePermit::merge`), and each
wait is for one frame's growth. The cost: two growing bodies can wait on
each other, and both are bounded by `--native-buffer-wait` (a 503,
retryable).

## 2. Ownership

The reservation is owned by the buffer's `Bytes` (`BufferPermit::own`,
`native_budget.rs:127`, `Bytes::from_owner`). It is released when the last
clone of the memory is dropped, wherever that happens:

| Holder | Before (RE-4) | Now |
|---|---|---|
| Response body being written | `PermitBody` wrapper: released when the body was dropped | released when hyper drops the bytes it wrote or queued |
| hyper's write queue (up to `max_buf_size`, 64 KiB, per connection) | not counted once the body yielded the frame | counted until written |
| Compression cache fill (`compress_cache::gzip`) | the identity copy outlived the permit (review finding 9) | counted until the fill has compressed it |
| Streaming gzip (`GzipBody`) | — | the input chunk is held, and counted, until compressed |
| Proxy body: the proxy's copy and the route's copy | `Arc` permit in two `PermitBody` wrappers | the `Bytes` themselves. JavaScript's reads copy the bytes into V8 (`Vec::from(Bytes)`); those copies are not in this budget |

`PermitBody` is gone. A streamed chunk's reservation is moved into the
blocking read task, so a read that is still running when the body is
dropped frees its chunk and its reservation together when it finishes.

## 3. Streaming (`FileStream`, `native_budget.rs:323`)

- `load_file` (`lib.rs:2371`) opens the file, stats the handle, evaluates
  `If-Match`/`If-Unmodified-Since`, `If-None-Match`/`If-Modified-Since`,
  `Range` and `If-Range` exactly as before. Then, for a selection above
  `W`, it reserves only the first chunk (with the usual wait and 503) and
  returns the handle, seeked to the first selected byte.
- Headers are those of the whole-read path: Content-Length (the
  selection's length), Content-Range, ETag, Last-Modified,
  Accept-Ranges, Cache-Control.
- Each later chunk is reserved, then read on the blocking pool
  (`read_chunk`, exactly `min(remaining, chunk)` bytes).
- **Backpressure:** hyper polls for the next frame only when it can buffer
  it. A slow client therefore holds at most hyper's queue plus one chunk.
- **File changes:**
  - Truncated while streaming: the read comes up short, the body fails,
    hyper closes the connection, and the client sees a short body against
    its Content-Length, never other bytes.
  - Grown: stops at the declared length.
  - Replaced by rename: the opened file keeps being sent.
  - The whole-read path with a budget now reads at most the stat'ed
    length (`take(len)`), so a growing file cannot exceed its reservation.
- **Mid-stream wait:**
  - A later chunk waits for room up to 30 s (`STREAM_WAIT`, the default
    write timeout), then fails the body. It is counted in
    `refused_total{kind="static_stream"}`.
  - This exists because a consumer that keeps every chunk (an
    in-process collector) could otherwise stall the budget for good.
  - No NextRS consumer does that: hyper writes and drops, the image source
    fetch copies each chunk, and in-process self-requests forward through
    a channel of one. The unit test
    `a_consumer_that_keeps_every_chunk_fails_instead_of_stalling` pins
    the bound.
- **Compression:** a streamed file never has a compression cache entry.
  If gzip is negotiated, `GzipBody` compresses chunk by chunk, with no
  whole-file copy. Partial responses stay identity, as before.
- **Security:** path resolution and the dot-segment and `..` checks happen
  before `load_file` and are untouched.
- **Metrics:** new kind `static_stream` in
  `nextrs_native_buffer_{reserved_bytes_total,waits_total,refused_total}`.

## 4. Tests

`crates/nextrs-server/tests/native_budget.rs` runs on a real Next.js
16.4.3 build snapshot, copied with a 6 MiB compressible chunk and a 3 MiB
`application/octet-stream` public file. `--lib native_budget` holds the
unit tests. **20 tests, all passing** (11 integration, 9 unit).

| Test | Shows |
|---|---|
| `streamed_responses_match_the_unbudgeted_server` | 14 request variants × 2 files: status, every header except Date, and the decoded body are identical with the budget on (64 KiB) and off. Variants: full, gzip, HEAD, three ranges, unsatisfiable, multi-range, 304 by ETag and date, 412, If-Range by ETag and date, range + gzip. Reservations are 0 after each |
| `concurrent_full_and_range_requests_stay_within_the_budget` | 32 concurrent full, gzip and range requests on the 6 MiB file, 128 KiB budget, 4 worker threads: every body correct, sampled peak ≤ budget, 0 after |
| `slow_clients_and_disconnects_hold_at_most_the_budget` | real listener, 256 KiB budget. 8 clients reading 32 KiB every 10 ms, 8 hanging up after 100 KiB, 4 never reading then hanging up, and 1 fast client: every complete body correct, peak ≤ budget, 0 after the disconnects are noticed |
| `a_file_truncated_or_replaced_while_streaming` | rename: old content sent in full, the next request gets the new file. Truncation: the body errors, reservations 0 |
| `a_file_larger_than_the_budget_is_streamed_within_it` | 10 KiB file, 4 KiB budget: 1 KiB reserved, two more streams fit beside it, all bodies correct (RE-4: the whole 4 KiB held) |
| updated RE-4 tests | 503 after the wait, waiter served on release, HEAD/304 reserve nothing, a range fits where a whole read does not, 64 concurrent, budget off unchanged |
| unit | limits arithmetic; `own` releases with the last clone (slices count); one chunk in hand at a time; held chunks hold back the next; truncation fails; growth stops at the length; 50 drops mid-read release everything; the 30 s bound (shortened in the test) |

Proxy bodies, end to end (including a body trickled while static and
public files are requested: both 200 without waiting, at most 1 KiB
reserved), are in
[RE-4.1-NESTED-REQUEST-COMPATIBILITY](RE-4.1-NESTED-REQUEST-COMPATIBILITY.md)
§4.

RE-4's existing static-file suites (`range`, `compression`,
`preconditions`, `http`) run with the budget off and pass unchanged.

## 5. Budget off

`load_file` with no budget follows the same code as before. `wanted` is
computed, no reservation is made and `streamed` is false. The bytes
become `Bytes::from(vec)`, as before. The proxy path takes no reservation
and keeps its limit. Only the body type of the whole-read field changed
(`Vec<u8>` to `Bytes`, converted at the same point as before).

## 6. Not covered, and limits

- **CPU:** a streamed file costs one blocking-pool hop per 64 KiB chunk,
  the same granularity as `tokio::fs`. Not measured on the benchmark
  host.
- **Budgets under 16 MiB** stream files between `B/4` and 4 MiB, which
  then lose their cached gzip copy and are compressed per request, with
  a sync flush per chunk (a lower ratio at tiny chunks). The runbook's
  32 MiB keeps every cacheable file cached.
- **Streams overcommit:** only the first chunk is reserved before the
  response starts. Under sustained overload a stream can fail mid-body
  (after the 30 s chunk wait) instead of getting a 503 up front. About
  budget / 128 KiB slow readers, each holding hyper's 64 KiB queue plus a
  chunk until the write timeout, can occupy the budget. That is strictly
  less than RE-4, which held whole files. A per-connection cap is a
  possible follow-up.
- **hyper's flatten mode** copies a frame into its own buffer and drops
  the `Bytes` at once. That copy, at most `max_buf_size` per connection,
  is not counted. In queue mode, which hyper uses for `Bytes` frames,
  the frames themselves are queued and counted.
- **Still bounded only per request or per cache:** response chunks from
  JavaScript, outbound fetch bodies, multipart copies, image buffers (they
  have their own budgets), caches, and resident prerenders
  ([RE-4-NATIVE-BUFFER-BUDGETS](RE-4-NATIVE-BUFFER-BUDGETS.md) §3).
- **Not measured in a container.** The tests are process-level.

**Verdict for 4.1A:** with the flag on, the native buffer budget is a
strict bound on the bytes it covers (static and `public/` files and
proxy-buffered request bodies), tested at process level. It is not a
bound on the process's native memory.
