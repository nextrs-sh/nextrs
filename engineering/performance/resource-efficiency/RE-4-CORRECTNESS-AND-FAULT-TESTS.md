# RE-4I: local correctness and fault tests

Everything that can run without a native container engine: unit tests,
integration tests with real isolates, tests against real Next.js build
snapshots, child-process fault tests, and an end-to-end run of the CLI
binary with the fixture application. Fedora 45 host (bare metal,
i5-9400F), inside the VS Code Flatpak sandbox, Rust 1.98.0 (CI's pin)
unless stated, Node.js 24.18.0 for the fixture app (CI pins 22.20.0).

**Process-level results only.** No test here runs under a cgroup memory
limit, so none of them is a container result.

## Gate

| Run | Result |
|---|---|
| `cargo +1.98.0 fmt --all --check` | clean |
| `cargo +1.98.0 clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo +stable clippy …` (Rust 1.100.0) | clean (the `needless_range_loop` RE-23B found is fixed in `3615a71`) |
| `cargo +1.98.0 test --workspace --no-fail-fast` at `14f9404` | **513 passed, 0 failed**, 26 ignored (fixture-dependent, as before), 44 suites (RE-23B: 448 / 40). At `550c5ff`: 511 / 0 |
| Intermittent | one failure of `cache.rs` `every_isolate_sees_one_cache` (a tag revalidation not yet visible to the next read) in one full-workspace run; 0 failures in 80 reruns of that test (40 with a competing CPU spinner) and in the next full run. The shared data cache is not changed by RE-4 and the test runs with every RE-4 control off; recorded, not attributed. *Root-caused and fixed in RE-4.1: an isolate's `performance.timeOrigin` taken across a preemption ([RE-4.1-CACHE-CORRECTNESS](RE-4.1-CACHE-CORRECTNESS.md))* |
| `cargo +1.98.0 test -p nextrs-server --test dynamic --test reliability --test memory --test proxy -- --ignored` (CI's fixture job), at `550c5ff` and again at `14f9404` | **22 passed, 0 failed** (dynamic 7, memory 1, proxy 1, reliability 13), against `fixtures/next-app` built with Next.js (Node.js 24.18.0) |
| End-to-end, CLI binary + `fixtures/next-app` | release binary at `14f9404` ([results/re4-local](results/re4-local/SUMMARY.md)): with every control off and with each on, `/`, `/dynamic`, `/books/42`, `/api/health`, `/request-info` and a 229 kB static chunk answer 200, as do `/readyz` and `/healthz`. With admission at the critical level, 23 of 24 concurrent dynamic requests got 503 `executor-memory-pressure` with `Retry-After: 2` while sequential requests, static files and health stayed 200 (`/data-fetch` is 500 in every run, controls off included: it needs `NEXTRS_UPSTREAM`) |
| Compatibility suites (`scripts/compat-suite.sh`) | run by CI on the PR (pinned Next.js with 1 and 4 isolates, Next.js 16.4.5) |

## New tests by area

| Brief item | Tests | File |
|---|---|---|
| Admission budget arithmetic | thresholds, inactive page cache excluded, configured budget below the limit, no limit refused at construction, invalid thresholds | `nextrs-runtime/src/admission.rs` (unit) |
| Concurrent reservations | `a_burst_cannot_spend_the_same_headroom_twice` (exactly 7 × 2 MiB in 80 → 95 MiB); ArrayBuffer budget `concurrent_reservations_never_cross_the_limit` (8 threads) | `admission.rs`, `external.rs` (unit) |
| Reservation release | `a_fresh_reading_resets_reservations`; native permits released by body drop; ArrayBuffer budget back to ~0 after isolate replacement | `admission.rs`, `native_budget.rs`, `tests/native_budget.rs`, `tests/external_memory.rs` |
| Queue saturation, bounded queue memory | `the_queue_is_bounded_in_bytes`; existing `queue_deadline_and_queue_bound_are_enforced` | `tests/admission.rs`, `tests/deadlines.rs` |
| Admission timeout, FIFO, no starvation | `elevated_caps_concurrency_and_the_rest_run_in_arrival_order` (order a, b, c, d); progress admission when nothing runs | `tests/admission.rs` |
| Request cancellation | `a_cancelled_waiter_leaves_the_queue_and_frees_its_bytes`; existing `cancelling_while_queued_never_executes_the_request` | `tests/admission.rs`, `tests/deadlines.rs` |
| Critical state, recovery | `critical_refuses_while_others_run_and_always_admits_one`, `queued_requests_behind_a_running_one_are_refused_at_critical`, `admission_recovers_when_memory_falls` | `tests/admission.rs` |
| Server Actions | `a_refused_server_action_never_runs_and_an_admitted_one_runs_once` | `tests/admission.rs` |
| Stale measurements, missing cgroup | `failed_readings_degrade_after_the_stale_limit_and_recover` (unit), `failed_readings_reduce_concurrency_but_never_stop_traffic` (isolates), `no_limit_and_no_budget_is_refused_at_construction` | `admission.rs`, `tests/admission.rs` |
| Multiple isolates, heap state | `an_isolate_near_its_heap_limit_takes_no_more_requests_while_busy`; shared ArrayBuffer budget over 2 isolates | `tests/admission.rs`, `tests/external_memory.rs` |
| Isolate replacement | budget released with replaced isolates; heap-limit episodes recorded with replacement time | `tests/external_memory.rs`, `tests/heap_limits.rs` |
| Graceful shutdown | `shutdown_with_deferred_waiters_does_not_hang` (waiter gets `ShuttingDown`, shutdown completes) | `tests/admission.rs` |
| External-memory growth | RangeError past the limit, GC before refusal, fill and release, small allocations never refused, API sweep (child process) | `tests/external_memory.rs` |
| Large JavaScript allocations | recoverable (child), fatal (child, documents the abort), refusal then full heap (child), refusal alone (child), refusal with 10–35 MiB of the heap held (child, from the review) | `tests/heap_limits.rs` |
| Nested requests | in-process self-requests are marked and bypass the gate; the mark does not leak into spawned tasks | `admission.rs` (unit) |
| Native allocation budgets | 7 integration tests on a Next.js 16.4.3 snapshot (hold until drop, wait then 503, HEAD/304 reserve nothing, ranges, oversize file, 64 concurrent, budget off) + 3 unit | `nextrs-server/tests/native_budget.rs`, `native_budget.rs` |
| Large request bodies, slow clients, streaming backpressure | existing `body.rs` (6 MiB streamed, slow uploads, aborted upload, 413), `backpressure.rs`; RE-4 changes no body path when its flags are off | `nextrs-runtime/tests` |
| Young generation | 6 unit tests on the sizing arithmetic; real V8 keeps the heap limit for 4, 8, 16 MiB floors | `worker.rs`, `tests/memory.rs` |
| 503 mapping | `refusals_are_retryable_503s_and_memory_pressure_asks_for_longer` | `nextrs-server/src/dynamic.rs` |
| CLI | defaults off; flags parsed; sub-flags need their switch; invalid values refused | `nextrs-cli` `resource_controls` |
| Existing runtime profiles | `memory_settings` tests unchanged and passing; profiles do not set any RE-4 flag | `nextrs-cli` |

Fault tests that may abort run in child processes: the parent test
starts the test binary on one case with `NEXTRS_TEST_CHILD` set. Without
it, the child case returns at once, so a normal or `--ignored` run never
executes a fatal case in-process.

## Blocked (needs a native container engine with cgroup v2)

| Test | Why it cannot run here |
|---|---|
| Admission against a real cgroup limit (OOM prevented or not at 256 MiB) | no cgroup memory limit can be set from the sandbox |
| Native memory exhaustion to `memory.max` | same |
| Concurrent SSR / queue saturation / slow streams inside 256 MiB | same; the process-level tests check the mechanisms, not the memory outcome |
| Several isolates sharing 256 MiB | same |
| Repeated pressure and recovery cycles in a container | same |
| CPU per request and GC with the young-generation floor | needs the dedicated host; timing in this sandbox is not representative |

All of them are specified in [RE-4-LINUX-BENCHMARK-RUNBOOK](RE-4-LINUX-BENCHMARK-RUNBOOK.md) §6.
