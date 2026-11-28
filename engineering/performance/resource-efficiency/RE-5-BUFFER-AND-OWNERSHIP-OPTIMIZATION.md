# RE-5C: Rust–V8 buffers and ownership

This report audits where request and response bytes are copied between
Rust and V8. It records one copy removed (`38e5659`) and why the rest stay.
It is a source audit of the pinned engine (deno_core 0.413.0, v8 crate
150.4.0, serde_v8 0.322.0) together with the allocation profile in
[RE-5-BASELINE-AND-PROFILING](RE-5-BASELINE-AND-PROFILING.md) §3.

**Nothing here is zero-copy end to end.** The one change removes one
copy on one path.

## 1. What the profile says first

On JavaScript routes, the copies that NextRS makes are a small share of
the native allocation:

- **SSR:** 76 % of allocation calls and about 90 % of bytes (417 of
  464 KB per request) have no NextRS frame on the stack. They come from
  V8's compiler zones, GC worklists, remembered sets and code space.
- **Response chunks** (`ResponseSender::write`): 5 calls and 4.8 KB per
  SSR request.
- **Request serialization** (`JsRequest` through serde_v8): 10 calls and
  0.4 KB.

So removing every NextRS copy on the SSR path would cut native allocated
bytes per request by about 1–2 %, not the 15–30 % the brief proposes.
That target is not reachable from NextRS code on JavaScript routes. On
the static and prerendered paths, allocation is already 31–45 calls per
request, spread over hyper, axum and http plumbing, with no single site
above 13 %.

## 2. Copy points

| Path | Where | Copies (Rust ↔ V8) | Class | Changed |
|---|---|---|---|---|
| Response body chunk, JS → Rust | `op_nextrs_response_write` (`#[buffer] &[u8]`) → `response.rs:161` `chunk.to_vec()` | 1 | **required by JS semantics**: the caller may reuse or mutate its `Uint8Array` after `write()` resolves; holding a view would need a detach (transfer), which changes JavaScript-visible behaviour | no |
| Fetch request body, JS → Rust | `op_nextrs_fetch_start` `#[buffer(copy)] Vec<u8>` | 1 | required (same reason; the body outlives the call) | no |
| Fetch upload chunks | `op_nextrs_fetch_upload` `#[buffer(copy)]` | 1 per chunk | required (same) | no |
| Request body to JS | `ToJsBuffer` (Box → backing store) | 0 (adopted) | adoption: **outside the ArrayBuffer budget** (RE-4.1E §2), bounded per chunk (≤ 64 KiB, streamed) | no |
| Fetch response chunks to JS | `ToJsBuffer` | 0 (adopted) | as above; ≤ `--net-max-response` | no |
| **Multipart body** (`formData()`) | `op_nextrs_multipart_parse` | **was 1 full copy of the body + multer's internal buffer + 1 per part; now multer's buffer + 1 per part** | the body copy existed only because the op was async (JavaScript could run while it was suspended); removable | **yes**, §3 |
| Multipart parts to JS | `data.to_vec().into()` | 1 per part | required: each part becomes a JavaScript-owned buffer; adopted, outside the budget | no |
| File reads | `op_nextrs_fs_read` returns `Vec` (adopted), then `Buffer.from(...)` in the polyfill | 1 (the polyfill's copy, budgeted) | the copy is what makes `readFileSync` **enforced** by the ArrayBuffer budget (RE-4.1E). Removing it would weaken enforcement | no |
| One-shot zlib | same pattern (`js/02_node.js:2052`) | 1 | same: enforced through the copy | no |
| `TextEncoder.encode`, `Buffer.from(string)` | deno_web / NextRS op returns a `Vec`, adopted | 0 | adoption is cheap but **unenforced** (RE-5H) | no |
| Cache hit value | `cache.rs:578` `from_utf8_lossy(..).into_owned()`, then serde_v8 makes a V8 string | 2 (Rust `String`, V8 string) | the Rust `String` copy is removable: serialize `&str` borrowed from the stored `Bytes`. **Not done:** each isolate already reuses its decoded value (`unchanged` lookups), so the copy happens once per isolate per entry generation, and the steady-state benefit is not measurable ([RE-5-CACHE-ALLOCATION-OPTIMIZATION](RE-5-CACHE-ALLOCATION-OPTIMIZATION.md)) | no |
| Cache set value | `#[string] Option<String>` → `Bytes::from(String)` | 1 (V8 string → UTF-8, unavoidable transcoding) | required | no |
| Prerendered and static bodies | Rust only (`Bytes` shared with hyper and the compression cache) | 0 | already shared and immutable; RE-5B changes where they live, not how they are shared | — |

## 3. The multipart change (`38e5659`)

Next decodes no-JavaScript Server Action forms with `formData()`.

- **Before**, an upload of N bytes held at once:
  - the JavaScript body (ArrayBuffer budget);
  - a full Rust copy (`Bytes::copy_from_slice`), which existed so the
    parser was immune to JavaScript changing the buffer during an
    `await`;
  - each part copied out (adopted by V8).
- **The parse never awaits.** Its only input is one ready chunk.
- **The op is now synchronous.** JavaScript cannot run until the op
  returns. The parser reads the `JsBuffer` through
  `Bytes::from_owner(InPlace(JsBuffer))`.
- **Ownership:**
  - the `JsBuffer` keeps its V8 backing store alive. It is created and
    dropped on the isolate thread, inside the op;
  - no reference escapes the op: every part is copied into its own
    buffer before return, and multer's stream is dropped;
  - there is no new `unsafe`;
  - a parser that ever waited, which it cannot, would get a TypeError,
    not a hang or a stale read.
- **Peak transient memory** for an upload falls from about 4 N to 3 N.
  The four copies were: the JavaScript body, the Rust copy, multer's own
  buffer (it copies its input into a `BytesMut`), and the parts. Only the
  Rust copy is gone (review F11).
- **Test:** `multipart_parts_are_independent_of_the_parsed_body`. A
  1 MiB file part, a UTF-8 field and 200 fields; the source array is
  overwritten after `formData()` returns, and no part changes. The
  Node-compared FormData checks are unchanged.

The Phase 3A `action-multipart` workload sends a few hundred bytes, so its
per-request count changes by about one allocation. The benefit scales with
upload size and is not visible in that workload.

## 4. Budgeted backing-store adoption (evaluated, not implemented)

Adoption with accounting would give Rust-made buffers (request chunks,
fetch chunks, multipart parts, `TextEncoder`) a reservation in the
ArrayBuffer budget:

- the v8 crate's `ArrayBuffer::new_backing_store_from_ptr` takes a
  deleter that V8 calls when it frees the store, on any thread;
- `ExternalBudget::reserve`/`release` are atomic and thread-safe.

The design would reserve, refuse (a RangeError) when over budget, and
release in the deleter. It is not implemented, because:

- **These paths are bounded today.** Per chunk (≤ 64 KiB), per response
  (`--net-max-response`), or by the heap (≤ 3 × the string's UTF-16
  length). The gap is the aggregate across concurrent requests.
- **Refusing inside an op changes JavaScript-visible behaviour.** A
  stream read or `formData()` would throw a RangeError where Node
  succeeds. Each call site needs its own compatibility decision.
- **A deleter that runs late or on another thread** must free with the
  allocator that allocated (Rust's global allocator, not V8's). That is
  a new `unsafe` FFI surface, worth its own reviewed change with
  sanitizer runs, not a side effect of an efficiency milestone.

It remains the right mechanism if container measurements show adopted
buffers matter. See [RE-5-EXTERNAL-MEMORY-EFFICIENCY](RE-5-EXTERNAL-MEMORY-EFFICIENCY.md).

## 5. Enforcement

No path became less enforced:

- the multipart body was budgeted before and is still;
- the removed copy was native and unbudgeted;
- parts are adopted as before.
