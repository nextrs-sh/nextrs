# RE-23B: compatibility and CI

**Status: complete. No compatibility regression; CI is green; the
streaming test failure is explained.** It is a timing margin in a debug
build, not a streaming correctness bug. Follow-up:
[#52](https://github.com/nextrs-sh/nextrs/issues/52).

The candidate is `main` at `c65731b` (PR #51 merged). That is `re2c` plus
`cargo fmt`. The baseline for the streaming reproduction is `79e4fe5`
(`main` before RE-2/3, the commit RE-2/3 used). Host: the Fedora
workstation in [RE-23B-LINUX-ENVIRONMENT](RE-23B-LINUX-ENVIRONMENT.md),
on bare metal, not in a container.

## CI on GitHub

| Run | Commit | fmt, clippy, tests | RustSec | next pinned, 1 isolate | next pinned, 4 isolates | next 16.4.5, 1 isolate |
|---|---|---|---|---|---|---|
| [46099611677](https://github.com/nextrs-sh/nextrs/actions/runs/46099611677) (PR #51) | `af5bb49` | pass | pass | pass | pass | pass |
| [46099805442](https://github.com/nextrs-sh/nextrs/actions/runs/46099805442) (push to `main`) | `c65731b` | pass | pass | pass | pass | pass |

Each compatibility job runs:

- the fixture-dependent Rust tests that are ignored locally:
  `nextrs-server` `dynamic`, `reliability`, `memory` and `proxy` with
  `--ignored`;
- the HTTP comparison against `next start`, with Cache Components,
  proxy, images, Pages and (pinned) browser, adapter and proxy
  combinations;
- the real-world proxy applications;
- the graceful-shutdown test.

PR #51 was merged at 02:47:49Z, before its three compatibility jobs
finished (03:06–03:11Z). They passed, and so did the same jobs on the
merge commit, so nothing is outstanding. Merging before the checks
finish is a process gap, not a result.

## Local gate (Linux, `c65731b`)

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo +1.98.0 clippy --workspace --all-targets -- -D warnings` (CI's toolchain) | clean |
| `cargo clippy` on Rust 1.100.0 (newer than CI) | 1 error: `needless_range_loop` at `nextrs-runtime/src/pool.rs:501`, in code from 2026-11-04 (`ab1b48c`). New in that clippy version, not caused by RE-2/3. |
| `cargo test --workspace` (Rust 1.100.0) | **448 passed, 0 failed**, 26 ignored (the fixture-dependent tests above, which CI runs), 40 suites |

On the Mac's Docker builder, RE-2/3 recorded 447 of 448. The one test
that failed there passes here.

## `large_streamed_body_arrives_intact_in_many_reads`

The test streams a 6 MiB body in 64 KiB chunks to a handler
(`tests/fixtures/handlers/dist/echo.js`) that hashes it as it reads, then
checks the byte count, the SHA-256 and that there were at least 96
reads. It uses `small_limits()`, which sets a **500 ms CPU stall limit**.

### Reproduction on both commits

| Condition | `79e4fe5` (baseline) | `c65731b` (candidate) |
|---|---|---|
| Unmodified test, 30 runs | 30/30 pass | 30/30 pass |
| Pinned to one core (`taskset -c 0`), 30 runs | 30/30 pass | 30/30 pass |
| One core plus two busy loops on the same core, 20 runs | 20/20 pass | 20/20 pass |

It does not fail on this host, with or without CPU contention.

### What the test is sensitive to

The tests below used scratch copies of the test (in
`results/re23b-safety/src/`, not part of the test suite) with the stall
limit and the payload size as parameters. All runs are on `c65731b`, 10
runs per point.

| Stall limit | 500 | 450 | 400 | 350 | 300 | 250 | 200 | 100 | 50 | 25 | 10 | 5 ms |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Pass | 10 | 10 | 10 | 10 | **0** | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

- **The threshold is sharp:** 350 ms and above always pass, 300 ms and
  below always fail. The test's busiest isolate turn uses 300–350 ms of
  CPU in a debug build on this CPU. That is a margin of about 1.5×
  against the 500 ms limit.
- **The failure is the stall watchdog.** At 200 ms the executor records
  `cpu_stall` (1) and restarts the isolate, and the response never
  ends. At 350 ms it returns 200 with 96 reads and the correct SHA-256.
  Whenever the handler completes, the data is intact.
- **The busiest turn grows with the size of the whole body, not with the
  chunk size.**

  | Payload | 100 ms limit | 300 ms limit | 350 ms limit |
  |---|---|---|---|
  | up to 1 MiB | pass | pass | pass |
  | 3 MiB | fail | pass | pass |
  | 6 MiB | fail | fail | pass |

  Every chunk is already available, so `for await (const chunk of req)`
  resolves through microtasks and never returns to the event loop. The
  25 ms heartbeat cannot run, so the watchdog sees the whole 6 MiB
  read-and-hash as one turn.
- **Contention alone does not trip it on bare metal.** The watchdog reads
  the isolate thread's CPU clock (`pthread_getcpuclockid`), so time spent
  waiting for a core is not counted.

### Conclusion

The failure depends on the environment and existed before RE-2/3: it
appears on `79e4fe5` as well, and the code under test is unchanged. A host where this turn costs
about 1.5× more thread CPU time fails the test: an older or slower CPU,
a Docker Desktop VM, or a VM whose vCPU preemption is charged to the
running guest thread when steal time is not accounted. The Mac failure
was not kept, so which of these applied there is inferred, not
observed.

There is no production impact in the measured envelope. Release builds
run this turn much faster, the default `--js-stall-timeout` is 10 s, and
`--max-request-body` defaults to 10 MiB.

**Follow-up:** [#52](https://github.com/nextrs-sh/nextrs/issues/52).
Remove the stall limit from this test and from
`simultaneous_uploads_stay_separate` (as the heap-limit tests in
`supervision.rs` already do), since the watchdog is not what they test.
This test-only change is not part of RE-23B.

## Independent review fixes in the candidate

Every RE-2/3 review fix ([RE-23-INDEPENDENT-REVIEW](RE-23-INDEPENDENT-REVIEW.md))
is in `c65731b`:

| # | Where it is |
|---|---|
| M1 flat 64 MiB grant | `nextrs-runtime/src/worker.rs:33` `HEAP_HEADROOM`, `:158` callback; test `supervision.rs:149` |
| M2 `arrivals` counter, 0.3 req/s allowance | `nextrs-server/src/http_metrics.rs:265,310`, `reclaim.rs:285` |
| L1 `pressure_unhelpful` | `nextrs-server/src/reclaim.rs:184-259` |
| L2 `..` paths resolve to no cgroup | `nextrs-runtime/src/memory.rs:217-231` (`Component::Normal` only) |
| L3 `V8 heap limit in effect` | `nextrs-runtime/src/worker.rs:171`, CLI warning `nextrs-cli/src/main.rs:1046` |
| L4 semi-space rounded down to a power of two | `nextrs-runtime/src/worker.rs:224-228` |
| L5 heartbeat around the low-memory collection | `nextrs-runtime/src/worker.rs:247,258` |
| L6 one-isolate module-state test | `nextrs-runtime/tests/memory.rs:68` |

## Reproduction

```sh
cargo fmt --all -- --check
cargo +1.98.0 clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo +1.98.0 test -p nextrs-runtime --test body           # on each commit
# stall-margin sweep: copy results/re23b-safety/src/re23b_body_margin.rs to
# crates/nextrs-runtime/tests/, then
RE23B_STALL_MS=300 RE23B_BYTES=6291456 cargo test -p nextrs-runtime --test re23b_body_margin -- --nocapture
```

Data: [`results/re23b-ci/`](results/re23b-ci/) contains the CI job
list, the local gate log, the clippy logs, the per-condition loop counts,
the sweeps, and the test-binary hashes.
