# RE-4.1: independent review

A separate reviewer was given three things:

- the code diff (`git diff origin/main...HEAD -- crates/`, at `0b8aaa9`);
- the brief's requirements;
- an instruction not to rely on the RE-4.1 reports or commit messages.

It read the code paths around the diff and checked deno_web 0.291.0 and
tokio's semaphore against their sources. It ran the suites repeatedly
and built two probes outside the repository: a tokio head-of-line probe,
and a temporary test against the next-proxy build, deleted afterwards. It
modified no tracked file.

The cache fix (`3973254`) was reviewed earlier by the author, before it
was taken from the investigation's worktree, and again by this reviewer
(§2).

## 1. Findings and resolution

| # | Severity | Finding | Resolution |
|---|---|---|---|
| F1 | **High** (opt-in flag; also in RE-4; **reproduced**) | A proxy body of unknown length, or with a declared length close to the budget, reserved its maximum before the first byte. One client trickling a chunked POST therefore held the whole budget for as long as it stayed connected. tokio's semaphore is FIFO, so a waiter needing the whole budget at the head absorbs every released permit. Probe (4 MiB budget): `reserved 4194304 of 4194304`, static chunk 503 after 503 ms, public file 503 | **Fixed** (`NativeBufferBudget::grow`, `resolve.rs` `buffer_budgeted`). Bodies are reserved as they arrive, one frame's growth at a time (≤ 64 KiB), so no reservation waits for room it does not yet need. New end-to-end test `a_trickling_proxy_body_holds_only_what_it_sent`: ≤ 1 KiB reserved while trickling, static and public 200 without waiting, body intact. Unit test for growth, refusal without change, and release |
| F2 | Medium | The admission-requested reclamation (RE-4.1C) applied to every existing `--memory-admission` + `--memory-pressure-reclaim` setup with no new flag, and skipped the pressure backoff | **Fixed:** only with `--memory-admission-emergency` set. RE-4 configurations keep their reclaim schedule. Unit test |
| F3 | Low | Default `sampled` accounting is not byte-identical to RE-4: a retry is not charged twice, and a waiter gone at dispatch is taken back out of `reserved` | **Accepted as intentional fixes**, documented ([RE-4.1-ADMISSION-ACCOUNTING](RE-4.1-ADMISSION-ACCOUNTING.md) §1) |
| F4 | Low | A withdrawn admission still used up the emergency interval's one request | **Fixed:** the commitment records the token it replaced and `withdraw` restores it. Unit test |
| F5 | Low | A retry (its isolate died before taking the job) was gated again: refused in emergency because its own first admission had used the token, and refused at critical while others ran | **Fixed:** retries skip the memory gate as nested requests do. Concurrency limits still apply |
| F6 | Low–Medium (CPU, flag on) | Budgets under 16 MiB stream files between budget/4 and 4 MiB: no cached gzip, a sync flush per chunk | **Documented** ([RE-4.1-NATIVE-BUFFER-HARDENING](RE-4.1-NATIVE-BUFFER-HARDENING.md) §6). 32 MiB, the runbook value, is unaffected |
| F7 | Low (design) | Streams reserve only their first chunk before answering. Under sustained overload or many slow readers, a download can fail mid-body (after the 30 s chunk wait) rather than get a 503 up front | **Documented** (same §6) as a follow-up (per-connection cap). Strictly better than RE-4's whole-file holds |
| F8 | Low | With a declared length above the configured limit, the 413 named the budget, not the limit | **Fixed** with F1: the limit is no longer lowered |
| F9 | Low (CPU) | Commitments are scanned linearly under the pool lock | **Accepted:** bounded by active requests (hundreds). A cursor or index can come with measurements |
| Info | — | A comment said JavaScript's reads of a proxy body keep the reservation alive; they copy into V8 | **Fixed** (module docs, report) |

## 2. Checked and found correct (reviewer)

- **`units_for`'s `debug_assert`** cannot fire. Whole reads ≤
  whole-read limit ≤ budget; chunk ≤ budget for every budget ≥ 1 KiB;
  proxy bounds ≤ budget. Zero-length files reserve one unit.
- **Static files:**
  - Content-Length on full, range and stream responses;
  - HEAD and 304 read and reserve nothing;
  - 412 and 416; If-Range with gzip; open, suffix and EOF ranges;
    multi-range falls back to the full response;
  - truncation mid-stream fails the body, never sends other bytes; growth
    is capped at the declared length; a renamed-over file sends the
    opened handle;
  - path checks unchanged.
- **Permit lifetimes:**
  - `from_owner` keeps the permit with every clone: hyper's queue,
    `GzipBody`'s pending input, the compression cache's identity, released
    after compression even if cancelled;
  - no whole-file duplication;
  - `FileStream` releases everything when dropped mid-read; polling after
    an error ends the body; backpressure holds.
- **Admission lifecycle:**
  - commitments are created only under the pool lock and released
    exactly once (drop, `release_now`, or `disarm` + `withdraw`);
  - no slot that holds a pool reference is dropped under the lock;
    cancellation takes the locks in sequence, never nested;
  - with the gate off nothing changes;
  - `committed` and `emergency` are opt-in, and validation rejects bad
    values.
- **Nested marker:** a task-local set only in `InternalRoute`'s spawned
  tasks; no header reaches it; spawned tasks do not inherit it;
  recursion is bounded by 64 permits (fail-fast).
- **Reclaimer:** rate-limited by `pressure_interval`, shared with the
  normal path; the request flag is taken every tick, so it never fires
  late.
- **`performance.timeOrigin`:** origin = wall − (mono_before + width/2),
  against the same `deno_web::StartTime` that `op_now` uses, so
  `timeOrigin + now()` stays consistent with the wall clock. The patched
  text matches the vendored file, with a warning fallback. No V8
  snapshot freezes it.
- **Send / blocking:** `FileStream` is `Send`; file I/O runs on the
  blocking pool.

## 3. Runs (reviewer)

- **Suites:** `native_budget` 11, `range` 11, `compression` 13,
  `preconditions` 4, `http` 14, `--lib native_budget` 9, `--lib reclaim`
  10, runtime `--lib admission` 24, `--test admission` 15, `ops::tests` +
  `web_patches` 5, `resource_e2e --ignored` 7/7.
- **Repetition:** 10 runs each of the native-budget, range and
  compression suites and of runtime admission, with 0 failures.
- **Clippy:** `+stable` clean.

## After the fixes

No Critical or High finding is open. Open, documented, Low: F6, F7, F9.
The pre-existing slot-exhaustion stall of nested requests is in
[RE-4.1-NESTED-REQUEST-COMPATIBILITY](RE-4.1-NESTED-REQUEST-COMPATIBILITY.md)
§3 (Medium, bounded by the queue timeout, owner decision).
