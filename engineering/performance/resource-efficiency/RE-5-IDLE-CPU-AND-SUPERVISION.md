# RE-5F: idle CPU and event-driven supervision

This report covers why an idle NextRS process wakes up, what each wakeup
is for, and the change that removes the largest source (the isolate
heartbeat timer) without changing what the watchdog detects. It gives
process-level measurements at 0, 1, 2 and 4 isolates, idle, with
health-probe traffic only, and with low continuous traffic.

## 1. Periodic wakeups (baseline)

| Source | Period | Thread | Purpose | Needed when idle? |
|---|---|---|---|---|
| Isolate heartbeat (`worker.rs`, `tokio::time::interval`) | 25 ms (40/s per isolate) | the isolate thread | stores the time and the thread's CPU time; the supervisor measures a turn as "time since the last beat" | **no**: a parked thread runs no JavaScript and cannot be stalled |
| Supervisor (`pool.rs` `watch`, `recv_timeout(WATCH_INTERVAL)`) | 50 ms (20/s per isolate) | `nextrs-supervisor-N` | detects an exit; repeats a requested termination; judges stalls (CPU over `stall_timeout`, or wall over 6 ×); drives the shutdown drain | only its exit and shutdown duties, which could be event-driven (§5) |
| Heap sample (`event_loop`) | ≤ 1/s, only when the loop is polled | the isolate thread | V8 heap statistics for metrics and admission | piggybacks on polls; adds no wakeup |
| V8 platform workers | V8's own delayed tasks | `V8 DefaultWorker` | GC and compile helpers | V8's |
| Reclaimer (`reclaim.rs`) | 1 s, only when enabled | main runtime | idle and pressure reclamation | by design (opt-in) |
| Admission sampling | on decisions, rate-limited (50 ms / 500 ms) | caller | memory readings | not periodic |
| Main Tokio runtime, hyper | timers per connection only | `tokio-rt-worker` | keep-alive and header timeouts | event-driven (0 wakeups/s idle without an executor) |

## 2. The change

The isolate thread now proves it is not stuck in JavaScript through its
own events instead of a timer:

| Event | Hook | Effect |
|---|---|---|
| The runtime is about to park (nothing to run) | Tokio `on_thread_park` | beat, then `parked = true` |
| The runtime wakes up (I/O, a timer, a job, V8 work) | Tokio `on_thread_unpark` | beat, then `parked = false` |
| An event-loop turn starts (`event_loop` poll) | `beat_turn` | beat, at most once per 5 ms (the thread CPU clock read is a system call) |

`busy_for()` and `busy_cpu_for()` are zero while `parked`. Otherwise they
are measured from the last beat, as before.

### 2.1 Safety argument

| Property | Before | After |
|---|---|---|
| A CPU-bound loop (`for (;;) {}`) | one poll never returns; the heartbeat task cannot run; the beat ages; terminated after `stall_timeout` of CPU | the same poll; no turn start, no park; the beat ages from the turn start; terminated identically |
| An endless microtask chain | the same (inside one poll) | the same |
| A turn blocked off the CPU (slow disk, throttling) | wall time ages, CPU does not; logged, terminated at 6 × | identical |
| Many short turns (busy, yielding) | the heartbeat runs between turns | a beat at each turn start (≤ 5 ms apart while busy); never parked long without a beat |
| A stall that starts right after idle | the heartbeat was beating | the unpark beat, then the turn-start beat, ages from there |
| Idle | 40 beats/s | **no wakeups**; `busy_for = 0` |
| Dispatch preference (`BUSY_HINT`, 200 ms) | time since the last beat | the same, zero while parked |
| Ordering | – | the unpark hook beats **before** clearing `parked` (`Release`; the supervisor reads `parked` with `Acquire`), so the supervisor never sees an unparked thread with a beat from before it parked |

**What can differ:**

- A turn that starts within 5 ms of the previous beat is measured from
  that earlier beat. It is judged at most 5 ms early against a 10 s
  default limit (500 ms in the tests).
- `blocked_turn` (log once per blocked turn) may merge turns less than
  5 ms apart, which affects logging only.

**Unchanged:** the supervisor's 50 ms loop, termination repetition, the
CPU/wall rules, shutdown, panic recovery, bootstrap deadlines, the
near-heap-limit handling and reclamation.

### 2.2 Tests

New tests in `tests/supervision.rs`:

- `a_stall_that_starts_after_an_idle_period_is_terminated`: 1 s idle,
  then a CPU loop; terminated as `CpuStall` and replaced.
- `many_short_turns_are_not_a_stall`: 2 s of work in 2 ms slices, each
  yielding to a timer, with a 500 ms limit. It completes and the isolate
  is kept.
- `an_idle_isolate_is_not_woken_by_its_watchdog`: in a child process, the
  isolate thread's scheduler count over 2 s idle.

Every existing supervision, deadline, timer and admission test passes
unchanged: endless loops, microtask loops, blocked turns and the wall
backstop, heap limits, bootstrap deadlines, shutdown. See
[RE-5-CORRECTNESS-VALIDATION](RE-5-CORRECTNESS-VALIDATION.md).

## 3. Measurements

`benchmarks/resource/idle-cpu-process.mjs`, process level, on the
otherwise idle Fedora host (load average below 1 during the runs):

- fixtures/next-app;
- 10 s settle after the first response, then a 60 s window;
- run time from `schedstat` (scheduler nanoseconds; "mcore" = ms of CPU
  per second);
- wakeups = times scheduled (`schedstat` field 3, which includes
  preemptions; an idle thread is rarely preempted).

Traffic:

- **none;**
- **probe:** `/readyz` on the metrics listener every 10 s, as a
  container health check;
- **low:** the probe plus 1 request/s (`/books/7` SSR, or `/` without an
  executor).

The probe runs in a separate process.

Binaries:

- baseline `54daca7…` (`8d83d03`);
- candidate `d0bc4b6…` (`1ddb050` plus this change, before the review
  fixes, which do not touch the heartbeat).

Raw results: `results/re5/idle-cpu/`. This first comparison ran with the
script before review F13 added a process-level CPU cross-check. Threads
that exit inside the window, such as idle blocking-pool threads, are
missing from the sums; they are the same for both binaries.

| Isolates | Traffic | Baseline mcore | Candidate mcore | Change | Wakeups/s baseline → candidate | Isolate threads (wakeups/s) |
|---|---|---|---|---|---|---|
| 0 | none | 0 | 0 | – | 0 → 0 | – |
| 0 | probe | 0.02 | 0.02 | – | 0.3 → 0.3 | – |
| 0 | low | 0.12 | 0.12 | – | 1.35 → 1.35 | – |
| 1 | none | 1.42 | **0.73** | **−49 %** | 75.9 → 21.0 | 55.1 → 0.32 |
| 1 | probe | 1.39 | **0.72** | −48 % | 75.9 → 21.5 | 55.0 → 0.38 |
| 1 | low | 6.97 | 6.42 | −8 % | 93.1 → 38.8 | 57.6 → 2.95 |
| 2 | none | 1.95 | **0.98** | **−50 %** | 150.7 → 41.3 | 110.0 → 0.67 |
| 2 | probe | 2.09 | 0.91 | −56 % | 151.0 → 41.7 | 110.0 → 0.68 |
| 2 | low | 7.94 | 6.92 | −13 % | 168.6 → 59.6 | 112.1 → 3.6 |
| 4 | none | 3.35 | **1.35** | **−60 %** | 300.4 → 81.5 | 219.8 → 1.08 |
| 4 | probe | 3.62 | 1.40 | −61 % | 300.7 → 82.1 | 219.7 → 1.25 |
| 4 | low | 8.95 | 6.90 | −23 % | 318.2 → 99.1 | 221.9 → 3.85 |

- **Without an executor** the process is already event-driven: no
  wakeups while idle.
- **Per idle isolate:**
  - CPU fell by about half;
  - wakeups by 72–73 %;
  - the isolate thread went from 55 wakeups/s (the 40/s timer, and the
    turns it caused) to under 0.4/s.
- **At 1 request/s** the request's own work dominates (about 5–6 mcore
  per request per second). The saving is the same absolute 0.5 mcore per
  isolate.
- **A container's health-check process** (curl or wget every 10 s) is
  not in these numbers. It runs in its own process and costs the
  container more than the probe's server-side work.

The test `an_idle_isolate_is_not_woken_by_its_watchdog` measured **0**
scheduler wakeups of the isolate thread in 2 s idle, in a child process.
The baseline's 55/s would be 110.

## 4. What is left

| Remaining per idle isolate | Wakeups/s | Notes |
|---|---|---|
| Supervisor loop (50 ms) | 20 | §5; about 0.6 mcore per isolate |
| V8 platform workers | 0.7–0.9 (all isolates) | V8's delayed tasks |
| Isolate thread | 0.3 | V8 foreground tasks, heap sample |

Against the brief's aspirational target, idle process CPU is **−49 to
−61 %** with isolates, and unchanged (already 0) without. The target is
met at process level. No container measurement was run.

## 5. Supervisor (not changed)

The supervisor still wakes 20 times a second per isolate. Making it
event-driven means waiting on three things at once:

- the isolate's exit;
- a termination request;
- shutdown;
- and, to keep the stall-detection latency, the isolate's transition from
  parked to busy.

That needs one doorbell (a condition variable or a channel) that the exit
path, `terminate`, `begin_shutdown` and the unpark hook all ring, and a
supervisor that sleeps long only while the isolate is parked. It is not
done in RE-5:

- **It changes the watchdog's own loop.** Its correctness under panics
  (an exit without a message) and under slow terminations needs its own
  review and fault tests.
- **Every termination and shutdown path depends on the 50 ms loop**
  repeating `interrupt()` until the thread reports.

A bounded alternative, a longer timeout only while parked, would delay
detection of a stall that begins right after idle by up to that timeout.
That is a weakening, so it is not adopted.
