# RE-4: independent review

A separate reviewer was given the code diff (`git diff main...HEAD --
crates/`) and the requirements, not the reports or the author's
conclusions. It read the code, checked engine behaviour against the
vendored V8 150.4.0 source, and ran code: the test suites repeatedly,
and an outside probe crate built against `nextrs-runtime` to reproduce
V8 behaviour. It modified nothing in the repository.

## Findings and resolution

| # | Severity | Finding | Resolution |
|---|---|---|---|
| 1 | **High** (reproduced) | With the ArrayBuffer budget on, a refused allocation **replaced the isolate** when the JS heap was more than about half full: the near-heap-limit decline rule compared the *live* heap with half the limit. Reproduced at 35 MiB of small objects held under a 64 MiB limit. A Next.js app above 50 % of its heap would turn every refusal into a restart that fails all in-flight requests | **Fixed.** The callback now reads the committed heap-space sizes. It declines only when the refusal is pending (cleared by the next granted large allocation, at most 1 s), the committed old generation is below 90 % of the limit, and all committed heap is below the limit. That bounds V8's own `CanExpandOldGeneration` test from above, so the genuine call sites are never declined. New child-process test with 10, 25 and 35 MiB held: RangeError, no restart. Sweep: RangeError up to 45 MiB held, replacement from 48 MiB (heap ≈ 90 % committed) |
| 2 | Medium (confirmed, 5 of 15 runs) | `elevated_caps_concurrency_and_the_rest_run_in_arrival_order` was flaky: spawned tasks reached `acquire` in any order. The test was wrong, not the pool | **Fixed.** Waiters are enqueued one after another (wait for `queued == n` after each). 15/15 runs |
| 3 | Medium (from reading) | Next.js's in-process requests to its own server go through admission. With the gate on, a parent request counts as active, so its child could be deferred (cap 4 per isolate) or heap-skipped, and with one isolate it could stall for `queue_timeout`. At critical, the child would be refused and a Server Action would fail half way | **Fixed.** In-process self-requests run in a task-local "nested" scope (`admission::nested`, set in `internal.rs`), and the mark travels on their queued ticket. They bypass the memory gate and the heap skip; concurrency limits and `MAX_INTERNAL_REQUESTS` still apply. Unit test for the scope. The end-to-end path (a Server Action calling its own server under pressure) is not exercised by a local test |
| 4 | Low–medium (theoretical; not triggerable in the reviewer's attempts) | The old decline rule's one-second window was never consumed, so a genuine last-resort call within the window with the live heap under half could be declined | **Fixed** with 1: the refusal is consumed by the decline and cleared by a granted allocation, and declines need committed memory below the limit, which V8's limit test cannot then report as reached |
| 5 | Low | A request retried after `Unavailable` keeps its dead slot while it re-acquires. With the gate on, the dead slot counts as active, and the reserve is charged twice | **Open, documented.** It clears when the replacement isolate is installed; it only arises during an isolate failure. *Fixed in RE-4.1 (`Slot::release_now`, [RE-4.1-ADMISSION-ACCOUNTING](RE-4.1-ADMISSION-ACCOUNTING.md) §1)* |
| 6 | Low | A failing memory probe was retried on every acquire and release, under the pool lock. `MemoryProbe` implementations must not call back into the executor | **Fixed**: failed reads are throttled to the sampling interval. **Documented** on the trait |
| 7 | Low | The CLI help said static files and prerendered pages are never gated, but they are when the build's proxy (middleware) runs for them | **Fixed** (help text; the design doc already covers it under §1) |
| 8 | Low | A proxy body without a known length reserves the full limit (10 MiB) of the native buffer budget | **Documented** ([RE-4-NATIVE-BUFFER-BUDGETS](RE-4-NATIVE-BUFFER-BUDGETS.md)); conservative by design |
| 9 | Low | Native budget accounting: a file growing during the read, the compression cache's copy outliving the permit, a re-reserve loop for a file whose size keeps changing (each iteration bounded by the wait) | **Documented**; accounting only, no unbounded growth |
| 10 | Low | `--native-buffer-wait NaN` panicked at startup; `--memory-admission-budget 0` was accepted (permanently critical); `--native-buffer-budget 0` silently meant off | **Fixed**: validated at startup (with or without an executor); CLI tests added |
| 11 | Low | The near-heap-limit callback logged at `warn` on every granted call, even with every feature off: the one behaviour change with defaults | **Changed**: one `warn` per isolate, further calls at `debug`, then the episode summary. A heap-limit termination was already logged as an error by the supervisor |
| 12 | Low | Timing-sensitive tests: a 100 ms window in `queued_requests_behind_a_running_one_are_refused_at_critical`; a 40 ms assumption in a unit test | **Fixed**: windows widened (500 ms / 2 s; seconds in the unit test) |

## Checked and found correct (reviewer)

- **Locks and re-entrancy:**
  - no `Slot` is dropped under the pool lock (send failures are disarmed);
  - `LeaveQueue::drop` only moves senders;
  - shutdown drops waiters outside the lock.
- **Queue bytes:** subtracted on every removal path; count and byte
  bounds checked before the push.
- **FIFO and progress:** every queued request ends (dispatch, refusal or
  `queue_timeout`). Refused requests never reach JavaScript. No request
  runs twice.
- **Gate off:** behaviour identical to before. Only the queue-byte
  bookkeeping and a harmless dispatch when the head waiter leaves remain.
- **Allocator** (checked against V8's source):
  - `free` receives the allocated length, and zero-length stores are never
    allocated;
  - resizable buffers use V8's page allocator, not this one;
  - the `Arc` is released exactly once (`~RustAllocator`);
  - no V8 calls from the allocator, and cross-thread `free` is safe;
  - `CheckIneffectiveMarkCompact` needs ≥ 80 % live, so it is never
    declined.
- **Native budget:** permits are released on 304, HEAD, 412, 416, early
  returns, blocking-task panics and cancellation. An oversize buffer is
  clamped to the budget, so there is no deadlock. Nothing changes without
  a budget.
- **Young-generation floor:** the total never increases.
- **Arithmetic:** saturating or clamped throughout.
- **Repeated runs:** admission (other tests) 6/6, `heap_limits` 5/5,
  `external_memory` 8/8.

## After the fixes

No critical or high finding is open. Open low findings: 5 (documented).
Residual risks without an end-to-end test:
- the nested-request bypass under real Server Action traffic;
- the proxy-body permit.
