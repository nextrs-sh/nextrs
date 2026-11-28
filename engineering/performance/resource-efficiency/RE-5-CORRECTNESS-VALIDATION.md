# RE-5: correctness validation

This report lists every test run for RE-5, the regression tests that
each change added, and what still needs CI or the container campaign.
It covers local runs only:

- Fedora 45 (kernel 7.3.4), Intel i5-9400F, 6 cores;
- the VS Code Flatpak sandbox;
- fixture builds with Node.js 24.18.0. CI pins 22.20.0.

## 1. Results

On the final branch (`8017043`, Rust 1.98.0 unless noted):

| Check | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -D warnings`, 1.98.0 and 1.100.0 | clean on both |
| Workspace tests | **566 passed, 0 failed**, 37 ignored, 47 suites (RE-4.1: 545) |
| Ignored tests (`--ignored --test-threads=1`: fixture-dependent server tests, including `resource_e2e`, and the child-process fault cases) | **37 passed, 0 failed** |
| `tests/nested.rs`, `tests/supervision.rs` | 3 consecutive whole-binary runs each, all passed |
| Next.js compatibility suites (pinned, 16.4.5; 1 and 4 isolates) | **not run locally**: CI on the PR |

The RE-4.1 counts were 545 workspace tests and 30 fixture-dependent
server tests.

RE-5 adds 21 workspace tests:

- 5 nested;
- 1 Web API;
- 4 supervision, one of which is the child-process case;
- 9 prerender-store unit tests;
- 2 prerender parity tests.

It adds 2 fixture-dependent end-to-end tests (`resource_e2e`, now 10)
and rewrites 2.

**CI scope.** CI's "fmt, clippy, tests" job runs the workspace tests and
`dynamic`, `reliability`, `memory` and `proxy` with `--ignored` on a
next-app build. `resource_e2e` (needing next-app, next-proxy and
next-pages builds) is not in CI. It ran locally, as in RE-4.1.

## 2. New and changed tests, by change

| Change | Commit | Tests | Fails without the change |
|---|---|---|---|
| Nested requests take a bounded overdraft slot | `7149b13` | `tests/nested.rs` (5, runtime: no wait for the parent's slot; external requests never use the allowance; recursion bounded (64 permits at first; per isolate since `213c265`); cancellation and shutdown leave no slot; at critical memory children recorded, never gated); `resource_e2e` (real Next.js: redirect to a dynamic page with one slot, rewritten; 8 concurrent actions on 2 slots, rewritten; revalidate-then-redirect with one slot, new; Pages `res.revalidate` with one slot, new) | yes, checked at `7149b13`: all 5 runtime tests and all 4 e2e cases fail with the allowance at 0 (before: empty payloads after 2.0 s, 6 of 8 actions 503, `res.revalidate` 500) |
| File-backed prerender store | `b16dfc3` | 6 store unit tests; `tests/prerender_store.rs` (2 391 header-and-byte comparisons over 5 committed snapshots; changed build file refused) | the parity test fails at its first comparison when the file-backed path drops its ETag (negative control) |
| `generate_etag` without a temporary | `b16dfc3` | the pinned Next.js ETag tests (ASCII, UTF-16 code units, surrogate pairs) | – (same values) |
| Build metadata freed after startup | `1ddb050` | the whole server suite and the fixture tests start servers through the same code path (the CLI change is not reachable from tests); measured in RE-5B | – |
| Multipart parsed in place | `38e5659` | `multipart_parts_are_independent_of_the_parsed_body`; the Node-compared FormData checks; multipart unit tests | – (equivalence) |
| Event-driven isolate heartbeat | `733d295` | `a_stall_that_starts_after_an_idle_period_is_terminated`, `many_short_turns_are_not_a_stall`, `an_idle_isolate_is_not_woken_by_its_watchdog`; every existing supervision, deadline, timer and admission test | the wakeup test was not run against the baseline; RE-5F measured the baseline isolate thread at 55 wakeups/s (110 per 2 s, against the test's limit of 20), and the candidate at 0 |
| Cache file encoding outside the store lock | `287bf3e` | `tests/cache.rs` (persistence, restart, corruption, concurrent writers, tags, revalidation, eviction, cross-isolate visibility), cache unit tests | – (same file bytes) |
| Prerendered header values made once | `c721cb2` | `tests/prerender_store.rs` (every header compared), `tests/http.rs`, `tests/compression.rs` | – (same headers) |
| Independent review fixes | `213c265` | `overdraft_is_bounded_per_isolate_and_recursion_ends`; the nested shutdown case; `with_a_budget_kept_entries_are_reserved_until_evicted`, `a_body_larger_than_the_whole_budget_is_served_unreserved`, `a_replacement_with_the_same_length_and_mtime_is_refused` | the per-isolate bound changes the earlier 64-deep recursion and two-children shutdown cases (now queued, as before RE-5) |

## 3. Controls on and off

- **Resource controls on:** `resource_e2e` (admission at critical and in
  emergency, native buffer budget, proxy bodies) and `tests/admission.rs`
  run with the controls on.
- **Prerender store:** `tests/prerender_store.rs` runs the same requests
  against servers without it and with three cache sizes.
- **Everything else:** the default configuration.

## 4. Memory-safety tools

- **New `unsafe`:** none in RE-5. The multipart change uses `JsBuffer`
  and `Bytes::from_owner` (safe APIs). The heartbeat uses atomics and
  Tokio hooks.
- **Miri and the sanitizers** were not run: they cannot execute the V8
  integration (a C++ library, JIT code). The new pure-Rust module
  (`prerender_store`) has no `unsafe`.
- **Passing tests are not a proof of memory safety.** The ownership
  arguments are in the RE-5C and RE-5F reports and the independent
  review.

## 5. What remains for CI and the campaign

- **CI on the PR:** fmt, clippy, workspace tests, the pinned and 16.4.5
  compatibility suites at 1 and 4 isolates (the suites run with every
  control off), and the RustSec audit.
- **The container campaign:** sampled vs committed accounting,
  `--prerender-cache` in a cgroup (page-cache charging), young-generation
  sizing, fault workloads, soaks. See
  [RE-5-MILESTONE-GATE](RE-5-MILESTONE-GATE.md).
