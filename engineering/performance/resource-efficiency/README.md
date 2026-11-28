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

### RE-23B: dedicated Linux validation

**RE-23B gate: BLOCKED** ([RE-23B-FINAL-MILESTONE-GATE](RE-23B-FINAL-MILESTONE-GATE.md)).
No container benchmark or soak has run: the Fedora host's Docker is
Docker Desktop (containers in a VM), the tools' sandbox could not reach
the host, and the images were not on it. CI and compatibility are
closed. Process-level fault tests ran.

| Report | Contents |
|---|---|
| [RE-23B-LINUX-ENVIRONMENT](RE-23B-LINUX-ENVIRONMENT.md) | The Fedora host, why it does not yet qualify, what unblocks it |
| [RE-23B-COMPATIBILITY-AND-CI](RE-23B-COMPATIBILITY-AND-CI.md) | CI on PR #51 and `main`; local gate; the streaming-test failure explained ([#52](https://github.com/nextrs-sh/nextrs/issues/52)) |
| [RE-23B-CONSTRAINED-RUNTIME-SAFETY](RE-23B-CONSTRAINED-RUNTIME-SAFETY.md) | Heap-limit faults: which allocations are recoverable and which abort the process; native memory beyond the heap budget |
| [RE-23B-FINAL-MILESTONE-GATE](RE-23B-FINAL-MILESTONE-GATE.md) | Status by section, the nine questions, owner decisions |
| `RE-23B-CONTROLLED-BENCHMARKS`, `-YOUNG-GENERATION-TUNING`, `-CONTINUOUS-TRAFFIC`, `-POST-TRIM-REGROWTH` | not written: blocked, nothing measured |

### RE-4: resource-aware admission and memory exhaustion resilience

**RE-4 gate: see [RE-4-MILESTONE-GATE](RE-4-MILESTONE-GATE.md)** (separate
verdicts for implementation, compatibility, resource safety, benchmark
validation and release). Every new control is opt-in. No default or
profile changed. No container benchmark ran: the Linux host is still
blocked, and the [runbook](RE-4-LINUX-BENCHMARK-RUNBOOK.md) is for the
owner.

| Report | Contents |
|---|---|
| [RE-4-RESOURCE-ACCOUNTING](RE-4-RESOURCE-ACCOUNTING.md) | What holds memory, which numbers overlap, what NextRS can and cannot enforce (written before the implementation) |
| [RE-4-ADMISSION-CONTROL-DESIGN](RE-4-ADMISSION-CONTROL-DESIGN.md) | `--memory-admission`: states, reservations, hysteresis, overload responses, failure of measurements |
| [RE-4-NATIVE-BUFFER-BUDGETS](RE-4-NATIVE-BUFFER-BUDGETS.md) | The 31-site native buffer audit; `--native-buffer-budget` for whole-file reads and proxy bodies |
| [RE-4-EXTERNAL-MEMORY-ANALYSIS](RE-4-EXTERNAL-MEMORY-ANALYSIS.md) | How ArrayBuffers are allocated; `--js-external-memory-limit` through a budgeted `ArrayBuffer::Allocator`; hard vs best-effort limits |
| [RE-4-V8-HEAP-OVERSHOOT](RE-4-V8-HEAP-OVERSHOOT.md) | Why one allocation reaches 6× the heap limit (repeated grants); the fatal boundary; child-process regression tests |
| [RE-4-FAULT-CONTAINMENT-ADR](RE-4-FAULT-CONTAINMENT-ADR.md) | In-process isolates vs worker processes: decision to stay in-process for now |
| [RE-4-YOUNG-GENERATION-EXPERIMENT](RE-4-YOUNG-GENERATION-EXPERIMENT.md) | `--js-young-semi-space`: a cap-preserving young-generation floor, and the experiment plan |
| [RE-4-CORRECTNESS-AND-FAULT-TESTS](RE-4-CORRECTNESS-AND-FAULT-TESTS.md) | Every local test, what it covers, what is blocked on a container engine |
| [RE-4-LINUX-BENCHMARK-RUNBOOK](RE-4-LINUX-BENCHMARK-RUNBOOK.md) | Native Docker Engine on Fedora, reproducible rebuilds, the decisive campaign, fault tests, soaks |
| [RE-4-INDEPENDENT-REVIEW](RE-4-INDEPENDENT-REVIEW.md) | Review findings and their resolution |
| [RE-4-MILESTONE-GATE](RE-4-MILESTONE-GATE.md) | The five verdicts and the eight questions |

### RE-4.1: resource control correctness and memory safety hardening

**RE-4.1 gate: see [RE-4.1-MILESTONE-GATE](RE-4.1-MILESTONE-GATE.md).**
Every control stays opt-in; no default or profile changed. No container
benchmark ran.

| Report | Contents |
|---|---|
| [RE-4.1-NATIVE-BUFFER-HARDENING](RE-4.1-NATIVE-BUFFER-HARDENING.md) | Large static files streamed in budgeted chunks; reservations owned by the bytes; 413 for proxy bodies above the budget; the budget is now the bound |
| [RE-4.1-ADMISSION-ACCOUNTING](RE-4.1-ADMISSION-ACCOUNTING.md) | Commitment lifecycle; sampled vs committed accounting under delayed allocation (model and real isolates); the emergency state; reclaim coordination |
| [RE-4.1-NESTED-REQUEST-COMPATIBILITY](RE-4.1-NESTED-REQUEST-COMPATIBILITY.md) | Server Actions with in-process nested requests under pressure, and proxy-read bodies, end to end on real builds; the slot-exhaustion stall |
| [RE-4.1-EXTERNAL-MEMORY-COVERAGE](RE-4.1-EXTERNAL-MEMORY-COVERAGE.md) | Which allocation paths the ArrayBuffer budget enforces, which it does not, and why |
| [RE-4.1-CACHE-CORRECTNESS](RE-4.1-CACHE-CORRECTNESS.md) | The intermittent `every_isolate_sees_one_cache` failure: a time-origin race, fixed |
| [RE-4.1-RESOURCE-BUDGET-COORDINATION](RE-4.1-RESOURCE-BUDGET-COORDINATION.md) | What each limit bounds, overlaps, a coordinated-profile proposal, unvalidated 256/512 MiB starting points |
| [RE-4.1-BENCHMARK-READINESS](RE-4.1-BENCHMARK-READINESS.md) | Runbook corrections, harness validation and dry runs, the fault workloads, preflight: what was checked statically and what remains for the owner |
| [RE-4.1-INDEPENDENT-REVIEW](RE-4.1-INDEPENDENT-REVIEW.md) | Review findings and their resolution |
| [RE-4.1-MILESTONE-GATE](RE-4.1-MILESTONE-GATE.md) | The five verdicts and the seven questions |

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
| `re23b-ci` | local gate, streaming-test reproduction and sweeps | Fedora host, no container, `79e4fe5` and `c65731b` |
| `re23b-safety` | heap-limit and external-memory fault cases, one process each | Fedora host, no container, 64 and 128 MiB heap limits |
| `re4-heap-overshoot` | the same heap-limit cases with near-heap-limit instrumentation (callback calls, limits, unwind and replacement time) | Fedora host, no container, 64 and 128 MiB heap limits |
