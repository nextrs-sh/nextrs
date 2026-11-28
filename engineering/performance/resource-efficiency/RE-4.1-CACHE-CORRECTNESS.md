# RE-4.1: the intermittent `every_isolate_sees_one_cache` failure

RE-4I recorded one failure of `tests/cache.rs` `every_isolate_sees_one_cache`
in a full-workspace run: after a tag revalidation, the next tagged read
still returned the entry. The test then passed 80 targeted reruns (40 with
a CPU spinner) and the next full run. This milestone finds the cause.

Fedora 45 host (bare metal, i5-9400F, 6 cores, `tsc` clocksource), VS Code
Flatpak sandbox, Rust 1.98.0, debug test builds. Process-level results.
Paths are under `crates/nextrs-runtime/`; line numbers are at the fix
commit.

## Classification

**A genuine cross-isolate coherency defect, caused by an existing race
outside the cache.** The shared store is coherent; the clock one isolate
judged it with was not. Under CPU contention one isolate in 280 to 550
got a `performance.timeOrigin` 1 to 23 ms too early, so its clock lagged the
wall clock by that much for its whole life, and a tag that the store had
just stamped as expired was not expired on that isolate yet. Not a test
assumption and not expected asynchronous behaviour: the same revalidation
was visible at once on every other isolate.

## What the test does

`tests/cache.rs:125`: 4 isolates; `set("shared", tags: t)`; 64 concurrent
reads; `revalidateTag(["t"])` through one isolate; 2 ms sleep; 16
sequential reads with tag `t`, each must miss (`tests/cache.rs:149`).
The fixture is `tests/fixtures/handlers/dist/cache.js`, which has no Next.js
install, so the handler judges tags with its own copy of Next's functions
(`js/03_server.js:838`, `localTags`). An app build loads Next's
`tags-manifest.external.js`, which is the same code.

## The path, and what is not the cause

| Step | Source | Finding |
|---|---|---|
| `revalidateTag` → op | `js/03_server.js:1167`, `src/ops.rs:1295` | The op calls `CacheStore::revalidate` before its first await; only the tags-file write (none here, no directory) is awaited after it. The JS awaits the op before the response ends. Ordered, nothing fire-and-forget |
| Stamp | `src/cache.rs:801`, `:807`, `:817` | Under the store mutex: `expired = now_ms()`, the wall clock (`SystemTime`, `src/cache.rs:85`) truncated to ms; `tags_version` bumped and logged (`record_tag`) |
| Entry time | `src/cache.rs:700` | `last_modified = now_ms()` at `set`, same clock |
| Read | `js/03_server.js:1077` | `op_nextrs_cache_get`, then `syncTags()` (`:864`, `src/cache.rs:843`): versioned, under the same mutex, so the read after the revalidation always receives the new `expired` |
| Judgment | `js/03_server.js:843` (Next: `tags-manifest.external.js` `areTagsExpired`) | expired if `expiredAt <= now && expiredAt > timestamp`, with `now = performance.timeOrigin + performance.now()` |

The store, its versioned tag sync and the op ordering are correct. The
two comparisons:

- `expiredAt > timestamp` (entry written strictly before the revalidation,
  both truncated ms): Next's own semantics; a `set` and a `revalidateTag`
  in the same millisecond leave the entry valid under `next start` too.
  In this test 64 reads separate them, several ms. Not the cause.
- `expiredAt <= now`: the stamp is the wall clock truncated, `now` is the
  isolate's performance clock with sub-ms precision. With agreeing clocks
  it holds as soon as the op returns (the 2 ms sleep is not even needed).
  It fails only if the isolate's performance clock is **behind** the wall
  clock. That is the failure.

## Mechanism

deno_web (pinned 0.291.0) estimates the time origin from two clock reads:

```rust
// deno_web-0.291.0/timers.rs:47
let wall_time = SystemTime::now();
let monotonic_time = state.borrow::<StartTime>().elapsed();
let epoch = wall_time.duration_since(UNIX_EPOCH).unwrap() - monotonic_time;
```

`00_globals.js:67` calls it once per isolate (`perf.setTimeOrigin()`).
If the thread is preempted between the two reads, `monotonic_time` is
larger by the delay, the origin is early by the delay, and every later
`performance.timeOrigin + performance.now()` on that isolate lags the wall
clock by the same amount. The error is one-sided (never late) and is
permanent for the isolate's life. Isolates boot on their own threads in
parallel; in a full-workspace run several pools boot at once on 6 cores,
which is when the preemption happens.

On such an isolate a revalidation stamped at wall time T is judged
"expired" only once that isolate's clock reaches T, i.e. lag ms later; on
every other isolate at once. The test's sequential reads all go to the
lowest-numbered ready isolate (`src/pool.rs:627`, ties to the lowest slot),
so it fails only when that one isolate lags by more than the 2 ms sleep
plus the time to the first read — rarer than a lagging isolate.

Production effect: a `revalidateTag` (or `revalidatePath`, which is a tag)
is not visible on an affected isolate for up to the lag (observed ≤ 23 ms);
the same clock also makes Next's age and `"use cache"` expiry checks
(`js/03_server.js:1232`) late by the lag. Bounded, but a coherency
violation between isolates of one pool.

## Evidence

Probe (temporary, not committed): a handler returning `Date.now()` and
then `performance.timeOrigin + performance.now()` (wall first, so a delay
between the two reads can only hide a lag), plus four extra
`op_time_origin` samples taken right after bootstrap's.

| Observation | Result |
|---|---|
| Lag per isolate, no contention (2 pools × 10 rounds, 80 isolates) | max −0.18 ms (none lag) |
| Lag per isolate, 6 pools booting at once, 0 or 12 spinner threads, 7 runs (4 424 isolates) | 8 isolates lag 2.1–22.8 ms (1 in 553); on each, all 32 requests show the same lag (min within 1 ms of max, `Date.now()` resolution) |
| bootstrap's `op_time_origin` sample against four immediate resamples, on every lagging isolate (6) | first sample early by the lag (−2.1, −5.9, −6.8, −10.1, −11.7, −22.8 ms); the four resamples agree within 2 µs |
| The test's exact sequence with the isolate clocks forced to lag (`performance.now` shifted after bootstrap) | lag 0 and 1 ms: 0 of 16 reads served; 5 ms: 2 of 16; 10 and 30 ms: 16 of 16 — the observed failure |

So the lag comes from the origin read, and a lag beyond the sleep
produces exactly the recorded failure.

## Fix

`op_nextrs_time_origin` (`src/ops.rs:723`, estimator `src/ops.rs:754`)
replaces deno_web's op through a one-line `web_patches` edit of
`15_performance.js`'s import (`src/web_patches.rs:109`). Each wall read is
taken between two monotonic reads; a bracket of at most 20 µs is used at
once (error ≤ 10 µs), otherwise the narrowest of 16 attempts, at its
midpoint. Same buffer layout as deno_web's op; `performance.now()` is
unchanged. If a deno_web upgrade changes the import line, the edit is
skipped with a warning and the unit test fails first (existing
`web_patches` behaviour). `cache.rs` and the JS handler are unchanged.

**Production behaviour changes** only in that `performance.timeOrigin` is
now correct on every isolate (it was up to tens of ms early on a few).
Cost: three clock reads instead of two, once per isolate boot. Nothing
else reads the op.

## Tests

| Test | Kind | Before the fix | After |
|---|---|---|---|
| `ops::tests::a_preempted_clock_read_does_not_shift_the_time_origin` | unit, scripted clocks | deno_web's order reproduces a 10 ms early origin (asserted) | origin exact within 1 µs with the delay before or after the wall read |
| `ops::tests::without_a_tight_bracket_the_narrowest_one_is_used` | unit, scripted clocks | — | error exactly half the bracket asymmetry (50 µs) |
| `web_patches::tests::the_pinned_performance_source_takes_the_nextrs_time_origin` | unit | — | the pinned source is patched once |
| `cache.rs` `every_isolate_judges_tags_on_the_wall_clock` | integration, 3 pools | passes (rarely lags without contention); guards the invariant | passes |
| `cache.rs` `isolate_clocks_agree_under_cpu_contention` (`#[ignore]`) | stress, 30 rounds × 6 pools, 2 spinners per core | **9 of 10 runs fail: 22 of 6 193 isolates lag 1.1–14.8 ms** | **10 of 10 pass: 0 of 6 060 isolates lag** |

"Before" for the stress test: the same tree with the `PERFORMANCE` patch
removed from `PATCHES`.

## Repeated runs of the original test

| Run | Unpatched | Patched |
|---|---|---|
| `every_isolate_sees_one_cache` alone, 4 processes in parallel, 2 spinners | 300 / 300 pass | — |
| `every_isolate_sees_one_cache` alone, 8 processes in parallel, 4 spinners | 1 200 / 1 200 pass | 1 200 / 1 200 pass |
| whole `cache` binary, 3 processes in parallel, 2 spinners | 150 runs: `every_isolate_sees_one_cache` 0 failures (6 runs failed on another test, below) | 150 runs: 0 failures (7 on the other test) |
| whole `cache` binary, `--test-threads=1`, 2 processes, 2 spinners | — | 50 / 50 pass |
| the test's sequence, 6 pools at once, 2 spinners per core (probe) | **1 failure in 1 200** | 0 in 1 200 |

The original failure needs the lagging isolate to be slot 0 and to lag
by more than the sleep, which is why targeted reruns rarely show it; the
clock stress test is the reliable reproduction.

Separately, `a_write_is_visible_to_every_isolate_before_it_is_persisted`
failed 6 of 150 (unpatched) and 7 of 150 (patched) whole-binary runs under
contention: the "other isolate" read ran in the writer's isolate. Its
second isolate may not be ready yet when the read is dispatched
(`claim` considers only ready isolates, `src/pool.rs:633`). A test
assumption, unrelated to this fix; not changed here.

## Reproduce

```sh
export PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=$PWD/target/re41-cache
cargo +1.98.0 test -p nextrs-runtime --lib -- ops::tests web_patches
cargo +1.98.0 test -p nextrs-runtime --test cache
cargo +1.98.0 test -p nextrs-runtime --test cache isolate_clocks_agree_under_cpu_contention -- --ignored --nocapture
```

For the "before" column, remove `&PERFORMANCE` from `PATCHES`
(`src/web_patches.rs:119`) and rerun the stress test.

## What remains

- **Wall-clock steps.** The store stamps with the wall clock and Next
  judges with a monotonic-based clock. A wall-clock step forward after an
  isolate booted (NTP step, VM pause, host suspend: `CLOCK_MONOTONIC` does
  not advance during suspend) makes that isolate lag by the step until it
  is replaced; a step back delays expiry on all isolates. `next start`
  has the same exposure (FileSystemCache stamps `Date.now()`, the manifest
  judges with `performance`), so it is a residual risk, not a regression.
  Worth a tracking issue.
- The `a_write_is_visible_…` flake above (wait for both isolates to be
  ready before the writer starts).

## Addendum: the second flaky test, fixed

`a_write_is_visible_to_every_isolate_before_it_is_persisted` failed 6/150
whole-binary runs during this investigation, at "another isolate": the
read ran in the writer's isolate because the pool's second isolate was
not ready yet (requests only go to ready isolates). The test now waits
until both isolates report `ready` before the writer starts: a test
synchronization fix, no runtime change. After it: 100/100 whole-binary
runs of `tests/cache.rs`, 0 failures.
