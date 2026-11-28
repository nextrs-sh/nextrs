# RE-4.1B/C: admission accounting and critical pressure

Two questions about `--memory-admission`:

- **B:** does resetting reservations at every memory reading under-count
  requests that are admitted but have not allocated yet?
- **C:** is the "one request when idle" exception at critical safe?

Commit `1d10201`. Defaults are unchanged: the RE-4 policy stays the
default, and everything new is opt-in.

## 1. Lifecycle (all policies)

Every admission now records a **commitment** (`admission.rs:804`) owned
by the request's `Slot`. It ends exactly once:

| Path | How the commitment ends |
|---|---|
| Request completes (JavaScript reports done) | `Slot::drop` → `release` → `Gate::complete` |
| Client gone, request timeout, stall watchdog, heap-limit termination, isolate replaced | the slot is dropped with the request's op state or the isolate → `complete` |
| Waiter timed out between being granted and receiving the slot | `disarm` → `Gate::withdraw` (also removes the estimate from `reserved`) |
| Isolate died after being chosen, before taking the job (retry) | `Slot::release_now` before re-acquiring; the retry (`Ticket::retry`) is not gated again (it was admitted; concurrency limits still apply) and is not added to `reserved` twice (`pool.rs:559`, `:1176`) |
| Shutdown | isolates dropped → slots dropped |
| Nested in-process request | commitment recorded (its memory is real) but never gated |

**Default-policy differences from RE-4, intentional** (RE-4.1 review
F3): under `--memory-admission` with `sampled` accounting, a retry is no
longer charged to `reserved` a second time, nor re-gated, and a waiter
that had gone when it was granted is taken back out of `reserved`. Both
were over-counts.

This **fixes RE-4 review finding 5**. Before, the retry re-acquired while
still holding the dead slot: the dead slot counted as active, and the
reserve was charged twice.

Metrics (whatever the policy): `nextrs_admission_commitments`,
`_committed_bytes`, `_unrealized_bytes`, next to RE-4's
`_reserved_bytes`. A container run therefore records what both policies
would have seen.

## 2. The question: delayed allocation

| Policy | Estimate counts | Weakness |
|---|---|---|
| `sampled` (RE-4, default) | until the next reading (≤ 50 ms) | a request that allocates later (after awaiting a backend, a body, serialization) is never counted before it allocates |
| retain until completion (pure) | until the request ends | counts memory twice once it shows up in the working set; starves long-lived streams |
| **`committed` (new, opt-in)** | until the request ends, **less measured working-set growth** (credited to the oldest estimates first), **at most `--memory-admission-commitment-ttl`** (2 s) | growth from other sources is credited too (under-count, never below `sampled`); a GC that frees as much as requests allocate hides their growth until they end (bounded double count); TTL bounds protection to that window |

`effective = working set + reserved` (sampled) or `+ unrealized`
(committed). Credit and expiry run under the pool lock at each reading
(`admission.rs:648`, `:666`). This is O(live requests), a few hundred at
most, with no new lock.

### Model (deterministic, virtual time)

`cargo test -p nextrs-runtime --lib delayed_allocation -- --nocapture`:

- Gate: real `Gate` code with a scripted probe, 1 ms steps, the 50 ms
  sampling default, and a 10 s queue timeout.
- Container: 256 MiB with 150 MiB resident.
- Load: 200 SSR requests over 1 s. Each waits *delay*, then allocates its
  4 MiB estimate for 300 ms.
- Cells: peak working set; admitted / refused / timed out.

Default elevated cap (4 active per isolate):

| delay | sampled | committed, TTL 1 s | TTL 2 s | TTL 5 s |
|---|---|---|---|---|
| 0 ms | 218 MiB, 200/0/0 | 218 MiB, 200/0/0 | 218 MiB, 200/0/0 | 218 MiB, 200/0/0 |
| 20 ms | 230 MiB, 200/0/0 | 218 MiB, 200/0/0 | 218 MiB, 200/0/0 | 218 MiB, 200/0/0 |
| 50 ms | **254 MiB**, 28/172/0 | 218 MiB, 200/0/0 | 218 MiB, 200/0/0 | 218 MiB, 200/0/0 |
| 100 ms | **294 MiB**, 38/162/0 | 218 MiB, 200/0/0 | 218 MiB, 200/0/0 | 218 MiB, 200/0/0 |
| 300 ms | **390 MiB**, 77/123/0 | 218 MiB, 191/0/9 | 218 MiB, 191/0/9 | 218 MiB, 191/0/9 |
| 1000 ms | **390 MiB**, 200/0/0 | 218 MiB, 176/0/24 | 218 MiB, 102/0/98 | 218 MiB, 102/0/98 |
| 3000 ms | **390 MiB**, 200/0/0 | 218 MiB, 176/0/24 | 218 MiB, 127/0/73 | 218 MiB, 68/0/132 |

No elevated cap (accounting alone):

| delay | sampled | committed, TTL 1 s | TTL 2 s | TTL 5 s |
|---|---|---|---|---|
| 0 ms | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 |
| 20 ms | **254 MiB**, 28/172/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 |
| 100 ms | **318 MiB**, 43/157/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 |
| 300 ms | **390 MiB**, 82/118/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 |
| 1000 ms | **390 MiB**, 200/0/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 | 242 MiB, 200/0/0 |
| 3000 ms | **390 MiB**, 200/0/0 | 242 MiB, 200/0/0 | 242 MiB, 161/0/39 | 242 MiB, 92/0/108 |

Long-lived streams: 200 over 1 s, each reserving 2 MiB, allocating
64 KiB and living 30 s, no elevated cap. Sampled admits 200/0/0.
Committed: TTL 1 s 200/0/0, TTL 2 s 200/0/0, TTL 5 s **131/0/69**
(estimates that never materialize hold the rest back until they expire).

Reading the tables:

- In this model, **sampled oversubscribes the container whenever
  allocation trails admission by more than about one reading interval**
  (50 ms). The peak is 390 MiB in a 256 MiB container: an OOM kill. Its
  refusals arrive only after the memory is already there.
- **Committed keeps the peak at or below the critical threshold (243 MiB)
  while estimates live.** The asserted invariant holds for every
  TTL > delay. Its cost is deferral: requests wait, and with the default
  elevated cap and long requests some reach the queue timeout. That cost
  comes mostly from the cap of 4 (second table vs first).
- The TTL trades protection window against starvation of long-lived
  streams. At 2 s, both stream and SSR cases up to 1 s pass without
  timeouts when the cap is off.
- The model assumes exact estimates. Real requests allocate more or less
  than the 2 MiB default. An estimate that is too low weakens both
  policies equally.

### Real isolates

`crates/nextrs-runtime/tests/admission.rs` (15 tests, all pass):

- **Delayed allocation:**
  `committed_accounting_holds_back_requests_whose_estimates_are_outstanding`.
  A 1000 MiB budget at 900 MiB used, critical at 95 %, 2 MiB estimates,
  40 requests waiting 1.5 s on timers. Sampled admits **40**; committed
  admits **25** (= ⌊(950 − 900) / 2⌋ + 1) and defers the rest.
- **Completion, cancellation, timeout, isolate replacement:**
  `commitments_end_with_their_requests_on_every_path`. The replacement
  case is a CPU loop that the stall watchdog terminates. Commitments
  return to (0, 0) after each path.
- **Queue timeout:** `a_waiter_that_times_out_leaves_no_commitment`.
- **Shutdown:** `shutdown_releases_every_commitment`.
- Unit tests (`--lib admission`, 26): release exactly once, withdraw,
  retry and nested not added to `reserved`, credit without double
  counting, expiry, validation.

**Retry race:** the isolate must die between `claim` and `send`, a window
too small to provoke reliably. It is covered at the gate level
(`retries_and_nested_requests_are_not_added_to_reserved`) and by code
inspection of `release_now`.

### Evaluation

| Criterion | sampled | committed |
|---|---|---|
| Admission correctness under delayed allocation | under-counts (model: 390 MiB in 256) | counts while estimates live |
| Oversubscription risk | high when allocation trails admission by > 50 ms | bounded by estimate accuracy and TTL |
| Starvation / false refusals | none from accounting | deferrals; long-lived streams wait up to TTL |
| CPU | O(1) per admission | O(live) per reading, under the existing lock |
| Lock contention | unchanged | unchanged lock; a few hundred list operations at most |
| Default behaviour change | none | opt-in |

**Decision:** keep `sampled` as the default (no evidence from a real
container yet). Offer `committed` for the container campaign: the
exported totals show both from one run. Recommendation for the campaign:
`committed`, TTL 2 s, compared against `sampled` at 256 MiB and 512 MiB.

## 3. Critical pressure (RE-4.1C)

RE-4's progress rule admits one request whenever none runs, even at
critical. That avoids an outage but lets sequential traffic allocate
without limit.

- **New optional `emergency` state** (`--memory-admission-emergency
  PERCENT`, at least critical; validated). From that level, new
  JavaScript requests are refused even when none runs, except one per
  `--memory-admission-emergency-interval` (5 s). With `0`, none are
  admitted until memory falls.
- **Starvation:** the interval keeps an application whose resident set
  alone sits above the threshold serving, slowly. Only the explicit `0`
  setting refuses all JavaScript.
- **Recovery needs no JavaScript.** The next request triggers a reading,
  a cgroup file read, and is admitted as soon as memory is below the
  threshold minus hysteresis. Tested:
  `emergency_without_an_interval_admits_nothing_until_memory_falls`
  (unit), and end to end
  `emergency_admits_one_action_per_interval_and_never_runs_a_refused_one`
  (the refused action is answered before delivery; after memory falls,
  the next one is admitted).
- **Health endpoints, static files and prerendered pages** never pass the
  gate (unchanged).
- **Reclamation:** with the emergency state configured (only then, so
  RE-4 configurations keep their reclaim schedule), moving up into
  critical or emergency sets a request that the pressure reclaimer takes
  at its next 1 s tick. It then
  reclaims without waiting for its backoff, though never within
  `pressure_interval` (10 s) of the previous pressure reclamation, so a
  container oscillating at the threshold is not collected more often
  than with RE-2/3 (`reclaim.rs:300`). Without `--memory-pressure-reclaim`
  the request is ignored. Metric:
  `nextrs_memory_reclaim_admission_requested_total`.
- **Thresholds:** all stay opt-in until the container campaign
  establishes them. No emergency threshold is recommended yet. If one is
  tried: `--memory-admission-emergency 98` with
  `--memory-pressure-reclaim 85`.

## 4. CLI

`--memory-admission-accounting sampled|committed`,
`--memory-admission-commitment-ttl SECONDS` (2),
`--memory-admission-emergency PERCENT`,
`--memory-admission-emergency-interval SECONDS` (5; 0 = none). All
require `--memory-admission`. Invalid values (NaN, out of range, below
critical, a zero TTL) are refused at startup. Tested in
`nextrs-cli resource_controls`.
