# RE-4.1: milestone gate

Resource control correctness and memory safety hardening.

- Branch `fix/re-4.1-resource-controls`, on `main` `68927b4` (RE-4 merged
  as PR #56; CI on the PR and on the merge: every job passed, including
  the pinned and 16.4.5 compatibility suites).
- PR #54 (issue #52, test-only) was reviewed: merged as `c805f52`. It
  only removes the stall limit from the two bulk-body tests, keeping
  their streaming-integrity assertions, and the dedicated watchdog tests
  are untouched.

Every control stays opt-in. No default, profile, version, package or
deployment changed. **No container benchmark, fault test or soak ran.**
Nothing below is a container result.

## Verdicts

| # | Area | Verdict | Basis |
|---|---|---|---|
| 1 | Implementation readiness | **PASS WITH CONDITIONS** | 4.1A–E, G and H implemented and tested. The independent review's one High finding (F1, reproduced, also in RE-4) and four lower ones fixed with tests; the rest documented. Conditions: owner review of the PR; CI on the PR (not yet run) |
| 2 | Correctness and compatibility | **PASS WITH CONDITIONS** | Local: workspace **545 passed, 0 failed**, 35 ignored, 45 suites (1.98.0, after the review fixes); clippy clean on 1.98.0 and 1.100.0; fmt clean; 30 fixture-dependent server tests pass, including 8 new end-to-end; budget-on vs budget-off parity on 14 request variants. One pre-existing cross-isolate defect found and fixed (`performance.timeOrigin`). Conditions: CI's compatibility suites on the PR; the nested-request slot stall (pre-existing, Medium, bounded) awaits a decision |
| 3 | Resource-control effectiveness | **PASS WITH CONDITIONS** | Native buffer budget: now a strict bound on its buffers. Admission: lifecycle exact, duplicate reservation fixed, delayed-allocation weakness of the default quantified, alternative offered. External memory: coverage measured and corrected. All at process level. Conditions: container measurements; the default accounting's measured weakness stays until they choose |
| 4 | Benchmark readiness | **PASS WITH CONDITIONS** (harness) / **BLOCKED** (execution) | Runbook corrected, gate, fault drivers, dry runs and preflight implemented and statically validated ([RE-4.1-BENCHMARK-READINESS](RE-4.1-BENCHMARK-READINESS.md)). Nothing ran against Docker: the host still has Docker Desktop, not the native engine |
| 5 | Production release readiness | **BLOCKED** | Needs 4 executed, the soaks, and the RE-23B conditions. No release work done |

## Acceptance criteria

| Criterion | Status |
|---|---|
| No unresolved critical or high findings | met ([RE-4.1-INDEPENDENT-REVIEW](RE-4.1-INDEPENDENT-REVIEW.md)) |
| All existing CI gates pass | local equivalents pass; **PR CI pending** |
| No new Next.js compatibility regressions | local: none; parity tests; CI suites pending |
| Oversized static files not buffered whole with the control on | met: streamed in budgeted chunks |
| No leaked or duplicated admission reservations | met: commitments tested on completion, cancellation, timeout, watchdog replacement, queue timeout, shutdown; retry fix |
| Delayed-allocation behaviour understood and tested | met: model table and real-isolate test; `sampled` under-counts, `committed` offered |
| Nested Server Actions tested under memory pressure | met: critical, emergency, exhaustion, cancellation, shutdown, spoofing |
| Proxy-body budgeting tested end to end | met: byte integrity, 413, concurrency, disconnect, trickle |
| External-memory limitations accurately documented | met: measured with < 64 KiB headroom, asserted; RE-4 corrected |
| Cache invalidation failure investigated | met: root cause found and fixed ([RE-4.1-CACHE-CORRECTNESS](RE-4.1-CACHE-CORRECTNESS.md)) |
| Linux runbook corrected and reproducible | met statically; not executed |
| No default configuration changes | met (the timeOrigin accuracy fix is the one change with every flag off) |
| No release or publication | met |

## The seven questions

**1. Are oversized static files safely streamed?** **Yes, with
`--native-buffer-budget`, at process level.**

- Above a quarter of the budget (at most 4 MiB), files and ranges stream
  in chunks of at most 64 KiB, each reserved before it is read.
- The bytes own their reservation, so hyper's queue, gzip and cache fills
  stay counted.
- Headers and bodies are identical to the budget-off server across 14
  request variants.
- Tested: slow clients, disconnects, concurrency, truncation and
  replacement during reads.
- Without the flag, files are read whole as before.

**2. Is admission reservation accounting correct across asynchronous
request lifecycles?**

- **The lifecycle is.** Every commitment ends exactly once on every path,
  and the duplicate charge during isolate replacement is fixed.
- **The default accounting is not complete.** `sampled`, the RE-4 policy,
  forgets an estimate at the next reading, 50 ms later. Requests that
  allocate after waiting are not counted. In the model, 200 SSR requests
  allocating 300 ms after admission take a 256 MiB container to 390 MiB.
- **The alternative is available, opt-in:** `--memory-admission-accounting
  committed` keeps estimates until the request ends, less measured
  growth, at most 2 s. It held the peak at the critical threshold in
  every case where estimates lived. Its cost is deferral.
- The default stays until the container campaign compares the two (an
  optional variant pair and its gate file are ready).

**3. Can nested requests safely complete under pressure?** **Yes.**

- Server Actions whose redirect target Next fetches in-process complete
  at critical and in emergency, cannot be spoofed by headers, and leave
  no commitment. Recursion is bounded at 64.
- One pre-existing, bounded limitation: a nested request needs an
  isolate slot. When parents hold every slot, it waits out the queue
  timeout and the action answers without the target's payload. This
  needs `max_active_per_isolate` (128) to be exhausted; a fix is
  proposed.

**4. Which external-memory allocation paths remain unenforced?**

- `TextEncoder.encode`, `Buffer.from(string)` (and base64/hex decoding);
- resizable ArrayBuffer growth;
- allocations under 64 KiB (counted, never refused);
- the transient Rust-made buffers behind the polyfills' copies;
- request and fetch chunks (bounded per chunk);
- V8-internal memory.

Measured as **enforced**, contrary to RE-4: `fs.readFileSync`, one-shot
zlib, and `Response`/`Blob` `.arrayBuffer()`.

**5. Is the shared-cache invalidation issue resolved or tracked?**
**Resolved.**

- **The defect:** a genuine cross-isolate coherency defect outside the
  cache. deno_web computes `performance.timeOrigin` from a wall-clock
  read followed by a monotonic read. Under boot contention that left
  about 1 isolate in 280–550 lagging the wall clock by up to 23 ms. Next's
  `areTagsExpired` then saw a fresh revalidation as not yet expired.
- **The fix:** a bracketed clock read (`3973254`).
- **Evidence:** a stress reproduction that failed 9 of 10 runs passes 10
  of 10.
- **A second, unrelated flaky test** in the same file (a read landing on
  the writer's isolate before the second was ready) is fixed in the
  test.
- **Tracking issue to open:** a wall-clock step after boot delays
  revalidation on that isolate. `next start` has the same exposure.

**6. Is the dedicated Linux harness ready to run?** **Ready to start,
not proven.**

- Every statically checkable item is fixed.
- **Fixed:**
  - absolute `OUT`;
  - no bare placeholders;
  - resolved mounts;
  - one Next.js build for both images, hash-verified;
  - young8 excluded at 256 MiB in data;
  - Node 24.18.0 pinned;
  - variants validated against each binary's flags;
  - continuous vs load-idle named;
  - raw series kept;
  - a correctness gate that `profile` requires;
  - eight fault drivers;
  - dry runs;
  - a read-only preflight.
- **Not run:** none of it ran against Docker. The host needs the native
  engine and the owner's host terminal. The first real run (preflight →
  gate) will be its validation.

**7. Are there any remaining engineering blockers before RE-5?**

- **No code blockers.** The open items are decisions and measurements:
  1. Container campaign: blocked on the host, and needed for every
     threshold and for the sampled-vs-committed choice.
  2. Nested-request slot exhaustion: owner decision on the proposed fix.
  3. Owner review and merge of the RE-4.1 PR after CI.
- **RE-5 candidates, not blockers:**
  - enforcement for `TextEncoder` / `Buffer.from(string)` (an
    allocator-backed encode);
  - a per-connection cap on streamed static files;
  - coordinated resource profiles (RE-4.1-RESOURCE-BUDGET-COORDINATION
    §4).

## Owner decisions requested

1. Review the RE-4.1 PR; it is not to be merged without approval.
2. Nested requests under slot exhaustion: allow them above
   `max_active_per_isolate` (bounded by 64), or keep the queue-timeout
   behaviour.
3. Open the tracking issue for the wall-clock-step exposure (text in
   RE-4.1-CACHE-CORRECTNESS).
4. Benchmark campaign: install the native Docker Engine and run the
   corrected runbook from a host terminal. Choose `RE4_COMMIT` (this
   branch's merge), the gate configurations, the number of fault rounds,
   and whether to run the optional sampled-vs-committed pair. The
   estimate is about 45 h of exclusive host time.
5. Keep every RE-4/RE-4.1 control opt-in until the campaign reports.

RE-5 is not started.
