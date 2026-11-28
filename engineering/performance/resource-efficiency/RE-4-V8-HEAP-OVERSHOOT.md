# RE-4E: V8 near-limit behaviour and the heap overshoot

RE-23B measured that one large allocation at a 64 MiB heap limit can
push the process to 400 MiB before the isolate is replaced, and that
192 MiB aborts the process. This milestone instrumented the
near-heap-limit callback to explain why, and added child-process
regression tests for the recoverable and fatal cases.

**Process-level measurements, not container measurements.** Each case
ran in its own process on the Fedora host, outside any cgroup limit
([results/re4-heap-overshoot](results/re4-heap-overshoot/SUMMARY.md)).

## Instrumentation added

Per isolate (`pool.rs` `on_near_heap_limit`, supervisor):

| Recorded | Where |
|---|---|
| every callback call, with the old-generation limit V8 passes (`current`), the configured one, the grant, the heap live after the last full GC, external memory | `warn` log `V8 heap near its limit` |
| calls and declined calls, per slot over its life | `ExecutorSnapshot.near_heap_limit_calls`, `near_heap_limit_declined` |
| per episode: calls, first and last limit passed, time from the termination request to the thread's exit (unwind), and to a replacement being ready | `ExecutorSnapshot.last_heap_limit` (`HeapLimitEpisode`), `warn` log `heap-limit termination finished`, `info` log `isolate replaced after reaching its heap limit` |
| peak process memory | the test driver (`VmHWM`) |

## Results at a 64 MiB heap limit

The old-generation limit is 58 MiB (the young generation takes the rest).
The first and the last limit passed to the callback are recorded; a
middle value follows from the callback returning `current + 64 MiB`.

| Mode | n MiB | Outcome | Callback calls | Old-generation limit at each call (MiB) | Unwind ms | Replaced after ms | Process peak MiB |
|---|---:|---|---:|---|---:|---:|---:|
| `heap-single` | 8–48 | served | 0 | — | — | — | 67–147 |
| `heap-single` | 60 | isolate replaced | 1 | 58 | 78 | 136 | 169 |
| `heap-single` | 72 | isolate replaced | 1 | 58 | 68 | 125 | 193 |
| `heap-single` | 96 | isolate replaced | 1 | 58 | 122 | 180 | 241 |
| `heap-single` | 128 | isolate replaced | 2 | 58 → 122 | 378 | 435 | 304 |
| `heap-single` | 160 | isolate replaced | 2 | 58 → 122 | 409 | 467 | 369 |
| `heap-single` | 176 | isolate replaced | 3 | 58 → 122 → 186 | 501 | 558 | 400 |
| `heap-single` | 192, 256 | **process aborted** (`Fatal JavaScript out of memory: Reached heap limit`, exit 133) | — | — | — | — | — |
| `heap-string` | 8–48 | served | 0 | — | — | — | 68–147 |
| `heap-string` | 60 | isolate replaced | 1 | 58 | 34 | 91 | 169 |
| `heap-string` | 72 | isolate replaced | 1 | 58 | 39 | 97 | 193 |
| `heap-string` | 96 | isolate replaced | 1 | 58 | 51 | 108 | 240 |
| `heap-string` | 128 | isolate replaced | 2 | 58 → 122 | 75 | 133 | 303 |
| `heap-string` | 160 | isolate replaced | 2 | 58 → 122 | 89 | 147 | 367 |
| `heap-string` | 176 | isolate replaced | 3 | 58 → 122 → 186 | 114 | 172 | 400 |
| `heap-string` | 192, 256 | **process aborted** (exit 133) | — | — | — | — | — |

At a 128 MiB limit (old generation 116 MiB), `heap-string`: 96 served
(242 MiB peak); 128 replaced after 1 call (304); 192 replaced after 2
calls, 116 → 180 (432); 256 aborted.

Every peak is within 1 MiB of RE-23B's, which measured the same cases
without the instrumentation, so the instrumentation does not change the
behaviour.

## What explains the overshoot

1. **Repeated grants within one episode.** V8 calls the callback more
   than once before the termination takes effect: once per allocation
   attempt that fails at the current limit. Each call returns
   `current + 64 MiB`, and `current` already includes the earlier grants.
   The limit therefore grows by 64 MiB per call: 58 → 122 → 186 MiB
   before the isolate dies. That, not a single grant, is what lets
   160–176 MiB objects through and puts the process at 6× the limit.
2. **Termination is not immediate.** The callback requests termination
   at the first call, but V8 acts on it only at the next interrupt check
   in JavaScript, after the allocation in progress completes. Until then
   the allocation runs to completion with whatever limit it needs. The
   unwind took 34–122 ms for one-call episodes and up to 501 ms for
   three calls (debug-built Rust code around a release V8).
3. **The fatal boundary is where the grants stop being enough.** In this
   V8 version, when an allocation still does not fit after the last
   collection and call, V8 calls `FatalProcessOutOfMemory` (heap.cc
   `CheckHeapLimitReached`, heap-allocator.cc `RetryCustomAllocateOrFail`).
   No more than 3 calls were observed in one episode. Objects that need
   more (192 MiB here) abort the process. Doubling-style allocations
   (`toUpperCase` of a flat string holds the source and the result)
   reach that boundary at about half the object size you would expect.
4. **Not explained by:** the size of the grant alone (one grant would
   cap the peak near 58 + 64 MiB plus the object), or the young
   generation, which is a few MiB here.

Which V8 path produces the second and third call is **not identified**:
the callback has no stack. The count, the limits passed and the
timings are measured.

Two further findings:

5. **The callback was also called when the heap was not full.** V8's
   last-resort collection (`Heap::CollectAllAvailableGarbage(kLastResort)`,
   heap.cc:1319) calls the near-heap-limit callback for *any* failed
   allocation, including an ArrayBuffer the new external budget refused.
   Before the fix, such a refusal replaced the isolate instead of throwing
   a RangeError.

   The callback now declines (keeps the limit, keeps the isolate) only
   when both of these hold:
   - this thread's ArrayBuffer budget refused an allocation, with none
     granted since, within the last second;
   - the heap is clearly below its limit: committed old generation (every
     space but the young generation's, read with `GetHeapSpaceStatistics`
     in the callback, as Node.js's callback does) below 90 % of the limit,
     **and** all committed heap below the limit.

   V8's other two calls (`CheckHeapLimitReached`, ineffective
   mark-compacts) happen only when the old generation's *capacity* has
   reached the limit (`CanExpandOldGeneration`), and committed memory is
   at least that. So those calls are never declined; declining them
   would abort the process.

   The first version of this rule compared the *live* heap with half the
   limit. The independent review showed that it replaced the isolate
   whenever the heap was more than half full ([RE-4-INDEPENDENT-REVIEW](RE-4-INDEPENDENT-REVIEW.md)).
   With the committed-heap rule, at a 64 MiB limit (58 MiB old
   generation) and 10–45 MiB of small objects held, a refused 64 MiB
   ArrayBuffer is a RangeError with no restart. From 48 MiB held (about
   90 % committed) the isolate is replaced, as for a full heap
   ([results/re4-heap-overshoot/retain-then-refuse.txt](results/re4-heap-overshoot/retain-then-refuse.txt)).

   Covered by `heap_limits.rs`
   `a_full_heap_right_after_an_external_refusal_is_still_replaced`,
   `an_external_refusal_alone_declines_the_callback_and_keeps_the_isolate`
   and `a_refusal_with_a_half_full_heap_is_a_range_error_not_a_restart`.
6. **Replacement time** after a heap-limit termination: 91–558 ms here,
   for a test bootstrap. A Next.js bootstrap is longer, and requests
   arriving meanwhile wait in the queue ([RE-23-CONSTRAINED-CONTAINER-VALIDATION](RE-23-CONSTRAINED-CONTAINER-VALIDATION.md)).

## What was not changed, and why

- **The 64 MiB grant.** Reducing it was ruled out by the brief and by
  RE-3: a single object larger than the grant then aborts the process.
- **Grant once per episode.** It would cap the overshoot at one grant, but
  a 128–176 MiB object (recovered today) would then abort the whole
  process instead of replacing one isolate. Inside a 256 MiB container
  both outcomes are probably fatal to the process: one is an abort, the
  other a cgroup OOM kill (arithmetic from these peaks, not measured in a
  container). Outside a container, or with room to spare, today's
  behaviour recovers. Choosing between them needs the container fault
  tests, so the behaviour is unchanged and recorded here.

## Regression tests (child processes)

`crates/nextrs-runtime/tests/heap_limits.rs`, one case per child process:

| Test | Asserts |
|---|---|
| `a_recoverable_large_allocation_replaces_the_isolate_and_is_recorded` | 96 MiB string at 64 MiB: child exits normally, 1 restart, 1 `heap_limit` failure, an episode with ≥ 1 call and a replacement time, next request 200 |
| `an_allocation_past_the_fatal_boundary_aborts_its_process` | 256 MiB at 64 MiB: the child aborts with V8's out-of-memory message. Documents behaviour; it does not gate it |
| `a_full_heap_right_after_an_external_refusal_is_still_replaced` | refusal then gradual heap growth: replaced, not aborted |
| `an_external_refusal_alone_declines_the_callback_and_keeps_the_isolate` | RangeError, no restart, ≥ 1 declined call |
| `a_refusal_with_a_half_full_heap_is_a_range_error_not_a_restart` | 10, 25 and 35 MiB of small objects held, then a refused 64 MiB ArrayBuffer: RangeError, no restart |

A child case is a no-op unless its parent started it
(`NEXTRS_TEST_CHILD`), so `--ignored` runs in CI never execute a fatal
case in-process.

## Answer

Fatal V8 failures can still terminate the process. In this V8 version,
any single allocation that needs more than about three 64 MiB grants
above the limit is fatal, and nothing the embedder does can catch it.
Recoverable large allocations overshoot because the callback is called
repeatedly and every call raises the limit again. The heap limit is
therefore **not** a memory bound. It must not be documented as one.
