# RE-2/3: milestone gate

**Verdict: PASS WITH CONDITIONS.** Keep V8. The new mechanisms stay
opt-in (`--js-heap-limit auto`, `--memory-reclaim-idle`,
`--memory-pressure-reclaim`, the `balanced` and `low-memory` profiles)
until the conditions below are met and the owner decides on defaults.

All results are preliminary: one Docker Desktop VM (8 vCPU, 3.82 GiB) on
a shared Intel Mac, runs of 10–25 minutes. The soak tests (30 min, 2 h,
6 h) run last, on a dedicated Fedora Linux machine, and are not part of
this verdict ([RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md)).

## What was measured

| Binary | sha256 | Used for |
|---|---|---|
| `base`, rc.3 | `af994c15e3c6d6a438cfece9e753cf651598f31b050dea40b693bc5ed06548a2` | the baseline in every stage |
| `re2b` | `be739569be1451d973933980c69351e98fb4f9e7e50da82b71c409e304f33559` | single-factor, matrix, wide, envelope, cycles, GC trace |
| `re2c`, final | `052d741d4fc4659201da20679a9505e79e100f05dc51e9b5d1cdd25017191902` | confirmation, young generation, idle CPU |

`re2c` is this pull request's code, built before a formatting-only
change. Application: Velasia Airways `d04127a`, unmodified, one build for
both runtimes. Node.js comparison: Next.js 16.4.6 standalone on
Node.js 24.18.0.

The candidate measured throughout is `--js-heap-limit auto
--memory-reclaim-idle 10 --memory-pressure-reclaim 90`, one isolate: the
`low-memory` profile's mechanisms with the default 32 MiB compression
cache.

## The ten questions

**1. How much memory retention was attributable to glibc?**
63–88 MiB of allocator free memory 300 s after load in every `base` run,
40–55 % of the process's anonymous memory. V8's heap was back to
51–54 MiB in the same runs
([RE-2-ALLOCATOR-RETENTION-ANALYSIS](RE-2-ALLOCATOR-RETENTION-ANALYSIS.md)).

**2. How much of it was reclaimable?** Nearly all of it. One
`malloc_trim(0)` returned 74–76 MiB in 0.4 s, and every run at every size
ended at 73–80 MiB anonymous after the trim. That floor is ~51 MiB of
V8 heap plus ~25 MiB of live native memory; there is no large
unreturnable remainder. After load at 512 MiB: 76–92 MiB anonymous for
the candidate against 155–156 for `base`.

**3. Which allocator strategy was best?** The system glibc allocator,
default arenas, trimmed once per idle period, with pressure reclamation
as a safety net behind the heap budget. Two arenas retained more
(+15 MiB after load); jemalloc 5.3 and mimalloc 2.0 raised idle memory
by 11–33 MiB and peaks by ~100 MiB
([RE-2-ALLOCATOR-COMPARISON](RE-2-ALLOCATOR-COMPARISON.md)).

**4. Does container-aware V8 heap sizing prevent the OOM failures?** Yes.
`base` was OOM-killed in 9 of 11 runs of the `full` workload at 256 MiB
and in every `envelope` and `cycles` run there. The heap budget alone
survived both 256 MiB configurations; idle trimming alone did not
(single-factor attribution,
[RE-23-COMBINED-BENCHMARKS](RE-23-COMBINED-BENCHMARKS.md)). The
candidate was never OOM-killed: 0 of 13 runs at 256 MiB across all
stages (2 of them with the final binary), and it completed the 128 MiB
run in which `base` and Node.js were killed during the cold pass.

**5. Can NextRS sustain the Velasia workload in 256 MiB?** Yes, for the
measured workloads and durations, at both CPU sizes. The highest
anonymous peak was 163 MiB (93 MiB of margin). Whether that holds for
hours is the soaks' question.

**6. What is the maximum sustainable workload at that budget?**

| Container | Node.js | `base` | candidate |
|---|---:|---:|---:|
| 256 MiB / 1 vCPU | 60/s, OOM at 80 | 80/s, OOM at 100 | **120/s** (top of the ladder, CPU-bound at 0.98 cores), 124/s closed loop |
| 256 MiB / 0.5 vCPU | 15/s | 45/s, OOM at 60 | **45/s**; at 60/s it serves 44/s with no failures |

Mixed Velasia traffic, 40 s per rate, sustained = no failures and ≥ 95 %
of the offered rate achieved
([RE-23-CONSTRAINED-CONTAINER-VALIDATION](RE-23-CONSTRAINED-CONTAINER-VALIDATION.md)).
At 256 MiB the candidate is limited by CPU, `base` by memory.

**7. Did GC tuning increase CPU consumption or latency?** CPU, slightly;
latency, mostly not.

- GC: 7–8× as many scavenges and +40–60 % main-thread GC time, with
  shorter pauses: the longest dropped from 262 ms to 51–191 ms
  ([RE-3-GC-AND-CPU-TRADEOFFS](RE-3-GC-AND-CPU-TRADEOFFS.md)).
- CPU per request, final binary, where `base` has room (512 MiB): +4–11 %
  at fixed rates, +3 % in the closed loop (throughput −3 %). Single
  rounds of the wider matrix ranged from +3 % to +30 %. Still 35–55 %
  below Node.js.
- The cause is the smaller, container-sized young generation. A 16 MiB
  semi-space at 512 MiB brings CPU per request back to `base`'s for
  +16 MiB of peak. That is V8's own sizing, and already rc.3's behaviour
  with `--js-heap-limit auto` at 512 MiB and above.
- Latency: p50 is unchanged.
  - At 1 vCPU, p99 at fixed rates was equal or lower, except SSR at
    256 MiB.
  - At 0.5 vCPU, p99 was higher in the one valid envelope round
    (0.27–0.94 s against 0.16–0.34 s). `base`'s own p99 varied from
    0.34 to 1.9 s between rounds.
- Not settled on this host: the ≤ 5 % p99 target needs controlled
  hardware.

**8. Did any optimization regress compatibility?** No regression found.
- Every surviving run passed the body-checked correctness pass (SSR,
  route handlers, a Server Action).
- Locally: format and clippy (`-D warnings`) are clean, and 447 of 448
  tests pass, including every new test.
- The one failure, `large_streamed_body_arrives_intact_in_many_reads`,
  also fails on unmodified `main` (`79e4fe5`) in the same local builder,
  while `main`'s CI passes it. So it is environment-specific and
  predates this branch.
- The Next.js compatibility suites and the ignored server tests run in
  CI on the pull request.

**9. Is adaptive isolate management still necessary?** Not for the
resource objective. One isolate with a budgeted heap runs Velasia in
256 MiB. The budget divides the container between isolates (two
isolates at 256 MiB clamp each heap to 64 MiB, with a warning). That
case was not load-tested. Adaptive scaling is a throughput question, not
a memory fix, and stays out of scope.

**10. Is research into another JavaScript engine justified?** Not by this
evidence.
- The OOMs were a sizing problem: V8 sized itself from the host, not the
  container. Sizing fixes them.
- The remaining floor is attributed: 48–51 MiB of live JavaScript data
  after full collections (the application's own objects and code, which
  any engine holds in some form), about 25 MiB of live native memory, and
  37–42 MiB of binary and library pages.
- Idle anonymous memory is 20–21 MiB, against 46–48 MiB for Node.js.
- No JavaScriptCore or QuickJS-NG work is recommended.

## Engineering targets (milestone §14, against RE-1)

| Target | Result (candidate) | Status |
|---|---|---|
| Idle container memory ≤ 40 MiB | 20–21 MiB anonymous; `memory.current` 24–31 MiB, up to 65 when startup page cache stays charged (reclaimable) | met |
| Peak at 512 MiB / 1 vCPU ≈ 220 MiB or lower | 175–196 MiB (`base` 262–303) | met |
| After load at 512 MiB ≈ 100 MiB or lower | 97–99 MiB container (final binary), 76–92 anonymous, 300 s after load (`base` 164–170 container, 155–156 anonymous) | met |
| Retained allocator free memory ≈ 30 MiB or lower | 29–38 MiB at 300 s (`base` 63–88); most of it non-resident after the trim | met, approximately |
| 256 MiB / 1 vCPU stable within a documented envelope | no OOM in any run; to 120/s | met for runs of up to 25 min |
| 256 MiB / 0.5 vCPU stable within a documented envelope | no OOM in any run; to 45/s, overload queues instead of failing | met for runs of up to 25 min |
| CPU efficiency: maintain or improve | +3–11 % CPU per request at ≥ 512 MiB; better than `base` at 256 MiB where `base` runs at its limit | **not met at ≥ 512 MiB** (small, attributed, adjustable) |
| Throughput: no material regression | −3 % closed loop at 512 MiB (final binary); higher at 256 MiB where `base` dies | met within this host's noise |
| p99 ≤ 5 % regression on controlled infrastructure | not measurable here | **open**: Linux host |
| Compatibility: zero regression | none found | CI on the pull request |

## Conditions

1. **Soaks pass on the Fedora Linux host:** 30 min (`base` and candidate
   at 256 MiB / 1 vCPU and 512 MiB / 1 vCPU), 2 h (candidate, 256 MiB),
   6 h (candidate, `base`, Node.js, 256 MiB). The pass criteria are in
   the soak report. The soaks also settle the open regrowth question: at
   2 vCPU, memory rose after the trim with only probes arriving (79 →
   101 MiB in the wide matrix, 103 → 139 MiB in the young-generation run;
   still below `base`'s 162–168).
2. **CPU and p99 on the Linux host:** the first uncontended measurement
   of the +3–11 % CPU cost and of the p99 target, with the young
   generation at V8's size and at 8 MiB.
3. **CI on the pull request is green**, including the compatibility
   suites and the ignored server tests.
4. **Owner decision on defaults.** Nothing in this pull request changes a
   default. The `low-memory` profile no longer caps glibc arenas (the cap
   made retention worse).

## Not covered

More than one isolate under load; Kubernetes `memory.high` throttling
(detected, not load-tested); in-place container resize; other
applications; ARM; musl. The 24-hour validation stays in RE-8.

## Follow-ups (not in this pull request)

- A young-generation floor when the budget leaves room. 8 MiB recovered
  about two thirds of the closed-loop CPU at no measurable memory cost,
  in one round.
- An event-driven isolate heartbeat (milestone §15). NextRS's idle CPU is
  9.3 millicores in the process: 6.6–6.9 on the isolate's 25 ms
  heartbeat and 2.0 on the supervisor's 50 ms watch loop. An
  event-driven heartbeat could save ~7 millicores per idle isolate, but
  it changes the stall detector's invariants. This change was not made.
- Buffer reuse in the whole-file and cache-copy paths that produce the
  glibc retention.
