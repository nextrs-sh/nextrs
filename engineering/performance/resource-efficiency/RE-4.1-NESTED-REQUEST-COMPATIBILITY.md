# RE-4.1D: nested requests and proxy bodies, end to end

RE-4's two residual risks without an end-to-end test:

- nested self-requests under memory pressure;
- the proxy-body budget.

Both are now tested on real Next.js builds over a real listener, with the
resource controls on: `crates/nextrs-server/tests/resource_e2e.rs`,
commit `0b8aaa9`. The tests are ignored and fixture-dependent, like the
other server tests that need a build.

```
(cd fixtures/next-app && npm ci && npx next build)
(cd fixtures/next-proxy && npm ci && npx next build)
cargo +1.98.0 test -p nextrs-server --test resource_e2e -- --ignored --test-threads=1
```

**Local result:** 7 passed, 0 failed. Fedora 45, Node.js 24.18.0 for the
fixture builds (CI pins 22.20.0), Rust 1.98.0. Memory states are set with
a scripted probe, so the run is process-level.

## 1. The nested path

A client-side Server Action that calls `redirect()` gets Next's
`x-action-redirect` and, in the same response, the target's RSC payload,
which Next fetches from its own origin. NextRS answers that fetch
in-process (`InternalRoute::start`, `internal.rs`) inside
`admission::nested`. The child bypasses the memory gate and the heap
skip, but **still needs an isolate slot**, unless the target is
prerendered, which NextRS serves natively.

Each test asserts that the listener saw exactly one request per action,
so the nested fetch was dispatched in-process, not over the network.

## 2. Results

| Test | Setup | Result |
|---|---|---|
| `a_redirecting_server_action_completes_its_nested_request_at_critical` | admission at critical (99 %) | Redirects to a prerendered page and to a dynamic page (`/books/7`): both 200 with `x-action-redirect` and the target's payload. 1 listener request each. Parents admitted by the progress rule, 0 refusals, 0 commitments after |
| `emergency_admits_one_action_per_interval_and_never_runs_a_refused_one` | emergency (99.5 % of a 98 % threshold), 30 s interval | 1st action completes; 2nd: 503 `executor-memory-pressure`, no `x-action-redirect` (never delivered); memory falls → 3rd completes |
| `concurrent_redirecting_actions_with_concurrency_exhausted` | 1 isolate, 2 slots, queue timeout 3 s; 8 concurrent actions, dynamic targets | all end in 3.06 s: 2 × 200 with redirect, 6 × 503 queue timeout (never delivered); 0 commitments after; the server serves the next action |
| `a_nested_request_waits_for_a_slot_at_most_the_queue_timeout` | 1 isolate, 1 slot, queue timeout 2 s | prerendered target: immediate. Dynamic target: **200 with `x-action-redirect` and an empty payload after 2.0 s** (§3) |
| `a_cancelled_action_and_a_shutdown_leave_no_commitment` | critical | 4 clients hang up after sending; commitments return to 0; next action completes. Graceful shutdown with an action in flight finishes; commitments 0 |
| `an_external_request_cannot_claim_to_be_nested` | critical, one action running | a direct request carrying `x-middleware-subrequest`, `x-nextrs-nested` and `x-nextjs-internal` is refused (503) |
| `the_proxy_reads_budgeted_request_bodies_intact` | next-proxy, 256 KiB budget | §4 |

**Revalidation:** `revalidatePath`/`revalidateTag` from actions is
covered by the compatibility suites in CI with every control off. It is
not repeated under pressure here: the fixture's revalidation page needs
the test upstream. On-demand revalidation (`InternalRoute::revalidate`)
runs in the same nested scope as the redirect fetch.

## 3. Finding: nested requests and slot exhaustion (pre-existing, Medium)

A nested request is not memory-gated, but it queues for an isolate slot
like any request, FIFO behind other waiting parents. When every slot is
held by parents that are waiting for their children, the children wait
until `queue_timeout` (10 s by default). The child then fails, and Next
answers the action with the redirect but without the payload, so the
client fetches the target itself.

- **Bounded, not a deadlock:** measured at 2.0 s with a 2 s timeout.
- **Not memory-related.** It needs `max_active_per_isolate` (default
  128) to be exhausted by such parents, and it exists without admission.
- **Not fixed here.** The acceptance criteria rule out behaviour changes
  with the controls off.
- **Proposed fix (owner decision):** let nested requests take a slot
  above `max_active_per_isolate`, bounded by `MAX_INTERNAL_REQUESTS`
  (64), and dispatch them ahead of external waiters. Memory admission
  keeps counting them.

## 4. Proxy-buffered bodies (next-proxy)

`/proxy/body`: the proxy reads `request.text()` and answers with it.
`/proxy/body-pass`: the proxy reads it, then rewrites to a route handler
that reads the same body again. Budget 256 KiB.

| Case | Result |
|---|---|
| 0 B, 1 B, 1000 B, 64 KiB, 200 KiB, both paths | byte-identical in the proxy's answer and in the route's copy; `x-proxy-body-length` correct; reserved 0 after each |
| 300 KiB (> budget) | 413 `native-buffer-budget-exceeded` |
| 16 concurrent 60 KiB bodies through both readers | each 200 and intact, or 503 after the wait. None corrupted |
| client sends 50 of 100 KB, then hangs up | reservation released |
| end | `reserved_bytes` 0; `reserved_bytes_total{kind="proxy_body"}` > 0 |

## 5. Bounds audit

| Bound | Where | Holds |
|---|---|---|
| In-flight internal requests | `MAX_INTERNAL_REQUESTS` = 64 permits, `try_acquire` (fails fast, no queue) | yes: nested fan-out and recursion depth × width ≤ 64 at any time |
| Nested request slots | `max_active_per_isolate`, queue, `queue_timeout` | yes, with the §3 stall |
| Memory | nested requests are recorded as commitments (counted under `committed` accounting), never gated | by design |
| Marker | task-local `NESTED`, set only by `admission::nested` in `internal.rs`, which runs only in-process dispatch tasks; not inherited by spawned tasks (unit test); `SelfDispatched` is a request extension only in-process code inserts | not reachable from headers (test above) |
| Retries | delivered Server Actions are never retried: refusals happen before delivery; the dispatch retry only covers jobs no isolate took | unchanged, tested in RE-4 |

**Verdict for 4.1D:** nested Server Actions complete under critical and
emergency memory states, cannot be spoofed, and leave no commitment.
Proxy body budgeting holds byte for byte, end to end. One pre-existing,
bounded concurrency stall is documented with a proposed fix.
