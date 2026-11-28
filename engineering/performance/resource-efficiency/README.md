# Resource efficiency (RE-1 to RE-3)

What NextRS needs in memory and CPU to run a real Next.js application in
a small container, why rc.3 did not fit in 256 MiB, and what was changed
so that it does. Application: Velasia Airways, unmodified.

**RE-2/3 gate: PASS WITH CONDITIONS** ([RE-23-MILESTONE-GATE](RE-23-MILESTONE-GATE.md)).
V8 stays. The new options are opt-in, and the defaults do not change
until the soaks pass and the owner decides.

Every number here is a local measurement on one shared host (Intel Mac,
Docker Desktop VM, 8 vCPU / 3.82 GiB, cgroup v2) from runs of 10–25
minutes: preliminary, not a published performance claim. The soak tests
(30 min, 2 h, 6 h) run last, on a dedicated Fedora Linux machine
([RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md)).

| Report | Contents |
|---|---|
| [RE-1-RESOURCE-BASELINE](RE-1-RESOURCE-BASELINE.md) | rc.3 against Next.js standalone across the container matrix; the 256 MiB OOM |
| [RE-1B-BASELINE-VERIFICATION](RE-1B-BASELINE-VERIFICATION.md) | Is the baseline repeatable? Binary hashes, environment, noise levels |
| [RE-2-ALLOCATOR-RETENTION-ANALYSIS](RE-2-ALLOCATOR-RETENTION-ANALYSIS.md) | How much after-load memory glibc retains, and how much a trim returns |
| [RE-2-ALLOCATOR-COMPARISON](RE-2-ALLOCATOR-COMPARISON.md) | glibc (default, 2 arenas, trimmed), jemalloc, mimalloc |
| [RE-2-NATIVE-MEMORY-OPTIMIZATION](RE-2-NATIVE-MEMORY-OPTIMIZATION.md) | The reclamation changes and their measured effect |
| [RE-3-CGROUP-RESOURCE-DETECTION](RE-3-CGROUP-RESOURCE-DETECTION.md) | Finding the container's memory and CPU limits (cgroup v2 and v1) |
| [RE-3-V8-HEAP-BUDGETING](RE-3-V8-HEAP-BUDGETING.md) | Why V8 outgrew the container; the heap budget behind `--js-heap-limit auto` |
| [RE-3-V8-MEMORY-OPTIMIZATION](RE-3-V8-MEMORY-OPTIMIZATION.md) | V8 controls in the pinned engine; the attribution ladder; heap exhaustion |
| [RE-3-GC-AND-CPU-TRADEOFFS](RE-3-GC-AND-CPU-TRADEOFFS.md) | GC frequency and pauses, CPU per request, the young generation, idle CPU per thread |
| [RE-23-COMBINED-BENCHMARKS](RE-23-COMBINED-BENCHMARKS.md) | Single-factor attribution, key and wide matrices, load and idle cycles, final binary |
| [RE-23-CONSTRAINED-CONTAINER-VALIDATION](RE-23-CONSTRAINED-CONTAINER-VALIDATION.md) | What runs in 128 MiB to 1 GiB; sustainable rate at 256 MiB; recommended settings |
| [RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md) | Soak plan and pass criteria (results pending, Linux host) |
| [RE-23-INDEPENDENT-REVIEW](RE-23-INDEPENDENT-REVIEW.md) | Code review findings and their fixes |
| [RE-23-MILESTONE-GATE](RE-23-MILESTONE-GATE.md) | The ten gate questions, the targets, the verdict and its conditions |

## Data and reproduction

[`results/`](results/) has one directory per stage, holding:
- `SUMMARY.md` and `summary.json`: every run, with medians and ranges;
- `meta-*.json`: the command, host and phase plan;
- `stage-*.txt`: the hashes of every binary and image used;
- `run.log`: the run's progress log;
- the GC-trace and idle-thread summaries.

Per-run sample files (memory every second, 2–18 MiB per stage) are kept
outside the repository.

The drivers, variants and runbook (including a soak on a Linux host) are
in [`benchmarks/resource/`](../../../benchmarks/resource/README.md).

| Stage | Workload | Configurations × rounds |
|---|---|---|
| `baseline-r1` | `full` | RE-1: 128 MiB / 0.5, 256 MiB / 0.5 and 1, 512 MiB / 1, 1 GiB / 2 vCPU, 1 round |
| `re23-screening` | `memory` | 256 MiB and 512 MiB / 1 vCPU, 13 variants, 1 round |
| `re23-gctrace` | `memory` with `--trace-gc` | 256 MiB and 512 MiB / 1 vCPU, 1 round |
| `re23-matrix` | `full` | 256 MiB / 0.5 and 1 vCPU, 512 MiB / 1 vCPU, 3 rounds |
| `re23-wide` | `full` | 128 MiB / 0.5, 512 MiB / 0.5, 1 GiB / 1 and 2 vCPU, 1 round |
| `re23-envelope` | `envelope` | 256 MiB / 0.5 and 1 vCPU |
| `re23-cycles` | `cycles` | 256 MiB and 512 MiB / 1 vCPU |
| `re23-confirm` | `full` | final binary, 256 MiB and 512 MiB / 1 vCPU, 2 rounds |
| `re23-confirm-young` | `full` | young generation 8 and 16 MiB, 512 MiB / 1 vCPU and 1 GiB / 2 vCPU |
| `re23-idle-threads` | none (health checks only) | 512 MiB / 1 vCPU, 300 s |
