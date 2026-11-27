# RE-2/3: independent review

Scope: every RE-2/3 code change: what merged in PR #50 (`65397c6`:
isolate-thread reclamation, the `drive` select loop, `heap_memory_bytes`,
the server reclaimer, profiles) and this branch (cgroup resolution, heap
budget, heap capping, reclaimer idle/pressure changes, CLI). The reviewer
was a separate agent given the diffs and the checklist below, not the
author's conclusions; it read the code and the pinned V8 (`v8/src/heap`,
`v8/src/api`) and deno_core 0.413 sources and ran nothing. Findings were
then verified by the author against the V8 source before fixing.

Checklist: unsafe Rust and FFI; the select-based drive loop
(cancel safety, starvation, re-entrancy); Notify lost wake-ups; request
lifecycle; cgroup detection; memory accounting and budget arithmetic; V8
semantics of the constraints used; the near-limit headroom; resource
exhaustion; platform portability; whether tests test what they claim.

**Critical: none. High: none.**

## Findings and dispositions

| # | Severity | Finding | Verified | Disposition |
|---|---|---|---|---|
| M1 | medium | The heap-proportional near-limit grant (¼ of the heap, 16–64 MiB) introduced on this branch could **abort the process** where the old flat 64 MiB did not: on V8's last-resort path the callback runs once and the allocation is retried once (`heap-allocator.cc` `RetryCustomAllocate` → `FatalProcessOutOfMemory("CALL_AND_RETRY_LAST")`), so one object larger than the grant aborts every isolate | yes, in `heap-allocator.cc:497-516`, `heap.cc:1318` | **Fixed:** reverted to the flat 64 MiB; new test `a_large_allocation_at_the_heap_limit_replaces_the_isolate_instead_of_aborting` (24 MiB single allocations at a 64 MiB limit) |
| M2 | medium | Idle detection counted `completed + in_flight`, which double-counts a response still streaming; and the 0.2 req/s quiet allowance equals Kubernetes' default liveness + readiness rate, so jitter could re-arm and repeat collections | yes (`http_metrics.rs`: `requests` counted at head, `in_flight` decremented at body end) | **Fixed:** an `arrivals` counter incremented on arrival; quiet allowance 0.3 req/s (≥ 3 per window); re-armed only above twice the allowance; test with jittery liveness + readiness probes |
| L1 | low | Pressure backoff doubled on the first reclamation of a new episode (`last_pressure` never cleared), and the test asserted the wrong interval | yes | **Fixed:** `pressure_unhelpful` flag, cleared below the threshold; test now expects the base interval |
| L2 | low | A cgroup path climbing out of the namespace (`/../x`) fell back to the mount point, i.e. another cgroup's limit | yes | **Fixed:** such paths resolve to no cgroup; test updated |
| L3 | low | The `auto` log printed the budget, which V8 may lower (its own default) or `--v8-flags` may override | yes (`heap.cc:4872,4981` apply flags after constraints) | **Fixed:** each isolate logs `V8 heap limit in effect`; warning when heap-size V8 flags are combined with `--js-heap-limit` |
| L4 | low | V8 rounds the semi-space up to a power of two, so a capped young generation could overshoot the cap by a few MiB | yes (`heap.cc:4896-4901`) | **Fixed:** the cap path rounds the semi-space down to a power of two first (the budget's own cases were already powers of two) |
| L5 | low | A long low-memory collection counts toward the stall watchdog | yes | **Fixed:** heartbeat before and after the collection; a single collection over `--js-stall-timeout` CPU seconds (10 s) would still be reported as a stall (not seen: 0.2–0.4 s at 50–120 MiB heaps) |
| L6 | low | Tests weaker than claimed: module-state check skipped when the second request hit the other isolate; `full_gcs > 0` true from bootstrap | yes | **Fixed:** new one-isolate `reclamation_keeps_module_state`; full-GC counts compared before/after per isolate |
| L7 | low | The heap budget is computed once at startup (no in-place resize) | yes | **Documented** (configuration.md, budgeting report) |

## Checked and found sound (reviewer, confirmed)

- FFI: `mallopt`, `malloc_trim`, `mallinfo2` behind `cfg(all(target_os = "linux", target_env = "gnu"))` with no-op fallbacks; `sysconf` errors map to `None`; `c_int::try_from` guards `mallopt`. Pre-existing note: `mallinfo2` needs glibc ≥ 2.33 (the release builds on Debian 12, glibc 2.36).
- GC epilogue data pointer: the `Arc::into_raw` reference outlives the callback registration; on unwind it leaks rather than dangles.
- `drive` loop: dropping and re-polling the event-loop future is safe (state lives in `JsRuntime`); a reclaim request is taken in the poll that returns it, so it cannot be lost; `biased` polling cannot starve either branch; reclamation never runs inside V8 (only between `poll_event_loop` calls); the isolate is entered, so repeated critical-pressure notifications are handled synchronously.
- `Notify`: `notified()` is created before the check and `notify_one` stores a permit: no lost wake-ups.
- V8 constraints: `ConfigureHeap` takes the old-generation size set after `ConfigureDefaults`, clamped to V8's minimum: "cap, never raise" holds.
- cgroup walk: only normal path components are accepted, so the ancestor walk terminates; unreadable files count as no limit; hybrid hosts fall through to v1; mountinfo fields and octal escapes are parsed correctly.
- Arithmetic saturates; `u64`↔`usize` conversions use `try_from` with saturating defaults; nothing breaks on 32-bit targets.
- Resources: the idle window holds at most one entry per second of `idle_after` (≤ 86 400, enforced by the CLI).

## Not covered by the review

Benchmark validity is assessed in [RE-23-COMBINED-BENCHMARKS](RE-23-COMBINED-BENCHMARKS.md)
and [RE-23-MILESTONE-GATE](RE-23-MILESTONE-GATE.md). Next.js compatibility
is covered by the compatibility suites run in CI on the PR. The fixes were
made after the matrix binary (`re2b`) was built. The final binary,
`re2c` (sha256 `052d741d4fc4659201da20679a9505e79e100f05dc51e9b5d1cdd25017191902`),
includes every fix above and was re-measured on the decisive
configurations ([RE-23-COMBINED-BENCHMARKS](RE-23-COMBINED-BENCHMARKS.md#final-binary-re2c-confirmation));
the only change to the code after it was built is `cargo fmt`.
