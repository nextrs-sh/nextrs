# RE-2/3: constrained container validation

Status: complete for 128 MiB to 1 GiB; preliminary (one Docker Desktop VM
on a shared Mac). Soak tests run last (30 min, 2 h and 6 h, on a
dedicated Fedora Linux machine,
[RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md)); until then, "runs
in N MiB" means for the 10–25 minutes of these workloads.

Candidate: `--js-heap-limit auto --memory-reclaim-idle 10
--memory-pressure-reclaim 90` (the `low-memory` mechanisms with the
default compression cache), one isolate. Data:
[RE-23-COMBINED-BENCHMARKS](RE-23-COMBINED-BENCHMARKS.md), stages
`re23-matrix`, `re23-wide`, `re23-envelope`, `re23-cycles`,
`re23-confirm` (final binary).

Margin is the container limit minus the highest peak. At 256 MiB the
container's `memory.current` peak includes up to 48–87 MiB of page cache
that the kernel reclaims before an OOM kill, so the margin is given
against anonymous memory (the highest anonymous peak was 163 MiB at 1 vCPU and 160 MiB at 0.5 vCPU).

## Does it run? (Velasia's `full` workload unless stated)

| Container | `node` | `base` (rc.3) | candidate | Candidate's margin (limit − peak) |
|---|---|---|---|---:|
| 128 MiB / 0.5 vCPU | **OOM** | **OOM** | runs (SSR CPU-bound) | 13 MiB |
| 256 MiB / 0.5 vCPU | runs, CPU-bound | **OOM 3 of 4** (+ envelope 2 of 2) | runs, 3 of 3 + envelope | 96 MiB (anon) |
| 256 MiB / 1 vCPU | runs | **OOM 6 of 7** (+ envelope, cycles) | runs, 5 of 5 + envelope + cycles | 93 MiB (anon) |
| 512 MiB / 0.5 vCPU | runs | runs | runs | 352 MiB |
| 512 MiB / 1 vCPU | runs | runs | runs, 3 of 3 + cycles | 330 MiB |
| 1 GiB / 1 vCPU | runs | runs | runs | 793 MiB |
| 1 GiB / 2 vCPU | runs | runs | runs | 778 MiB |

## Sustainable rate (`envelope`)

Mixed traffic at fixed open-loop rates, 40 s each; a rate counts as
sustained with no failed request and ≥ 95 % of it achieved.

| Container | Variant | Highest rate sustained | Next step | Peak MiB | p99 at the highest sustained rate |
|---|---|---:|---|---:|---:|
| 256 MiB / 1 vCPU | `node` | 60/s | **OOM** at 80/s | 242.7 | 821 ms |
| | `base` | 80/s | **OOM** at 100/s (3765 failed) | 251.4 | 88 ms |
| | candidate | **120/s** (top of the ladder) | then 124/s closed loop, no failures | **164.8** | 751 ms (100/s: 222 ms) |
| 256 MiB / 0.5 vCPU | `node` | 15/s | 28/s of 30 achieved | 216.9 | 7.2 s |
| | `base` | 45/s | **OOM** at 60/s | 255.0 | 1.9 s |
| | candidate | 45/s | 44/s of 60 achieved (CPU-bound), no failures | 254.2 (anon 159.8) | 944 ms |

At 256 MiB / 1 vCPU the candidate's limit is CPU, not memory: it ran the
ladder to 120/s at 0.98 cores with 91 MiB of room; the baseline ran out
of memory at 100/s with CPU to spare (0.64 cores at 80/s).

At 256 MiB / 0.5 vCPU (round 2; round 1 was invalidated by host load,
see the combined report) the candidate and `base` sustain the same rate,
45/s. One step higher, `base` is OOM-killed (both rounds) and the
candidate saturates its half core: 44 of 60 requests/s served, none
failed, queueing pushes p50 to 1.6 s and p99 to 17 s, anonymous memory
peaks at 160 MiB. Below saturation the candidate's p99 was higher than
`base`'s in this round (269–944 ms against 158–338 ms at 15–45/s, CPU
per request +8–21 %); `base`'s own p99 at 45/s ranged 338–1897 ms across
its two rounds, so tails at 0.5 vCPU are not settled on this host.

## What sets the floor

- **128 MiB** works with the heap clamped to 64 MiB (the budget warns:
  128 − 64 − 32 − 48 leaves nothing, below the 64 MiB floor). The peak was
  114.5 MiB and pressure reclamation ran once. It is not a recommended
  size: 13 MiB of margin, and one SSR search at a time saturates 0.5 vCPU
  (2.2 of 5/s, p50 19 s). `--profile low-memory` (8 MiB compression
  cache) is clamped the same way (128 − 120 reserved) but holds 24 MiB
  less in its cache; not measured at 128 MiB.
- **256 MiB / 1 vCPU** is the smallest configuration with both memory and
  CPU margin for Velasia's mixed workload up to ~100 requests/s.
- **256 MiB / 0.5 vCPU** is CPU-limited at ~45 requests/s; overload
  queues instead of failing.
- Live data after full collections, 48–51 MiB, plus 37–42 MiB of binary
  and library pages and ~25 MiB of native live memory, is the floor below
  which no setting goes (73–80 MiB anonymous after a trim).

## Recommended configuration for small containers

For one isolate in 256–512 MiB: `--profile low-memory`, or
`--js-heap-limit auto --memory-reclaim-idle 10 --memory-pressure-reclaim 90`
to keep the 32 MiB compression cache. At 1 GiB and above the heap
budget does not bind; idle reclamation still halves after-load memory.
These remain opt-in: the defaults are unchanged until the soaks pass and
the owner decides (see the [gate](RE-23-MILESTONE-GATE.md)).

## Not covered

More than one isolate (the budget divides the container; `--isolates 2`
at 256 MiB clamps each heap to 64 MiB and warns, not measured under
load); Kubernetes `memory.high` throttling (detected, not load-tested);
in-place resize; other applications; ARM.
