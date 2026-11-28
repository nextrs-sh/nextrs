# RE-3: GC and CPU trade-offs

Status: complete. GC timing from one traced round (`results/re23-gctrace`);
CPU per request from the matrix, wide matrix, cycles and the final
binary's confirmation ([RE-23-COMBINED-BENCHMARKS](RE-23-COMBINED-BENCHMARKS.md));
young-generation sizes from `results/re23-confirm-young`; idle CPU per
thread from `results/re23-idle-threads`. Preliminary: one Docker Desktop
VM on a shared Mac; CPU differences under ~15 % are not established on
this host by one round. Soak tests run last (30 min, 2 h and 6 h, on a dedicated
Fedora Linux machine).

## Method

Variants with `--v8-flags --trace-gc` (V8 prints every collection with its
main-thread pause and, for incremental mark-compacts, the main-thread
marking steps before it), `memory` workload (mixed 30/s for 60 s, then
30 s closed-loop), 1 vCPU, logs saved per run (`--save-logs`), parsed by
`benchmarks/resource/gc-summary.mjs`. Tracing adds the same output cost to
every variant; its absolute CPU figures are not used elsewhere.

| Variant | Binary | V8 heap |
|---|---|---|
| `base-gc` | `af994c15` (RE-1 baseline) | V8 default for the 3.82 GiB VM: 2052 MiB limit, 96 MiB young generation |
| `re2b-heap-gc` | `be739569` | `--js-heap-limit auto` (budget: 112 MiB at 256 MiB; V8's 268 at 512 MiB) |
| `re2b-combo-gc` | same | `auto` + idle reclamation (10 s) + pressure reclamation (90 %) |

## Results

### 512 MiB, 1 vCPU (all three runs completed)

| Variant | Scavenges | Mean / p99 / max scavenge ms | Full GCs | Mean / max full-GC pause ms | Main-thread GC total (pauses + incremental steps) | Share of the traced span |
|---|---:|---|---:|---|---:|---:|
| base | 121 | 16.4 / 82.9 / **261.5** | 12 | 44.3 / 178.5 | 3.23 s | 2.33 % |
| heap budget | 883 | 3.3 / 35.3 / 191.0 | 28 | 21.2 / 83.3 | 4.61 s | 3.37 % |
| combined | 1018 | 3.2 / 35.2 / 51.0 | 34 | 25.1 / 80.4 | 5.16 s | 3.57 % |

### 256 MiB, 1 vCPU

| Variant | Outcome | Scavenges | Mean / max scavenge ms | Full GCs | Main-thread GC share |
|---|---|---:|---|---:|---:|
| base | **OOM-killed** in the closed-loop phase | 75 | 51.2 / **878.7** | 11 | 4.32 % (until killed) |
| heap budget | completed | 1561 | 2.4 / 142.2 | 33 | 4.59 % |
| combined | completed | 1703 | 2.3 / 127.0 | 37 | 4.48 % |

## What it means

- **More GC work, shorter pauses.** The container-sized heaps run 7–8×
  as many scavenges (2–4 MiB semi-spaces instead of 32) and 2.5–3× as many
  full collections. Main-thread GC time rises by about 1.1–1.2
  percentage points of the run at 512 MiB (3.2 s → 4.6–5.2 s over the
  same work: +40–60 % GC time). In exchange the longest pauses fall from
  262 ms (512 MiB) and 879 ms (256 MiB, a scavenge copying a 32 MiB
  semi-space on a throttled CPU) to 51–191 ms, and the p99 scavenge from
  83 ms to 35 ms.
- **A small CPU cost, consistent in direction.** In the traced round
  itself the difference was inside the noise (CPU per request at
  30 requests/s 13.7 ms base, 11.1 heap budget, 12.8 combined). Across
  all later stages, wherever `base` survives, the candidate's CPU per
  request is higher: +3–11 % at fixed rates and +3 % in the closed loop
  with the final binary (512 MiB / 1 vCPU, 2 rounds: 8.14 against
  7.91 ms, 124 against 128 requests/s), +3–30 % in single closed-loop
  rounds of the matrix and wide matrix. The young-generation experiment
  below shows where it comes from.
- **Reclamation itself is cheap and rare.** Idle reclamation (V8
  low-memory GC + `malloc_trim`) took 0.23–0.39 s once per idle period;
  pressure reclamation did not fire with the heap budget in place (the
  working set stayed below 90 %). Without the budget it fired 3 times at
  256 MiB for 7.2 s of collection (`re2-native`, screening): pressure
  reclamation is a safety net, not a substitute for the budget.
- **Idle memory improves as a side effect.** Idle reclamation also fires
  once after startup (starting counts as activity), collecting the
  build-loading garbage: idle anonymous memory 21.4 MiB (combined) against
  30.6–30.7 MiB without it.

## Young generation: where the CPU cost comes from

V8 sizes a semi-space from the memory it is given (memory ÷ 128,
rounded up to a power of two, 2–32 MiB): 2 MiB at 256 MiB, 4 MiB at
512 MiB, 8 MiB at 1 GiB, against 32 MiB for `base`, which V8 sizes from
the whole 3.82 GiB VM. The final candidate with larger semi-spaces
(`--v8-flags --max-semi-space-size=N`, an unsupported V8 flag, for the
experiment only), `full` workload, one round each:

| Configuration | Semi-space | Heap limit MiB | Closed-loop req/s | CPU ms/req, closed loop | CPU ms/req, mixed 30/s | Peak MiB | Anon after 300 s |
|---|---|---:|---:|---:|---:|---:|---:|
| 512 MiB / 1 vCPU | 4 MiB (V8's own, candidate) | 268 | 122.6 | 8.22 | 10.50 | 196.4 | 75.6 |
| | 8 MiB | 280 | 130.5 | 7.72 | 10.44 | 191.6 | 77.0 |
| | 16 MiB | 304 | 134.9 | 7.48 | 10.24 | 212.4 | 100.5 |
| | `base` (32 MiB, confirmation medians) | 2052 | 127.8 | 7.91 | 9.92 | 297.3 | 155.2 |
| 1 GiB / 2 vCPU | 8 MiB (V8's own, candidate) | 536 | 194.9 | 7.33 | 10.21 | 222.8 | 114.3 |
| | 8 MiB (flag set to the same value) | 536 | 198.2 | 7.18 | 10.25 | 223.4 | 117.7 |
| | 16 MiB | 560 | 203.1 | 7.03 | 10.23 | 230.8 | 113.2 |

- At 1 GiB the "8 MiB" flag repeats V8's own size, so those two rows are
  the same configuration: they differ by 2 % in CPU and throughput, the
  noise within one uninterrupted series.
- At 512 MiB a 16 MiB semi-space brings closed-loop CPU per request
  (7.48) to `base`'s (7.91) and throughput above it, for +16 MiB of peak
  and +25 MiB after load. 8 MiB recovers about two thirds of the gap
  with no measurable memory cost. At 1 GiB the effect is small (+4 %).
- So the closed-loop CPU cost is the smaller young generation (more,
  cheaper scavenges), not reclamation or the heap limit. It is V8's own
  sizing for the container and already rc.3's behaviour with
  `--js-heap-limit auto` at 512 MiB and above.

Not changed in this milestone: one round per size, CPU-bound phases
only, and the flag is unsupported. A semi-space floor (for example
8 MiB when the heap budget leaves room) is a candidate follow-up, to be
decided on the Linux host. Deployments that prefer throughput to memory
at 512 MiB+ can already pass `--v8-flags --max-semi-space-size=8`.

## Idle CPU (milestone §15)

RE-1 measured 18–19 millicores of idle NextRS CPU and pointed at the
isolate and supervisor heartbeats. `benchmarks/resource/idle-threads.mjs`:
fresh container at 512 MiB / 1 vCPU, no traffic except the image's own
health check (every 10 s), 60 s to settle, then CPU time per thread
(`/proc/PID/task/*/stat`, 10 ms ticks) and for the whole container
(cgroup `cpu.stat`) over 300 s.

| Variant | Container millicores | Main process | Largest threads |
|---|---:|---:|---|
| `node` | 27.1 | 0.8 | `next-server` 0.67, V8 workers 0.13 |
| `base` | 14.9 | 9.3 | `nextrs-v8-0` 6.93, `nextrs-supervisor` 2.03, V8 worker 0.20, Tokio 0.07 |
| candidate (`re2c-combo`) | 14.8 | 9.3 | `nextrs-v8-0` 6.59, `nextrs-supervisor` 2.03, Tokio 0.50, V8 worker 0.13 |
| `--profile balanced` | 14.8 | 9.2 | `nextrs-v8-0` 6.53, `nextrs-supervisor` 2.00, Tokio 0.50, V8 worker 0.17 |

- **Where NextRS's idle CPU goes.** About 6.6–6.9 millicores on the
  isolate thread: its 25 ms heartbeat (40 wakeups/s, which the stall
  detector reads) plus the 1 s heap sample; 2.0 on the supervisor's
  50 ms watch loop (20 wakeups/s). The remaining ~5.6 millicores of the
  container are the health check itself (a `bash` started every 10 s).
- **Node.js's process is nearly silent (0.8)**, but its health check
  starts a `node` process every 10 s, which costs more than all of
  NextRS's wakeups together; the container total is what a host pays.
- **RE-2/3 adds 0.4 millicores** (the idle-reclamation check on a Tokio
  worker), and nothing else changes at idle.
- **Not changed.** The heartbeat and watch loop are how NextRS detects a
  stuck isolate and enforces timeouts; an event-driven heartbeat (beat
  only while a request is running) could save ~7 millicores per idle
  isolate, under 1 % of a core, but changes the stall detector's
  invariants. Per the milestone rules this stays separate from the memory
  work: a follow-up, not part of this pull request.

## Rejected

| Configuration | Why |
|---|---|
| Per-request GC | excluded by the milestone rules; never measured |
| A 2 MiB young generation on V8's default heap (`--max-semi-space-size=2` on `base`) | same GC pattern as the budget without the heap limit's protection, through an unsupported flag |
| Pressure reclamation without a heap budget | ≈2.4 s of collection per reclamation under load on 1 vCPU, throughput halved (screening) |

No configuration in this milestone produced frequent major collections
beyond 2.5–3× the baseline, GC-related latency spikes above the
baseline's, premature heap exhaustion (no isolate reached its limit), or
isolate terminations.
