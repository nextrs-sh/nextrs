# RE-5H: external-memory efficiency

The allocation paths that RE-4.1E measured as unenforced or partly
enforced by `--js-external-memory-limit` (the budgeted V8
`ArrayBuffer::Allocator`), and what bounding each would take. This is
analysis only. **No enforcement was added; current behaviour is kept.**
The pinned engine is unchanged from RE-4.1: deno_core 0.413.0, v8 crate
150.4.0, deno_web 0.291.0.

## 1. Paths and options

| Path | Today | Bounded by | Option evaluated | Decision |
|---|---|---|---|---|
| `TextEncoder.encode(s)` | deno_web op returns a Rust `Vec`, adopted by V8: not counted | the string (heap limit): ≤ 3 × UTF-16 length | allocate the result with the budgeted allocator (`new Uint8Array(n)`) and fill it with `encodeInto` | not changed (§2) |
| `Buffer.from(string)` (utf8, base64, hex, latin1) | NextRS op returns a `Vec`, adopted | the string | same: exact length first (`Buffer.byteLength`), then allocate and write in place | not changed (§2) |
| base64 / hex decoding | as above | ≤ ¾ / ½ of the string | as above; the length is computable without decoding | not changed |
| Resizable `ArrayBuffer` growth | V8 reserves `maxByteLength` and commits through its page allocator, not the `ArrayBuffer::Allocator` | `maxByteLength` per buffer | none in supported APIs: the page allocator is not hookable through the v8 crate | documented, unchanged |
| Rust-adopted backing stores (request/fetch chunks, multipart parts, file and zlib op results before the polyfill's copy) | not counted | per chunk (≤ 64 KiB), per response (`--net-max-response`), body limits | budgeted adoption (`new_backing_store_from_ptr` + a deleter that releases) | not changed: [RE-5-BUFFER-AND-OWNERSHIP-OPTIMIZATION](RE-5-BUFFER-AND-OWNERSHIP-OPTIMIZATION.md) §4 |
| Allocations < 64 KiB | counted, never refused | count × size | none: refusing small allocations can abort V8 (`JSTypedArray::GetBuffer` has no failure path) | **the exemption stays** |
| Transient Rust buffers | not counted | one result per op | none needed | unchanged |

## 2. Allocator-backed encoding: cost against benefit

**The mechanism.** `TextEncoder.encode(s)` and `Buffer.from(s)` can be
made budget-enforced without new native code:

1. compute the exact UTF-8 length: deno_web's
   `op_encoding_encode_into`, or a length pass in Rust;
2. allocate `new Uint8Array(len)`. That allocation comes from the
   budgeted allocator, so a refusal is a RangeError, as for any
   `ArrayBuffer`;
3. `encodeInto` the string, writing in place.

**The cost:**

- one extra pass over the string (UTF-16 → length);
- two op calls instead of one;
- for one-byte (Latin-1) strings V8 already knows the length is at most
  2 × chars, so an upper-bound allocation plus a `subarray` would avoid
  the length pass. But the result would then be a view over a larger
  buffer, which is JavaScript-visible: its `byteOffset`/`buffer` differ
  from Node's.

**The benefit** is enforcement of a path the heap already bounds:

- a string must exist in the heap first;
- its UTF-8 encoding is at most 3 × its UTF-16 length;
- so the unenforced memory per call is at most 3 × something that
  `--js-heap-limit` already bounds;
- and it is garbage when unreferenced.

The aggregate is what is unbounded: many large encoded buffers retained
at once.

**Not changed in RE-5, because:**

- **No measurement shows these paths matter.** The Phase 3A routes encode
  small strings. Velasia's containers have not run (the Linux campaign
  is blocked), so there is no evidence of external-memory pressure from
  encoding.
- **Every `TextEncoder.encode` and `Buffer.from(string)` would pay the
  extra pass.** These are hot in React's streaming renderer: every chunk
  is encoded.
- **Compatibility risk.** `Buffer.from(string)` and pooled small
  `Buffer`s have Node-specific `byteOffset` behaviour. Changing the
  allocation path needs the Node-compared buffer tests extended first.

**What would justify it:** a container fault workload, or Velasia under
the campaign, showing external memory from encoded strings near the
budget. Then the change is small and local. A micro-benchmark of the
extra pass, on the React streaming path, must come with it.

## 3. What this means for configured limits

`--js-external-memory-limit` remains a bound on allocator-made buffers
of at least 64 KiB. These sit outside it:

- encoded strings;
- resizable growth;
- adopted Rust buffers;
- small allocations, which are counted but never refused.

[RE-5-RESOURCE-PROFILE-DESIGN](RE-5-RESOURCE-PROFILE-DESIGN.md) counts
them in the envelope as unenforced, estimated demand, not as covered by
the budget.
