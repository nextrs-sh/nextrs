# RE-4.1E: external-memory coverage

This report audits which allocation paths `--js-external-memory-limit`
enforces, against the pinned engine (deno_core 0.413.0, v8 crate 150.4.0,
deno_web 0.291.0). The audit is measured, not inferred, and **corrects
RE-4** for four APIs. No enforcement was added. §3 explains why, and
what it would take.

## 1. Measured coverage

`crates/nextrs-runtime/tests/external_memory.rs`,
`no_buffer_api_aborts_the_process_when_the_budget_is_exhausted`
(commit `501a68f`). A child process fills a 16 MiB budget, first with
1 MiB buffers and then with 64 KiB buffers, so **less than the refusable
size is left**. It then runs each API. A ≥ 64 KiB result that still
succeeds did not come from the budgeted allocator.

RE-4's sweep stopped at the first refused 1 MiB buffer and left up to
1 MiB of headroom. Its 200 kB cases fitted in that headroom and looked
unenforced. The classification below is now asserted by the test and
stable in 5 of 5 runs; no API aborted the process.

| Path | With < 64 KiB left | Why | RE-4 said |
|---|---|---|---|
| `new ArrayBuffer`, typed arrays, `DataView`, `Buffer.alloc`/`allocUnsafe`/`concat`, `structuredClone`, `crypto.getRandomValues` (≥ 64 KiB) | **RangeError** | allocator-made | same |
| `fs.readFileSync` (128 KiB, raw or `"utf8"`) | **RangeError** | the polyfill returns `Buffer.from(op result)`, a copy made by the allocator (`js/02_node.js:478`) | not counted |
| one-shot `zlib` (`gunzipSync` to 2 MiB) | **RangeError** | `Buffer.from(op_nextrs_zlib(...))` (`js/02_node.js:2052`) | not counted |
| `Response.arrayBuffer()`, `Blob.arrayBuffer()` (200 kB) | **RangeError** | the result buffer is allocator-made | not counted |
| `TextEncoder.encode` (200 kB) | **granted** | deno_web's op returns a Rust `Vec`, adopted by V8 | same |
| `Buffer.from(string)` (2 MiB) | **granted** | encoded by a Rust op, adopted | — |
| resizable `ArrayBuffer`, grown to 2 MiB | **granted** | V8 reserves and commits it through its page allocator, not the `ArrayBuffer::Allocator` | committed bytes only, not growth |
| < 64 KiB allocator allocations | granted, **counted** | refusing them can be fatal (`JSTypedArray::GetBuffer`) | same |
| `JSON.stringify`, strings | not ArrayBuffer memory | JS heap (`--js-heap-limit`) | same |

## 2. What remains unenforced, and its bound

| Path | Bound today |
|---|---|
| `TextEncoder.encode`, `Buffer.from(string)` (also `base64`/`hex` decoding) | the string's size, which the heap limit bounds: output ≤ 3 × the UTF-16 length |
| Resizable ArrayBuffers (`maxByteLength`) | `maxByteLength` per buffer; V8 caps it |
| Transient Rust-made buffers behind the polyfills' copies (file read, zlib output, `Response` body chunks) | one result, until the next GC. The file's size; zlib's `maxOutputLength` (1 GiB default); the body limits |
| Request-body chunks to JavaScript | ≤ 64 KiB per chunk, streamed |
| Outbound fetch chunks | per chunk; the response ≤ `--net-max-response` (32 MiB) |
| Rust-adopted op results in general (`#[buffer]` → `new_backing_store_from_vec`) | per op |
| Allocations < 64 KiB | counted; `used` can exceed `limit` |
| V8-internal native memory (compiler, code space) | none |

## 3. Why no new enforcement here

**Rust-adopted buffers.** Two pieces exist in supported APIs:

- the v8 crate's `ArrayBuffer::new_backing_store_from_ptr`, whose deleter
  callback runs when V8 frees the store, on any thread;
- `ExternalBudget::reserve`/`release`, atomics only, already safe from
  any thread.

Together they could reserve before allocating and release with the
deleter. It is not done in this milestone because:

- **The remaining unbounded-looking paths are bounded by the heap.**
  `TextEncoder` and `Buffer.from(string)` output is proportional to a
  string that already lives on the heap. File reads and zlib are already
  enforced at their copy.
- **The paths that would need it are deno_web's** (`op_encoding_encode`
  and friends). Changing their return conversion means patching deno_web
  or replacing its ops: a compatibility change to a pinned dependency.
- **The removable cost is small.** The transient copy, which doubles a
  file read briefly, could be removed by returning the adopted buffer
  from `readFileSync` directly. But that would make readFileSync
  *unenforced*, the opposite of the aim. Keeping the copy keeps the
  enforcement.

**Small allocations** stay never-refused. V8 aborts the process when it
cannot materialize an on-heap typed array's buffer (≤ 64 bytes), and
refusing near that size is not safe. 64 KiB keeps a factor of 1000.

**Resizable buffers** go through V8's page allocator
(`BackingStore::TryAllocateAndPartiallyCommitMemory`). There is no
embedder hook short of a custom `v8::PageAllocator`, which the v8 crate
does not expose.

**No unsupported hook was added, and none of the reporting APIs is
treated as enforcement.** `AdjustAmountOfExternalAllocatedMemory` and
`external_memory()` inform GC only. The allocator's cross-isolate
accounting, the CAS reservation and the thread-safety rules are
unchanged (`external.rs`).

## 4. If more is needed (RE-5 candidates)

1. A NextRS replacement of `TextEncoder.encode` / `Buffer.from(string)`
   that encodes into an allocator-made buffer (JS `new Uint8Array(n)` +
   `encodeInto`). Then it is refusable without unsafe code. The cost is
   one extra length pass, to be measured.
2. A budgeted adoption helper (`new_backing_store_from_ptr` + deleter)
   for NextRS's own `#[buffer]` ops, for paths where a copy must be
   avoided.
3. Count resizable buffers' `maxByteLength` as a reservation at creation
   (conservative), if applications use them in practice.

**Verdict:** the ArrayBuffer budget enforces every allocator-made buffer
of 64 KiB or more, which, measured, includes `fs.readFileSync`, one-shot
zlib and `Response`/`Blob` bodies. Not enforced: `TextEncoder.encode`,
`Buffer.from(string)`, resizable-buffer growth, allocations under
64 KiB, and the transient Rust-made buffers behind the copies. All are
documented and bounded by other limits, as listed.
