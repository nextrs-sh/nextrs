# RE-1B: baseline verification

Status: complete for the measured configurations. Preliminary: one Docker
Desktop VM on a shared Mac (see [Environment](#environment)). Soak tests
run last (30 min, 2 h and 6 h, on a dedicated
Fedora Linux machine) and
are reported separately in [RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md).

## Question

RE-1 measured the rc.3 baseline once. Before attributing any improvement,
are its findings repeatable: does the baseline really fail at 256 MiB,
and how stable are its memory and CPU figures from run to run?

## Binaries and inputs (identical in every run of this milestone)

| Item | Identity |
|---|---|
| Baseline binary (`base`) | `nextrs 0.1.0-rc.3` serving code, sha256 `af994c15e3c6d6a438cfece9e753cf651598f31b050dea40b693bc5ed06548a2` (the RE-1 and Series 1 `optA` binary) |
| Shipped-`auto` binary (`re1`, screening only) | `926af1f6b077f0636d4e857f3c9127369835b80ed743b00486f377b7adb8edb3` (`main` at PR #50) |
| Candidate binaries | `re2` `2d70295d…53499` (screening), `re2b` `be739569…` (matrix, wide, envelope, cycles), final `re2c` `052d741d…91902` (confirmation, young generation, idle CPU; [gate](RE-23-MILESTONE-GATE.md)) |
| Load generator | `nextrs-loadgen` `166b0d705c4204964170965ed5c7c16443c1f5e0a846cd56e44a765d527f02ab` |
| Node.js image (`velasia-node:s1`) | `sha256:4cac18800cc879e50f1873f815a00a93209a9ca0f16e3186e9955e429962f526`, Node.js 24.18.0, Next.js 16.4.6 standalone |
| Velasia API image | `sha256:08f6ae32937d2bf64c976519c94af06eac3b237096baaf19bec209e59e7c5639` |
| Application | Velasia Airways `d04127a`, one build for both runtimes (BUILD_ID `gvWJrvYSM71x059u5PMn2`), not modified |

Each stage directory records the hashes it ran with (`stage-*.txt`) and
the full command, host and phase plan (`meta-*.json`).

## Environment

| Item | Value |
|---|---|
| Host | shared Intel Mac (Darwin 24.6.0), other sessions' work running; 1-minute host load 1.6–125 during the series |
| VM | Docker Desktop 29.7.2, LinuxKit 7.0.12, 8 vCPU, 3.82 GiB, cgroup v2 |
| Placement | storefront on VM CPUs 2–3 (`--cpus` per configuration), load generator and probe on 4–5, API on 6; builds pinned to 0, 1, 7 and never run during a measured stage |
| Contention record | host load, VM load, VM busy and steal ticks before and after every phase (`contention` in each run's JSON). The VM reported 0 % steal in every phase of every run, so host contention is only visible as host load and CPU-throttling time |

## Results

### Outcome at each size (all rounds of RE-1 and this milestone)

`base` = the rc.3 binary with no options; `node` = Next.js standalone.

| Configuration | `base` | `node` |
|---|---|---|
| 128 MiB / 0.5 vCPU | **OOM-killed 2 of 2** (RE-1, wide), in the correctness or cold pass | **OOM-killed 2 of 2** |
| 256 MiB / 0.5 vCPU | **OOM-killed 3 of 4** (RE-1 during fixed-rate SSR; matrix r1, r2 in the saturation phase); r3 survived | survived 4 of 4, CPU-bound (fixed rates not sustained) |
| 256 MiB / 1 vCPU | **OOM-killed 6 of 7** (RE-1, screening, matrix r2, r3, confirmation r1, r2, all at or near the start of the saturation phase); matrix r1 survived at **254.7 of 256 MiB** | survived 5 of 5 |
| 512 MiB / 0.5 vCPU | survived 1 of 1 | survived |
| 512 MiB / 1 vCPU | survived 7 of 7 | survived |
| 1 GiB / 1 vCPU | survived 1 of 1 | survived |
| 1 GiB / 2 vCPU | survived 2 of 2 | survived |

**RE-1's central finding reproduces:** the baseline does not reliably run
Velasia's mixed workload in 256 MiB, at either CPU allocation. It is not
deterministically killed: in the one survival, the container peaked
1.3 MiB below its limit with a 4.8 s p99 in the fixed-rate mixed phase.
"Fails 9 of 11 runs of the `full` workload" is the accurate statement
(plus every `envelope` and `cycles` run at 256 MiB).

The kills are cgroup OOM kills (`oomKilled: true`, exit 137, no restart
policy), recorded with the failed requests of the phase they occurred in.
All OOM runs are kept in the results and counted; none was excluded.

### Memory repeatability (`base`, MiB)

| Configuration | Runs | Idle container (anon) | Peak container | After 300 s (anon) |
|---|---:|---|---|---|
| 512 MiB / 1 vCPU | 3 (matrix) | 34.3–38.4 (30.4) | 296–303 | 170 (156) |
| 512 MiB / 1 vCPU | RE-1, screening | 33.3, 34.4 | 292, 262 | 164 at 120 s, 150 at 120 s |
| 1 GiB / 2 vCPU | RE-1, wide | 34.6, 34.5 | 318, 316 | 185 at 120 s, 172 at 300 s |

Idle memory is stable to about ±2 MiB. Peaks vary by up to 40 MiB
(262–303 at 512 MiB): when V8's next full collection falls relative to the
closed-loop phase decides the peak, so a single-round peak comparison has
roughly ±20 MiB of uncertainty. After-load memory varies by 10–20 MiB.

### CPU and throughput repeatability (`base`, 512 MiB / 1 vCPU)

| Measure | Range across runs | Note |
|---|---|---|
| CPU per request, mixed 30/s | 9.9–11.0 ms (matrix); 21.4 (screening, first run of the series); 10.2 (RE-1) | one outlier at the start of a series |
| Closed-loop throughput, mixed | 106–145 req/s (matrix 106, 145, 137; screening 106; RE-1 144) | **±20 % between runs** |
| p99, mixed 30/s | 60–1400 ms | tails are not authoritative on this host |

So on this host CPU-per-request differences under about 15 % and
throughput differences under about 25 % are not established by one round,
and memory peaks need several rounds; memory after load and idle are
reliable to ±10 MiB.

### Comparison with RE-1

| Finding | RE-1 | RE-1B |
|---|---|---|
| `base` OOM at 256 MiB | 2 of 2 | 9 of 11, survives marginally otherwise |
| `base` and `node` OOM at 128 MiB | both | both |
| Allocator free memory after load | 85–101 MiB | 63–88 MiB (`malloc free` 300 s after load) |
| Live V8 data after full GCs | 47–52 MiB | 48–51 MiB |
| NextRS CPU per request below Node.js | 30–80 % | 15–80 % at fixed rates (SSR least, static files most) |
| Idle anonymous memory | NextRS 31, Node.js 47 | 30.2–30.8, 46–48.5 |

## Invalid or contended runs

- None excluded. One run is marked contended in the combined report:
  `re2b-combo-256m-0.5cpu-r2` began with host load 11.5 and its fixed-rate
  mixed p99 (13.7 s) is out of line with its other rounds (161–242 ms).
  Its memory figures agree with the other rounds.
- The screening's first run (`base-256m-1cpu-r1`) and `base-512m-1cpu-r1`
  had CPU-per-request figures twice their later repeats; treated as noise
  (first run after a gap, host load rising).

## Limits

- One application, one VM on a shared host, Intel x86_64 only.
- Rounds: 3 for the decisive configurations (256 MiB at both CPU sizes,
  512 MiB / 1 vCPU), 1 for the rest of the container matrix.
- Soak behaviour (hours) is not covered here.
