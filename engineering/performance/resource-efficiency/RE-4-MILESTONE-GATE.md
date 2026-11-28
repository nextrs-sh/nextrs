# RE-4: milestone gate

Resource-aware admission control and memory exhaustion resilience.
Branch `feat/re-4-resource-admission`, on `main` `0d2a742` (RE-23B
reports merged as PR #53). Issue #52's test-only fix is in its own PR,
#54.

Implementation readiness and release readiness are judged separately.
The Linux benchmark host is still blocked
([RE-23B-LINUX-ENVIRONMENT](RE-23B-LINUX-ENVIRONMENT.md)). No container
benchmark, fault test or soak ran, and none is reported.

## Verdicts

| # | Area | Verdict | Basis |
|---|---|---|---|
| 1 | Implementation readiness | **PASS WITH CONDITIONS** | Admission, ArrayBuffer budget, native buffer budget, heap-limit instrumentation and young-generation floor are implemented, opt-in, and tested; the independent review's high finding (an ArrayBuffer refusal replacing the isolate when the heap was over half full) and its medium findings are fixed and covered by tests. Conditions: owner review of the PR; the proxy-body permit has no end-to-end test (no local build exercises it) |
| 2 | Compatibility and correctness | **PASS WITH CONDITIONS** | 513 local tests pass (0 failed; one intermittent pre-existing data-cache test failure seen once, not reproduced in 80 reruns); clippy clean on 1.98 and 1.100; the 22 fixture-dependent server tests pass; the release binary serves the fixture app with every control off and on (results/re4-local). Condition: CI's compatibility suites on the PR |
| 3 | Resource safety | **PASS WITH CONDITIONS** | Native buffers that NextRS now budgets stay within `max(budget, largest buffer)` with RAII release on every path; ArrayBuffer allocations ≥ 64 KiB are refused at the limit as RangeErrors; queues are bounded in count and bytes; no leaked reservation in any test. Conditions: everything is process-level; admission is best effort; fatal V8 aborts and OOM kills remain possible |
| 4 | Benchmark validation | **BLOCKED** | Native Docker Engine not installed; the tools cannot run host commands; runbook written for the owner |
| 5 | Production release readiness | **BLOCKED** | Requires 4, the staged soaks, and the RE-23B conditions (soaks, Linux CPU/p99) still open. No default changes, no version bump, nothing published |

## Acceptance criteria (implementation)

| Criterion | Status |
|---|---|
| No new compatibility regression | local: none; CI suites pending on the PR |
| All existing CI gates pass | local equivalents pass; CI pending |
| No unbounded admission queues | count (`--max-queued`) and, with admission, bytes (16 MiB) |
| No leaked resource reservations | tested: queue bytes 0 after cancellation and refusal; native permits 0 after drop (64 concurrent); ArrayBuffer budget released with replaced isolates; admission reservations expire with each reading |
| Controlled native buffers within their aggregate budgets | static files and proxy bodies: tested; never above the budget in the 64-request test |
| Resource-related overload produces documented responses | 503 `executor-memory-pressure` (Retry-After 2), 503 `native-buffer-budget` (Retry-After 1), RangeError for ArrayBuffers; documented in `configuration.md` |
| No automatic duplicate execution of Server Actions | tested; refusals happen before delivery, and NextRS never re-dispatches a delivered request |
| No new uncontrolled restarts or deadlocks | an ArrayBuffer refusal no longer replaces the isolate unless the heap is ≥ 90 % committed; nested self-requests bypass the gate (no parent/child stall); FIFO dispatch with a progress rule; shutdown with deferred waiters completes |
| Existing opt-in profiles remain compatible | unchanged; their tests pass |
| No unresolved critical or high findings | met: the one high finding is fixed ([RE-4-INDEPENDENT-REVIEW](RE-4-INDEPENDENT-REVIEW.md)); one low finding open, documented |

## The eight questions

**1. Which memory allocations can NextRS enforce?** Those it can refuse
before they happen:
- static and `public/` file bodies and proxy-buffered request bodies
  (`--native-buffer-budget`);
- ArrayBuffer backing stores of 64 KiB or more that JavaScript allocates
  through V8's allocator (`--js-external-memory-limit`);
- image sources and transformations (existing budgets);
- per-request limits: request body, response buffer, fetch response;
- cache capacities (compression, data/ISR, image).

**2. Which remain outside its control?**
- V8's heap beyond its limit: a single allocation overshoots, and very
  large ones abort the process.
- V8-internal native memory (compiler, code space).
- Buffers handed to JavaScript from Rust ops (`fs.readFileSync`,
  `TextEncoder`, `Response`/`Blob` bodies, fetch and request chunks:
  bounded per request at most).
- ArrayBuffers below 64 KiB.
- Response chunks copied out of V8.
- glibc retention between trims, thread stacks, file-backed pages.
- Other processes in a shared cgroup.

**3. Does admission control materially reduce resource exhaustion
risk?** **Not yet known.** The mechanisms work at process level: it
caps concurrency under pressure, refuses at the critical level, keeps
requests away from isolates near their heap limit, and bounds queue
memory. Whether that prevents OOM kills at 256 MiB depends on how much
an admitted request allocates beyond its estimate. That is the
container fault tests' question. It cannot prevent a single request's
large allocation.

**4. Can fatal V8 failures still terminate the process?** **Yes.** One
allocation needing more than about three 64 MiB grants above the limit
aborts the process (192 MiB at a 64 MiB limit, 256 MiB at 128 MiB;
reproduced in child processes). The overshoot before recovery comes from
V8 calling the near-heap-limit callback repeatedly, each call granting
another 64 MiB ([RE-4-V8-HEAP-OVERSHOOT](RE-4-V8-HEAP-OVERSHOOT.md)).

**5. Would worker-process isolation justify its memory cost?** **The
memory cost is small; the case is not made yet.** The estimates, from
process-level measurements, are +3 MiB anonymous for one worker and
+10 MiB per extra worker, if caches are not duplicated. The benefit at
one isolate is narrow, though: static serving survives a V8 abort,
while in-flight JavaScript requests and OOM kills do not. The IPC and
cache redesign are large. Revisit after
the fault tests, first for multi-isolate deployments of 1 GiB or more
([RE-4-FAULT-CONTAINMENT-ADR](RE-4-FAULT-CONTAINMENT-ADR.md)).

**6. Can young-generation tuning recover CPU efficiency?** **Probably,
at 512 MiB and above, but this is unmeasured on dedicated hardware.**
The Mac VM runs suggest 8 MiB recovers about two thirds of the gap and
16 MiB all of it. A supported, cap-preserving floor now exists
(`--js-young-semi-space`). No size is chosen
([RE-4-YOUNG-GENERATION-EXPERIMENT](RE-4-YOUNG-GENERATION-EXPERIMENT.md)).

**7. What remains blocked by the benchmark environment?**
- Every container measurement: the decisive campaign (5 variants × 3
  configurations × 3 rounds), the container fault tests, CPU/GC/p99 on
  Linux, and the 30 min / 2 h / 6 h soaks.
- The RE-23B conditions: soaks, Linux CPU and p99.

Deferred to RE-8: the full configuration matrix and the 24-hour soak.

**8. Is another JavaScript engine justified by the new evidence?**
**No.** The new findings are about the embedder:
- repeated heap-limit grants;
- an ArrayBuffer refusal mistaken for a full heap;
- native buffers without an aggregate bound;
- admission without memory awareness.

The fixes all go through supported V8 interfaces. Any engine would need
the same embedder policies. V8 stays the default.

## Owner decisions requested

1. Review and merge PR #54 (issue #52, test-only).
2. Review the RE-4 PR; it is not to be merged without approval.
3. Install the native Docker Engine and run the runbook (§1–§7) from a
   host terminal, or authorize host command access for the tools.
4. Keep all RE-4 controls opt-in until the campaign reports. No default
   or profile change is proposed.
