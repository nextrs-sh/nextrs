# RE-5: independent review

An adversarial read of every RE-5 change (`8d83d03`..HEAD plus the then
uncommitted heartbeat, cache and header changes), by a separate reviewer
agent. The reviewer was given the code and the risk areas, not the
author's conclusions. It only read code: no builds, tests or benchmarks
while measurements ran.

**Areas:**

- unsafe code, FFI and ownership;
- resource accounting;
- concurrency and memory ordering;
- watchdog safety;
- security;
- HTTP compatibility;
- benchmark methodology.

**Result:**

- **No critical or high findings.**
- Seven of the 13 findings were fixed with tests (§1).
- The other five are documented below (§2).

## 1. Fixed

| # | Severity | Finding | Fix |
|---|---|---|---|
| F1 | Medium | The nested-request overdraft was bounded only over the pool (64). One isolate could run `max_active + 64` concurrent renders, none gated by memory, and the near-heap-limit skip was not applied. | Overdraft is now also bounded **per isolate** at `max_active_per_isolate` (an isolate runs at most twice its configured concurrency). With admission, isolates near their heap limit are taken last. The recursion test became `overdraft_is_bounded_per_isolate_and_recursion_ends`. Consequence: nesting deeper or wider than one isolate's allowance queues as before RE-5, bounded by `queue_timeout`. Next's own nesting (a redirect target, a forwarded action, `res.revalidate`) is one level deep. |
| F3 | Medium | The prerender cache's entries were not in the native buffer budget, and an evicted body still being sent to a slow client was counted nowhere. 1 024 slow clients on distinct 1 MiB pages could hold about 1 GiB beyond both limits. | With a budget, **every body the store hands out is reserved**, kept or not. A kept entry that finds no room at once is served, reserved after the wait, but not kept. Test `with_a_budget_kept_entries_are_reserved_until_evicted`. |
| F5 | Low | Change detection compared only length and mtime. `cp -p`, `rsync -a` and image layers keep both. | Inode and ctime are compared too. Test `a_replacement_with_the_same_length_and_mtime_is_refused`. |
| F6 | Low | A body larger than the whole native budget tripped a `debug_assert` (debug builds), and in release it reserved the whole budget. | Such a body is read without a reservation: a documented exception. Test `a_body_larger_than_the_whole_budget_is_served_unreserved`. |
| F10 | Info | `dispatch_nested_waiters` spent overdraft even when an ordinary slot was free. | It tries an ordinary slot first. |
| F11 | Info | The multipart comment overstated the saving: multer still buffers its input. | Comment and RE-5C corrected: one of two body copies is removed. |
| F12, F13 | Medium / Low (method) | `prerender-scale.mjs`: Node's fetch negotiated gzip (the compression cache was in "sustained"); the page cache was warm from the previous mode; the readiness probe warmed the first sampled route. `idle-cpu-process.mjs`: threads that exit inside the window were dropped. | Identity requests; page cache dropped (`posix_fadvise`) before each run; readiness on `/`; process-level CPU recorded beside the per-thread sums. The reported prerender latencies are from the corrected quiet-host rerun (RE-5B §4.1). The first idle comparison ran with the earlier script; see RE-5F §3. |

## 2. Documented, not changed

| # | Severity | Finding | Why not changed |
|---|---|---|---|
| F2 | Medium | The internal-request permit belongs to the fetch task, not the slot. A nested render whose fetch was abandoned keeps its overdraft slot until it ends (`request_timeout`) after its permit is released. Abandoned renders can therefore occupy the allowance. | Bounded by the per-isolate cap (F1) and by `request_timeout`. In the worst case the RE-4.1 stall returns for the duration, which is no worse than before RE-5. Moving the permit into the `Job` is a follow-up; documented on `NESTED_OVERDRAFT`. |
| F4 | Low | A slow read holds its stripe lock (64 stripes): other cold entries on that stripe wait for it. | Reads are page-cache reads of build files. A per-id single-flight map is the follow-up if a slow disk shows it. |
| F7 | Low (performance) | A gzip response for a file-backed prerender reads the identity body before the compression layer finds its cached gzip copy. | Opt-in mode. The read is a cache hit or a page-cache read. Checking the compression cache first means restructuring `compress::apply`; it is a follow-up. |
| F8 | Low | A changed build file: a conditional request or HEAD whose ETag is known is still answered (304/200), a plain GET gets 500, and `/_not-found` a 500 instead of a 404. An in-place redeploy turns file-backed pages into 500s while the old process drains. | By design: never serve bytes the process did not start with. **Deploy new builds to a new directory** (as containers and atomic symlink swaps do). Stated in RE-5B §6 and the flag's help. |
| F9 | Low | CPU spent in Tokio tasks on the isolate thread outside the event loop, without the thread parking, no longer gets beats between tasks. | Such work would need more than `stall_timeout` (10 s) without a single event-loop turn or park. No NextRS task does that. The failure mode is a false termination, not a missed stall. Tokio's per-task poll hooks would close it but are unstable. |

## 3. Checked, nothing found

The reviewer checked these and found nothing:

- **Multipart FFI:** `V8Slice` holds a `SharedRef<BackingStore>` and is
  `Send`. The op is synchronous, so JavaScript cannot detach or resize
  the buffer. The `Bytes` never leave the op.
- **Park/unpark ordering:** the hooks run only around a real park, and
  JavaScript cannot run between them. The beat comes before the
  `Release` store and is read after the `Acquire` load.
- **Watchdog coverage:** endless loops, microtask chains, long GC or
  reclaim and blocked turns are detected as before. `stop`, `reclaim`
  and the inbox never relied on the timer.
- **Slot accounting:** the overdraft flag travels through every release
  path (drop, `release_now`, `undo`, a waiter that is gone, isolate death,
  retry). The counter cannot underflow.
- **Nested marker:** a task-local set only by the internal route; it
  cannot be spoofed.
- **Prerender paths:** from manifests only.
- **`etag.rs` rewrite:** equivalent.
- **Content-Length:** the validated length.
- **HEAD:** no body.
- **CLI:** `drop(info)` and `malloc_trim` are fine.
