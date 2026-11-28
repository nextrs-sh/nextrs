# RE-4D: external memory

How the pinned engine (deno_core 0.413.0, v8 crate 150.4.0) allocates
and accounts ArrayBuffer memory, whether a limit can be enforced through
supported interfaces, and what NextRS now enforces
(`--js-external-memory-limit`, off by default).

RE-23B: 512 MiB of `ArrayBuffer`s were accepted at a 64 MiB heap limit.
The heap limit does not cover them.

## 1. How backing stores are allocated

| Path | Allocator | Accounted by V8 as external memory | Examples |
|---|---|---|---|
| JavaScript creates the buffer | the isolate's `ArrayBuffer::Allocator` | yes | `new ArrayBuffer(n)`, `new Uint8Array(n)` and other typed arrays, `Buffer.alloc`/`allocUnsafe`/`concat`, `ArrayBuffer.prototype.slice`, `structuredClone` of buffers, `DataView` buffers |
| Rust hands a buffer to JavaScript | none: V8 adopts Rust memory with a deleter (`new_backing_store_from_vec`/`_boxed_slice`) | yes | every NextRS `#[buffer]` op result (request-body chunks, outbound fetch chunks, `fs.readFileSync`, hashes, zlib), `ToJsBuffer` |
| Rust asks V8 for a buffer | the allocator, through `ArrayBuffer::New`/`NewBackingStore` | yes | in the dependencies, not on NextRS's request path: `serde_v8` `serialize_bytes` (no NextRS op returns bytes through it), `deno_web` `image_data.rs` (`ImageData`, unused by Next's server), test-only helpers |
| On-heap typed arrays (≤ 64 bytes) | the JS heap, until `.buffer` materializes them through the allocator | — | small `Uint8Array`s |

`deno_core` passes the embedder's `CreateParams` through unchanged
(`runtime/setup.rs` `create_isolate`). The v8 crate installs V8's default
allocator only when none was set (`isolate_create_params.rs:288`). So an
embedder allocator can be installed per isolate without changing either
crate.

## 2. What the supported interfaces allow

| Interface | Enforces a limit? |
|---|---|
| `Isolate::AdjustAmountOfExternalAllocatedMemory` | **no**: it informs V8's GC heuristics. Reporting external memory never refuses an allocation |
| V8 heap limit / near-heap-limit callback | **no**: external memory is outside it. With `enforce_global_heap_limit` V8 can count it toward a global limit, but that flag is V8-internal and its failure mode is `FatalProcessOutOfMemory` |
| `v8::ArrayBuffer::Allocator` (v8 crate `new_rust_allocator`, a `RustAllocatorVtable` of allocate / allocate_uninitialized / free / drop) | **yes**, for allocations made through it: returning null is the documented failure signal |
| V8 sandbox mode | not enabled in the pinned build (`v8_enable_sandbox` off). With it, `new_rust_allocator` is unavailable and backing stores must live inside the sandbox |

### What V8 does when the allocator returns null

From the vendored V8 source:

- `BackingStore::Allocate` → `Heap::AllocateExternalBackingStore`. On
  failure V8 runs `RetryCustomAllocate`: garbage collections and a
  retry, then `CollectAllAvailableGarbage(kLastResort)` and a final
  retry. **Dead buffers are freed before a refusal is final.**
- JavaScript constructors (`builtins-arraybuffer.cc` `TryAllocateBackingStore`)
  then throw **`RangeError: Array buffer allocation failed`**: an
  ordinary, catchable exception.
- **Fatal on refusal** (`FatalProcessOutOfMemory`):
  - `JSTypedArray::GetBuffer`: materializing an on-heap typed array,
    which is at most 64 bytes (`V8_TYPED_ARRAY_MAX_SIZE_IN_HEAP`);
  - `ArrayBuffer::New` / `NewBackingStore` called from Rust with the
    default failure mode (the dependency paths in §1).
- The last-resort collection also calls the **near-heap-limit callback**,
  whatever the heap's size (heap.cc:1319). NextRS's callback treated every
  call as a full heap and replaced the isolate
  ([RE-4-V8-HEAP-OVERSHOOT](RE-4-V8-HEAP-OVERSHOOT.md) §5).

## 3. What was implemented

`crates/nextrs-runtime/src/external.rs`, enabled by
`Limits::external_memory_limit_bytes` / `--js-external-memory-limit`:

- One budget per executor, shared by all its isolates. Each isolate gets
  a `RustAllocator` holding one `Arc` reference to it, released by the
  vtable's `drop` when V8 destroys the allocator.
- The allocator uses glibc's `calloc`/`malloc`/`free`, as V8's default
  allocator does. Only the accounting is new, and glibc remains the
  allocator.
- The functions only touch atomics and libc. They never call into V8 and
  never log ("unsafe to call back into V8 from any of the allocator
  functions").
- **Allocations below 64 KiB are counted but never refused.** This
  covers the fatal on-heap path (64 bytes) by a factor of 1000.
- An allocation of 64 KiB or more that would cross the limit is refused.
  The reservation uses a compare-and-swap loop, so concurrent isolates
  cannot together exceed the limit through refusable allocations.
- The near-heap-limit callback declines only after a recent refusal
  (none granted since) *and* with the committed heap clearly below its
  limit: old generation under 90 %, everything under 100 %. That makes a
  refusal a RangeError, not a restart, unless the heap is itself nearly
  full ([RE-4-V8-HEAP-OVERSHOOT](RE-4-V8-HEAP-OVERSHOOT.md) §5).
- Metrics: `nextrs_external_memory_{limit,used,peak}_bytes`,
  `_refused_total`, `_refused_bytes_total`.

## 4. Measured (process level, Fedora host, debug test binary)

`crates/nextrs-runtime/tests/external_memory.rs`:

| Case | Result |
|---|---|
| 64 MiB `ArrayBuffer` under a 32 MiB limit | `RangeError` in the handler; isolate not replaced; next request on the same isolate succeeds |
| 40 × 16 MiB short-lived `Uint8Array`s (640 MiB in one turn) under 48 MiB | succeeds: V8 collects the dead ones before each would-be refusal. Peak budget use ≤ 48 MiB |
| 8 MiB buffers kept until refused, under 64 MiB | 48–64 MiB held, then `RangeError`; after release, a 32 MiB allocation succeeds |
| two isolates under one 64 MiB limit, 40 MiB held by one | the other cannot take 40 MiB (`RangeError`): one budget for all |
| every isolate replaced | budget use back under 8 MiB (the replaced isolates' buffers are freed) |
| `external` fault mode, 128 MiB at a 64 MiB external limit and 64 MiB heap (`heap_limits.rs`) | `RangeError`, no restart, ≥ 1 declined callback |

**API sweep with the budget exhausted (child process, so an abort would
be caught):** no API aborted the process.

| API | With the budget exhausted |
|---|---|
| `new Uint8Array(16).buffer`, `Buffer.alloc(1024)`, `Buffer.from(string)`, `Buffer.concat` of small buffers, `ArrayBuffer.prototype.slice` (small), `structuredClone` of 32 bytes, `crypto.getRandomValues(64 KiB)`, `JSON.stringify` of 100 kB | works: below 64 KiB never refused; the 64 KiB `getRandomValues` buffer is refusable, but fitted in the headroom left when the fill stopped at its first refused 1 MiB buffer |
| `new Uint8Array(1 MiB)`, `Buffer.alloc(1 MiB)`, `Buffer.allocUnsafe(1 MiB)`, `Buffer.concat` reaching 1 MiB, `structuredClone` of 1 MiB, `new DataView(new ArrayBuffer(1 MiB))` | `RangeError` |
| `new ArrayBuffer(10, { maxByteLength: 4 MiB })` | works (only the 10 bytes are committed) |
| `new TextEncoder().encode(200 kB)`, `Response.arrayBuffer()` and `Blob.arrayBuffer()` of 200 kB | **work**: their bytes come from Rust ops (`from_vec`), outside the budget |

## 5. Hard, best-effort, V8 and kernel limits

| Kind | Examples | Guarantee |
|---|---|---|
| **Hard, enforced by NextRS** | the ArrayBuffer budget (allocator-made buffers ≥ 64 KiB); the native buffer budget (RE-4C); the image budgets; per-request body, response-buffer and fetch-response limits; cache capacities | the allocation is refused before it happens |
| **Best effort, admission** | memory-aware admission (RE-4B) | refuses requests by measured memory and estimates; an admitted request can exceed its estimate |
| **V8 heap constraint** | `--js-heap-limit` | the isolate is terminated after the limit is crossed; the overshoot is not bounded, and very large single allocations abort the process |
| **Kernel** | cgroup `memory.max` | the OOM killer ends the process |

## 6. Limitations

- **Coverage is partial.** Buffers adopted from Rust are not counted:
  request and fetch body chunks (bounded per chunk and per request by
  their own limits), `fs.readFileSync` (unbounded), `TextEncoder`, and
  `Response`/`Blob` bodies. V8 still counts them in `external_memory()`.
- Small allocations below 64 KiB are never refused, so many small
  buffers can exceed the limit. They are counted, so `used` can be above
  `limit`.
- A refusal costs garbage collections (V8's retry), so a request that
  keeps hitting the limit spends CPU on full GCs before its RangeError.
- With the heap at 90 % or more of its limit when a refusal happens, the
  isolate is replaced instead (it is nearly full anyway).
- A refusal's flag lasts until the next granted large allocation or one
  second. A *different* allocation failure in that window, with the
  heap clearly below its limit (for example the system's own memory
  exhausted), would be declined at the last-resort call. For a heap
  allocation V8 then fails the retry, which is fatal with or without the
  decline. Not observed, not provoked.
- Not measured in a container. Whether the limit prevents the RE-1 OOM
  kill depends on the application's mix of allocator-made and
  Rust-adopted buffers.

## Answer

V8 offers no hook that limits external memory as a whole. A custom
`ArrayBuffer::Allocator`, through the supported v8 crate API, can safely
enforce a limit on the buffers JavaScript allocates. Refusals become
RangeErrors once NextRS's near-heap-limit callback tells them apart from
a full heap. Allocations below 64 KiB, and buffers made by Rust, stay
outside the enforced part.
