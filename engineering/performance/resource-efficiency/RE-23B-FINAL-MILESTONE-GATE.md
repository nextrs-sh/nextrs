# RE-23B: final milestone gate

**Verdict: BLOCKED.** None of the RE-2/3 conditions that need container
measurements could be closed. The Fedora host's Docker is Docker Desktop
(containers in a VM). It was not reachable from the tools' sandbox, and
the images being compared are not on the host
([RE-23B-LINUX-ENVIRONMENT](RE-23B-LINUX-ENVIRONMENT.md)). No benchmark,
soak or container fault result is reported, and none is inferred.

What did close:

- **CI and compatibility (condition 3)** is closed. The streaming test
  failure is explained ([RE-23B-COMPATIBILITY-AND-CI](RE-23B-COMPATIBILITY-AND-CI.md)).
- **Process-level heap-limit fault testing** produced new findings
  ([RE-23B-CONSTRAINED-RUNTIME-SAFETY](RE-23B-CONSTRAINED-RUNTIME-SAFETY.md)).

Nothing in the code, the defaults, the versions or the packages changed.
No PR was merged.

## Status by section

| Section | Status | Report |
|---|---|---|
| B.1 Dedicated Linux environment | **blocked**: host recorded, does not qualify yet | [RE-23B-LINUX-ENVIRONMENT](RE-23B-LINUX-ENVIRONMENT.md) |
| B.2 Decisive benchmarks (6 variants × 4 configurations × 3 rounds) | **not run** | `RE-23B-CONTROLLED-BENCHMARKS.md`, to be written |
| B.3 Young-generation tuning | **not run** (code-level groundwork below) | `RE-23B-YOUNG-GENERATION-TUNING.md`, to be written |
| B.4 Soaks: 30 min, 2 h, 6 h | **not run** | [RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md) (still pending) |
| B.5 Continuous traffic | **not run** | `RE-23B-CONTINUOUS-TRAFFIC.md`, to be written |
| B.6 Post-trim regrowth at 1 GiB / 2 vCPU | **not run** | `RE-23B-POST-TRIM-REGROWTH.md`, to be written |
| B.7 Constrained runtime safety | **partial**: process level done, container level blocked | [RE-23B-CONSTRAINED-RUNTIME-SAFETY](RE-23B-CONSTRAINED-RUNTIME-SAFETY.md) |
| B.8 CI and compatibility | **complete** | [RE-23B-COMPATIBILITY-AND-CI](RE-23B-COMPATIBILITY-AND-CI.md) |

The four unwritten reports have nothing to report yet. They are not
created as placeholders.

## RE-2/3 conditions

| # | Condition | Status |
|---|---|---|
| 1 | Soaks pass on the Fedora Linux host | open (blocked) |
| 2 | CPU and p99 measured on the Linux host, young generation at V8's size and at 8 MiB | open (blocked) |
| 3 | CI green, including the compatibility suites and the ignored server tests | **closed**: PR #51 head and `main` `c65731b`, all jobs pass |
| 4 | Owner decision on defaults | open; not needed until 1 and 2 close |

## B.3 groundwork (code only, no measurement)

A production young-generation floor needs no V8 flags.
`heap_params` (`nextrs-runtime/src/worker.rs:212-240`) already sizes the
young generation through `v8::CreateParams::set_max_young_generation_size_in_bytes`,
the supported `ResourceConstraints` API. A floor would raise `young`
toward 3 × 8 MiB or 3 × 16 MiB when the cap leaves room. The RE-2/3
8 MiB and 16 MiB experiments used `--v8-flags`, which is acceptable for
experiments only.

Any floor must stay inside the existing heap cap, so that the old
generation absorbs the difference. Total young-generation memory must
count both semi-spaces and the new large-object space, as the brief
requires. Measurement is still required before choosing a size.

## The nine questions

1. **Are the memory improvements reproducible on dedicated Linux
   infrastructure?** Not answered: nothing has been measured on it. The
   only evidence is still RE-2/3's, from a shared Mac Docker Desktop VM.
2. **Does NextRS remain stable at 256 MiB over six hours?** Not
   answered. The longest run is still 25 minutes.
3. **Does continuous traffic cause retention or unexplained growth?** Not
   answered. Structurally, idle reclamation needs a quiet window, so
   under continuous traffic only pressure reclamation (from 90 % of the
   limit) and V8's own GC act. That is the case RE-23B.5 must measure.
4. **Can young-generation tuning recover the CPU regression?** Not
   answered on Linux. In RE-2/3 (one round each), 8 MiB recovered about
   two thirds of the closed-loop CPU, and 16 MiB all of it for +16 MiB of
   peak memory. If the data justify it, a supported API is available
   (above).
5. **Is the heap budgeting policy conservative enough?**
   - For gradual heap growth: yes, as the RE-2/3 data show.
   - For large single allocations and for native memory: no.
     - Measured here, at a 64 MiB heap limit, one 192 MiB allocation
       aborts the whole process.
     - Recoverable large allocations overshoot the limit by up to 6× the
       limit before the isolate terminates.
     - 512 MiB of `ArrayBuffer`s pass a 64 MiB limit unchecked.
   - Inside 256 MiB, these become OOM kills. The heap budget is not a
     process memory guarantee.
6. **Are the low-memory and balanced profiles suitable for continued
   testing?** Yes, as opt-in profiles, for the B.2–B.6 runs. Nothing here
   argues against them. Nothing here qualifies them as defaults either.
7. **Is research into an alternative JavaScript engine justified?** No.
   The failure modes found here belong to the embedder and its policy:
   accounting for native memory, and admitting requests against memory
   state. Another engine would have them too. RE-2/3's attribution of
   the memory floor stands.
8. **Next milestone: resource-aware admission control, or CPU and native
   allocation?** Provisionally, **resource-aware admission control**. The
   remaining failures at 256 MiB end the process, and reclamation cannot
   prevent them. The CPU cost has a small, known fix (the young-generation
   floor), which can be validated alongside. Confirm this choice after
   B.2–B.7 run.
9. **Are owner decisions required before advancement?** Yes, see below.

## Owner decisions required

1. **Container engine on the Fedora host.** Install the native Docker
   Engine or Podman (cgroup v2), and stop using Docker Desktop's VM for
   measured runs. Alternatively, explicitly accept the Docker Desktop VM
   as the "Linux host". That would leave the results preliminary, just
   as the Mac results are.
2. **How the runs are driven.** Either allow these tools to run host
   commands (`flatpak-spawn --host`, which is currently denied), or run
   `benchmarks/resource/run.mjs` from a host terminal and hand the
   result directories back for analysis.
3. **Images.** Move `velasia-s1.tar.gz` from the Mac and check the hashes
   (`base` `af994c15…`, `re2c` `052d741d…`), or approve a rebuild on
   Linux, with the binary differences documented.
4. **Exclusive machine time.** About 60 hours of sequential runs, with
   the desktop session idle:
   - B.2: about 24 h;
   - B.4: 22 h (four 30-minute runs, one 2-hour run, three 6-hour runs);
   - B.3: about 6 h;
   - B.5: about 5 h;
   - B.6: about 2 h.
5. **Test-only fix [#52](https://github.com/nextrs-sh/nextrs/issues/52).**
   Approve removing the stall limit from the two body tests.
6. **This branch.** `feat/re-23b-linux-validation` holds these reports.
   It is committed locally only: not pushed, and no PR opened.

Defaults stay unchanged until conditions 1 and 2 close.
