# RE-2/3: soak tests

**Status: pending. The soak tests run last, after this milestone's
pull request, all on a dedicated Linux machine (Fedora, x86_64):** 30
minutes, 2 hours, then 6 hours. None runs on the shared Mac used for the
rest of the milestone. No soak
result is reported or relied on yet; this report holds the plan and will
hold the results.

Until the soaks are done, every after-load figure in the other reports
comes from runs of 10–25 minutes. They show that retention is returned
after one load, not that memory stays bounded over hours.

## Questions

1. Does warmed-up memory settle within a range, or keep climbing (a slow
   leak, allocator fragmentation, cache growth)?
2. Does the idle reclamation keep firing once per idle period, and does
   memory after each reclamation return to the same floor (73–80 MiB
   anonymous in the short runs)?
3. Does the post-trim regrowth seen once at 2 vCPU (79 → 101 MiB with
   only probes, [RE-2-ALLOCATOR-RETENTION-ANALYSIS](RE-2-ALLOCATOR-RETENTION-ANALYSIS.md))
   accumulate across cycles?
4. Do any isolate restarts, heap-limit terminations, stalls or OOM kills
   happen over hours at 256 MiB?
5. Is CPU per request stable over time (no growing GC cost)?

## Plan

Workload: `run.mjs profile --workload soak --soak-minutes N`, repeating
6-minute blocks of mixed traffic at 30 requests/s (210 s), 30 s
closed-loop, Server Actions at 5/s (30 s), 90 s idle (long enough for the
10 s idle reclamation). Summary: `soak-summary.mjs`, per block: peak,
memory at the end of each idle gap, reclamations, heap after full
collections, failures, CPU per request.

| Soak | Where | Variants | Configuration | Pass criteria |
|---|---|---|---|---|
| 30 min | Fedora Linux host | `base`, final candidate | 256 MiB / 1 vCPU, 512 MiB / 1 vCPU | no OOM or failed requests (candidate); idle floors within ±10 MiB of the first block |
| 2 h | Fedora Linux host | final candidate | 256 MiB / 1 vCPU | as above; no upward trend in idle floors (slope under 2 MiB/h) |
| 6 h | Fedora Linux host, x86_64, cgroup v2, nothing else running | final candidate, `base`, Node.js | 256 MiB / 1 vCPU | as above, plus CPU per request within ±10 % between the first and last hour |

The Linux runs use the same images (moved with `docker save` / `docker
load`, hashes checked) and the runbook in `benchmarks/resource/README.md`
("A soak on a Linux host"). The Linux host is the first non-shared
machine in this milestone, so it also gives the first uncontended CPU
figures.

## Results

*Pending.*
