# Regression validation

Correctness checks for the two code changes of this investigation
(`2f43289` metrics, `91a7729` reuse of decoded cache entries), and the
performance regression gate against their exact parent commits.

## Correctness

| Check | Where | Result |
|---|---|---|
| `cargo fmt --all -- --check` | Linux (Rust 1.98, as CI), branch head `9df8c73` | Pass |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Linux, `9df8c73` | Pass |
| `cargo clippy -p nextrs-server -p nextrs-runtime -p nextrs-cli --all-targets -- -D warnings` | macOS, after each change | Pass |
| New tests: `nextrs-server --test serve_metrics` | macOS | Pass |
| New tests: store unit tests (`a_known_generation_is_answered_unchanged_only_while_current`, `an_evicted_entry_is_never_unchanged`, `an_unchanged_entry_on_disk_is_not_read`) and all `cache::` unit tests | macOS | Pass (19 of 19) |
| New tests: `unchanged_page_entries_are_reused_never_stale`, `a_page_written_elsewhere_replaces_a_reused_copy` (executor, real JavaScript handler, 1 and 4 isolates) | macOS | Pass |
| `cargo test --workspace` | CI on the pull request | See the pull request's checks |
| Next.js comparison suites (`compat`: pinned Next.js at 1 and 4 isolates, 16.4.5), real-world proxy suites, graceful shutdown | CI on the pull request | See the pull request's checks |
| `cargo audit` | CI on the pull request | See the pull request's checks |
| Response parity on Velasia Airways (77 requests: static, RSC, HTML, HEAD, conditional, range) | Local, `capture.mjs` | `optA` identical to `instr`; identical decoded bodies, status and encoding to `next start` ([BASELINE.md](BASELINE.md#correctness-and-parity)) |

Not weakened: no test assertion or expected value was changed. The two
code commits add tests only.

### Flaky tests seen locally (not caused by these changes)

On this shared host, under heavy load (host load averages of 30 to over
400 from other work):

- `nextrs-runtime --test cache`: one different test failed in each of
  four full runs (`status == 200` at `tests/cache.rs:67` or `:429`, `:390`)
  **on the parent commit `2f43289` as well**, with the same pattern; each
  failing test passed when run alone. The tests use `small_limits()` (a
  500 ms stall limit), which a starved debug build can exceed.
- `nextrs-runtime --test body`: `large_streamed_body_arrives_intact_in_many_reads`
  and `simultaneous_uploads_stay_separate` failed once in Linux while
  another build compiled in the same VM. These tests exercise request-body
  streaming, which neither change touches.

The CI runs on the pull request are the reference result.

## Performance regression gate

Against the exact parent commit of each change, under equivalent
conditions (same images, CPU sets and workloads; gated series, 3 rounds):

| Change | Parent | Throughput | p50 / p95 | CPU per request | Idle RSS | Peak RSS | After recovery |
|---|---|---|---|---|---|---|---|
| `2f43289` (metrics) | `80f6d80` | Static 4,067 vs 4,156; RSC 222 vs 244 req/s (ranges overlap) | Within noise | Static 0.459 vs 0.447; RSC 6.48 vs 6.01 ms (ranges overlap) | 67.8 vs 68.0 MiB | 303 vs 302 MiB | 134 vs 134 MiB |
| `91a7729` (reuse) | `2f43289` | RSC **548** vs 222 req/s; static 4,252 vs 4,067 | RSC at 50/s: **5.6 / 16.0** vs 7.5 / 24.0 ms | RSC **2.98** vs 6.48 ms; static 0.438 vs 0.459 ms | 68.3 vs 67.8 MiB | **230** vs 303 MiB | 130 vs 134 MiB |

No material regression was found on any measure. The metrics commit's
overhead cannot be resolved by these measurements (see
[OPTIMIZATION-RESULTS.md](OPTIMIZATION-RESULTS.md#instrumentation-overhead)).

Known costs of `91a7729`: about 3 MiB more V8 heap per isolate for the 14
pages' decoded entries (bounded at 16 MiB per isolate or the cache's
memory budget, whichever is smaller).
