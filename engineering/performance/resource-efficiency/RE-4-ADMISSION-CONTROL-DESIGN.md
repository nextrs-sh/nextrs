# RE-4B: memory-aware admission control (design)

An optional admission policy for requests that need JavaScript: before
a request is given an isolate slot, the policy checks the container's
memory state and refuses or defers the request when admitting it would
push the container toward its limit. Built on the accounting model in
[RE-4-RESOURCE-ACCOUNTING](RE-4-RESOURCE-ACCOUNTING.md). Off by default
(`--memory-admission`).

**What it is not.** It is not a memory limit and it does not prevent
OOM kills. It refuses *requests* using a measured total and per-request
estimates. A single admitted request can still allocate more than its
estimate. Hard limits on allocations are separate (RE-4C, RE-4D).

## 1. Where it sits

```
HTTP ──► native routes (static, prerendered, image, health, metrics) ── no JS admission
     └─► dynamic route ──► Executor::render ──► PoolInner::acquire
                                                 ├─ concurrency (existing: max_active_per_isolate, max_queued, queue_timeout)
                                                 ├─ memory gate (new, opt-in)
                                                 └─ isolate choice (existing; new: skip isolates near their heap limit)
```

- The gate runs under the pool lock that `acquire` and `dispatch_waiters`
  already take. Reservations and decisions are therefore serialized: two
  simultaneous requests cannot both spend the same headroom. No new lock
  is added.
- Native routes never pass through it. Static files, prerendered pages,
  `/healthz` and `/readyz` keep their fast paths and cannot be blocked by
  it. Their buffers are bounded separately (RE-4C).

## 2. Inputs

| Input | Source | Freshness |
|---|---|---|
| Budget | `--memory-admission-budget` (MiB), else the effective cgroup limit (lowest `memory.max` / `memory.high` up the tree) | startup; re-read with each full sample |
| Usage | cgroup `memory.current` (one number, cheap), or without a cgroup and with an explicit budget, the process's resident anonymous memory | re-read on demand when the last reading is older than 50 ms |
| `inactive_file` | cgroup `memory.stat` (larger file) | re-read when older than 500 ms |
| Recent admissions | reservations made since the current usage reading | exact (under the lock) |
| Active and queued requests | pool state | exact |
| Per-isolate heap used and limit | V8 statistics (existing sampling) | ≤ 1 s while the isolate's event loop runs; an isolate parked on one long timer is not sampled until it wakes |

Sampling is **on demand**: whichever admission finds the reading stale
refreshes it, so an idle server reads nothing. RE-3 measured idle CPU per
thread, and a polling thread would add a wake-up 20 times a second.

## 3. States

`effective = working_set + reserved_since_reading`

| State | Entered when | Admission |
|---|---|---|
| `normal` | effective < pressure threshold | as today (concurrency limits only); the request's reserve is added |
| `elevated` | effective ≥ pressure (default 85 % of budget) | admitted only while total active JS requests < the pressure cap (default 4 per isolate) **and** effective + reserve ≤ critical threshold; otherwise the request **waits in the queue** (bounded: `max_queued`, queue bytes, `queue_timeout`) |
| `critical` | effective ≥ critical (default 95 %) | refused at once: **503** `executor-memory-pressure`, `Retry-After: 2`. Exception (progress): if no JS request is active at all, one is admitted |
| `degraded` | no successful reading for 2 s, or none yet | treated as `elevated` (reduced concurrency, never a full stop) |

**Hysteresis and cooldown.** Moving up (normal → elevated → critical)
is immediate. Moving down requires effective < threshold − 5 points
**and** at least 1 s in the current state. This prevents flapping when
usage hovers at a threshold.

**Reservations.** Every admission adds its reserve to
`reserved_since_reading`. A fresh usage reading resets it to zero,
because the memory of requests admitted before it has started to show
up in the working set. Within one 50 ms reading interval, a burst
therefore cannot spend more than the headroom left by that reading.
Reserve per request:

| Class (from the request, no route configuration) | Reserve |
|---|---|
| render (SSR, RSC, API route, Pages) | `--memory-admission-reserve` (default 2 MiB) |
| Server Action (`POST` with `next-action`) | the same; counted separately in metrics |
| proxy / middleware invocation | half the reserve |
| upload (declared `content-length` > 64 KiB, or chunked) | the reserve + the declared length (capped at `--max-request-body`), or 2× the reserve when chunked |

The reserve is an estimate, not a measurement. Its default came from
the RE-2/3 per-request heap growth on Velasia and needs the Linux runs.

## 4. Per-isolate heap gate

With admission on, an isolate whose sampled `heap_used` is at least 90 %
of its heap limit is skipped while it has requests in flight. Requests
go to other isolates, or wait in the queue, instead of joining an isolate
that is about to hit its limit. A heap-limit termination fails every
request in the isolate, so fewer requests in it means fewer failures.
An idle isolate is never skipped (progress).

## 5. Overload behaviour

| Requirement | How |
|---|---|
| Bounded queue size | existing `max_queued` |
| Bounded queue memory | new: queued requests' header + URL bytes (+ 1 KiB overhead each) ≤ 16 MiB when admission is on; beyond it → `executor-overloaded` |
| Bounded wait | existing `queue_timeout` (10 s) |
| Predictable responses | 503 + `Retry-After` and `x-nextrs-error: executor-memory-pressure` (new); existing codes unchanged |
| Cancellation | unchanged: a dropped request leaves the queue (`LeaveQueue`); a reservation needs no release (it expires with the reading) |
| No starvation | FIFO queue (existing); in `critical`, one request is always admitted when none is active; idle isolates are never skipped |
| No deadlock | the gate never blocks while holding anything other than the pool lock; waiters are woken by releases and installs (existing) and re-check the gate then |
| Health endpoints | native, never gated |
| Server Actions | refused only before delivery; NextRS never retries a request it delivered (existing `render` loop retries only undelivered jobs), so no duplicate execution |
| Rejection metrics | `nextrs_executor_rejected_total{reason="memory_pressure"}` plus per class; deferrals, progress admissions, heap skips, state, transitions, last reading |

Rejected requests are refusals: benchmark drivers must count them as
errors, not as throughput (the resource harness already counts non-2xx
responses as errors).

## 6. Measurement failures

| Situation | Behaviour |
|---|---|
| No cgroup limit and no `--memory-admission-budget` | gate off, warning at startup (nothing to compare against) |
| A read fails at runtime | the last reading is kept; after 2 s without success → `degraded` |
| cgroup v1 | supported (same files as `memory.rs`) |
| Ancestor limit shared with other containers | set `--memory-admission-budget` below it; their usage is not visible (accounting §3) |

## 6a. Nested requests

Next.js's requests to its own server (Server Action redirects and
forwarding, revalidation) are dispatched in-process (`InternalRoute`) on
behalf of a request that was already admitted. They bypass the memory
gate and the heap skip: deferring one would leave its parent waiting on
a child queued behind it, until `queue_timeout`, and refusing one would
fail a Server Action half way. A task-local scope marks them, and the
mark travels on the queued ticket. They remain bounded by the
concurrency limits and `MAX_INTERNAL_REQUESTS` (64). Added after the
independent review.

A failed memory reading is not retried more often than the sampling
interval, because the probe runs under the pool lock.

## 7. Interaction with reclamation

Pressure reclamation (`--memory-pressure-reclaim`, RE-2/3) frees memory.
Admission refuses requests. They should be configured so that reclamation
starts first: pressure reclaim at or below the elevated threshold (the
low-memory profile's 85 % equals the default elevated threshold). Neither
replaces the other.

## 8. Not done

- No adaptive isolate scaling (out of scope).
- No learning of per-route costs: no route classification is available
  without application configuration.
- Not a default and not in any profile until the Linux runs
  ([RE-4-LINUX-BENCHMARK-RUNBOOK](RE-4-LINUX-BENCHMARK-RUNBOOK.md)) show
  that it reduces OOMs without costing throughput inside the envelope.
