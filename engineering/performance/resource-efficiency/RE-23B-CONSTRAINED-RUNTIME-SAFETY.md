# RE-23B: constrained runtime safety

**Status: partial.** The heap-limit fault tests ran at the process level
on the Fedora host, outside any container. The container-level tests
(native exhaustion at a cgroup limit, concurrent SSR, queue saturation,
slow clients, several isolates sharing 256 MiB) are blocked until a
native Docker engine is available
([RE-23B-LINUX-ENVIRONMENT](RE-23B-LINUX-ENVIRONMENT.md)). Lifecycle and
admission were not changed.

## Setup

- `c65731b` (`main`), with V8 from the pinned prebuilt `rusty_v8`
  (release-built, so its heap behaviour is the same in a debug test
  binary).
- One isolate per process, created with an explicit heap limit
  (`Limits::heap_limit_bytes`) and no stall limit.
- Each case runs in its own process, because a fatal V8 OOM aborts the
  whole process.
- After the fault, the driver checks that the isolate was replaced and
  that the next request gets a 200. It also records the process's peak
  resident memory (`VmHWM`). The smallest case (one 8 MiB array) peaks
  at 68 MiB, which is the process's own floor plus that array.
- Sources: [`results/re23b-safety/src/`](results/re23b-safety/src/)
  (`re23b-faults.js`, `re23b_faults.rs`). Every case, including its exit
  status and V8's fatal message, is in
  [`results/re23b-safety/results.txt`](results/re23b-safety/results.txt).

Modes:

| Mode | Allocation |
|---|---|
| `heap-string` | `"a".repeat(n).toUpperCase()`: one flat one-byte string of n MiB, made from a flattened n MiB source, so about 2n is live at the peak |
| `heap-single` | `new Array(n·131072).fill(0.5)`: one n MiB elements store (the fixture behind the existing `oom-large` test), about 2n at the elements-kind transition |
| `external` | 32 MiB `ArrayBuffer`s, filled, held until n MiB: native memory outside the JS heap |

## Results

At a 64 MiB heap limit, the limit `auto` reaches when budgeting two
isolates into 256 MiB:

| Mode | n (MiB) | Outcome | Process peak |
|---|---|---|---|
| `heap-single` | 8, 24, 48 | served (fits) | 68–147 MiB |
| `heap-single` | 60, 72, 96, 128, 160 | **isolate replaced** (`heap_limit`), next request 200 | 169–368 MiB |
| `heap-single` | 192, 256 | **process aborted**: `Fatal JavaScript out of memory: Reached heap limit` (exit 133) | — |
| `heap-string` | 24, 48 | served | 99–148 MiB |
| `heap-string` | 72, 96, 128, 144, 160, 176 | **isolate replaced**, next request 200 | 192–400 MiB |
| `heap-string` | 192, 224, 256 | **process aborted** (exit 133) | — |
| `external` | 256, 512 | **served**: no limit applies | 307, 563 MiB |

At a 128 MiB heap limit (`heap-string`): 96 MiB served (243 MiB peak);
128 and 192 MiB replaced the isolate (304, 432 MiB peaks); 256 MiB
aborted the process.

## What this shows

1. **One allocation can end the process.** At both limits tested, a
   single allocation of 192 MiB (at a 64 MiB limit) or 256 MiB (at a
   128 MiB limit) aborted the process instead of replacing the isolate.
   That kills every isolate and every in-flight request. The embedder
   cannot catch it.
2. **The 64 MiB grant lets through more than one 64 MiB object.** Objects
   of up to 160–176 MiB were recovered at a 64 MiB limit, with the
   process peaking at up to 400 MiB (6× the heap limit) before the
   termination landed. The model in
   [RE-3-V8-HEAP-BUDGETING](RE-3-V8-HEAP-BUDGETING.md) (V8 calls the
   callback once and retries once, so the grant must fit the object)
   predicts a lower boundary. The callback returns `current + 64 MiB`
   every time it is called; whether V8 calls it more than once per
   episode was not instrumented. Either way, **the grant does not bound
   how far the heap overshoots its limit.**
3. **The JS heap budget does not bound process memory.** 512 MiB of
   `ArrayBuffer` backing stores were accepted at a 64 MiB heap limit.
   Native memory (array buffers, request and response bodies held in
   Rust, the compression cache, allocator retention) sits outside the
   heap limit. Pressure reclamation can return free memory, but it
   cannot refuse an allocation.
4. **Inside a 256 MiB container, "recoverable" mostly becomes an OOM
   kill.** With the budget at 1 isolate and a 32 MiB cache, the heap
   limit is 112 MiB, and the non-heap remainder is about 65–100 MiB
   (25 MiB live native, 37–42 MiB binary pages, up to 32 MiB cache;
   RE-2/3). Any single request that allocates more than about 40–80 MiB
   at once while the heap is near its limit would take the container
   over 256 MiB before V8 terminates the isolate. That ends the process,
   just as an abort does. This is arithmetic from the measured
   overshoot; it was not measured in a container. Gradual growth from
   many small objects (Velasia's SSR) terminates close to the limit,
   which is why RE-2/3 saw no OOM in 13 runs.

## What can be recovered and what is fatal

| Failure | Result | Measured |
|---|---|---|
| Heap reaches its limit through many small allocations | isolate replaced, its in-flight requests fail, the process keeps serving | RE-2/3 (`oom` test), here |
| One large heap object, up to about 160–176 MiB at a 64 MiB limit | isolate replaced, but the process peak overshoots by up to 6× the limit | here |
| One heap object at or above 192 MiB (64 MiB limit) or 256 MiB (128 MiB limit) | **process abort** (V8 `FatalProcessOutOfMemory`) | here |
| Native and external memory beyond the container | **cgroup OOM kill** of the process | RE-1 (`base`), not repeated here |
| Any overshoot past `memory.max` in a container | **cgroup OOM kill** | arithmetic (above); container test blocked |
| Failed V8 page reservation | **process abort** (documented in RE-3, not provoked) | — |
| CPU stall, endless microtask chain, executor panic | isolate replaced | existing `supervision.rs` tests |

## Not measured (blocked: needs a native container engine)

| Brief item | Existing bound | Open question |
|---|---|---|
| Native memory exhaustion at 256 MiB | none for native memory; pressure reclamation from 90 % | when `memory.current` reaches `memory.max`, is the result an OOM kill or a recovery? |
| Multiple concurrent SSR requests | `--max-concurrent` 128 per isolate | how many concurrent Velasia renders fit a 112 MiB heap |
| Queue saturation | `--max-queued` 512, `--queue-timeout` 10 s | memory per queued request (request bodies are not read until dispatch) |
| Slow streaming clients | `--request-timeout` 60 s; HTTP backpressure | memory held per stalled response stream |
| Several isolates sharing 256 MiB | the budget clamps each heap to 64 MiB (with a warning) | overshoot with two heaps near their limits at once |

## Is the policy conservative enough?

- **For gradual heap growth: yes.** That is the Velasia case, and the
  RE-2/3 data show it.
- **For large single allocations and native memory: no.** Both can end
  the process in a 256 MiB container, and nothing in the heap budget
  stops either.
- **The heap budget is not a process memory guarantee,** and it must not
  be described as one.

## Next milestone: admission control or CPU?

The evidence points to **resource-aware admission control** as the next
implementation milestone. The remaining 256 MiB failure modes are
process-fatal, and reclamation cannot prevent them: native memory, large
allocations near the heap limit, and the unmeasured concurrency and
queue cases. Admission control would act on `memory.current` and the
heap state before dispatching a request.

The CPU cost has a known and cheap remedy: a young-generation floor set
through the supported `ResourceConstraints` setter that `heap_params`
already uses, with no V8 flags (see the gate report).

This recommendation is provisional. It should be confirmed by the
container-level fault tests above, and by the Linux CPU measurements,
before RE-4 is scoped.

Small, test-only follow-ups (not done here):

- Count near-heap-limit callback invocations per episode, to explain
  item 2.
- Add a regression test asserting that a single allocation larger than
  the fatal boundary aborts. It would need a subprocess. This documents
  behaviour rather than gating it.
