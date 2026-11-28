# RE-1: resource baseline and attribution

Status: round 1
complete; round 2 discarded (see [Invalid runs](#invalid-runs)). The
repeats are in [RE-1B-BASELINE-VERIFICATION](RE-1B-BASELINE-VERIFICATION.md)
(the 256 MiB OOM reproduces in 9 of 11 runs).

## Summary

- **NextRS 0.1.0-rc.3's serving code is OOM-killed in a 256 MiB container
  on Velasia's mixed workload; the Next.js standalone server survives it.**
  Both die at 128 MiB. This is the single most important finding for a
  milestone whose goal is smaller containers.
- Cause: at the kill, V8 had **118 MiB committed for 67 MiB in use** and the
  native allocator (glibc) held **59 MiB free** next to 32 MiB in use. V8
  sizes its heap from the machine's memory (a 4 GiB VM: a heap limit of
  about 2 GiB) and never sees the container limit; Node.js sizes its heap
  from the cgroup limit (259 MiB heap limit in a 256 MiB container).
- **After load, free allocator memory dominates.** 120 s after the load,
  V8's own memory reducer has shrunk the heap back to 52 MiB, but glibc
  still keeps **85–101 MiB of free memory**: 55–60 % of the process's
  anonymous memory.
- **CPU is already NextRS's strength**: per successful request, 30–80 %
  less container CPU than the Node.js server on every workload, and 2.7×
  the saturation throughput on the mixed workload at 1 GiB / 2 vCPU.
- **Idle**: NextRS 31 MiB anonymous (container 35 MiB), Node.js 47 MiB
  (container 63 MiB).

## Environment

| Item | Value |
|---|---|
| Baseline commit | `0e1ec49` (main; serving code identical to `91a7729`, the Series 1 `optA` binary, sha256 `af994c15…48a2`, `nextrs 0.1.0-rc.3`, `dist` profile) |
| Harness | `benchmarks/resource` at `168ef73` (branch `feat/resource-efficiency`) |
| Toolchain | Rust 1.98.0 (Linux build in `rust:1.98-bookworm`), deno_core 0.413.0, rusty_v8 150.4.0 |
| Application | Velasia Airways `d04127a`, Next.js 16.4.6, one build for both runtimes (BUILD_ID `gvWJrvYSM71x059u5PMn2`) |
| Node.js | 24.18.0, Next.js standalone `server.js` (`velasia-node:s1`) |
| Images | Debian 12, glibc 2.36 |
| Host | Docker Desktop 29.7.2, LinuxKit 7.0.12, 8 vCPU / 3.8 GiB VM, cgroup v2, on a shared Intel Mac (host load average 40–300: other sessions' builds) |
| Storefront CPUs | VM CPUs 2–3 with `--cpus` per configuration; load generator and probe on 4–5; API on 6 |

## Method

`benchmarks/resource/README.md`. Per (runtime, configuration): a fresh
container, 30 s idle, a body-checked correctness pass over SSR and route
handlers, a cold pass, fixed-rate phases (static 200/s, RSC 30/s, SSR
5/s, route handlers 15/s, mixed 30/s), a closed-loop saturation phase on
the mixed workload (16 connections), then readings 10, 30, 60 and 120 s
after the load. The container is sampled once a second from outside it.
Raw data: [`results/baseline-r1/`](results/baseline-r1/) (one JSON per run,
one JSONL of samples per run, `SUMMARY.md`).

## Results (round 1)

### Memory

| Configuration | Runtime | Outcome | Idle container (anon) MiB | Peak container MiB | After 10 s | After 120 s (anon) |
|---|---|---|---:|---:|---:|---:|
| 1 GiB, 2 vCPU | NextRS | ok | 34.6 (31.2) | 318 | 318 | 185 (173) |
| | Node.js | ok | 63.2 (47.0) | 284 | 256 | 196 (166) |
| 512 MiB, 1 vCPU | NextRS | ok | 33.3 (30.2) | 292 | 284 | 164 (153) |
| | Node.js | ok | 52.8 (48.2) | 222 | 196 | 169 (158) |
| 256 MiB, 1 vCPU | NextRS | **OOM-killed** at the start of the saturation phase | 34.4 (30.9) | 250 | – | – |
| | Node.js | ok | 52.1 (47.6) | 221 | 194 | 166 (154) |
| 256 MiB, 0.5 vCPU | NextRS | **OOM-killed** during fixed-rate SSR | 34.3 (30.6) | 255 | – | – |
| | Node.js | ok (latency collapses: CPU-bound) | 51.3 (46.1) | 226 | 199 | 169 (158) |
| 128 MiB, 0.5 vCPU | NextRS | **OOM-killed** in the correctness pass | 33.9 (30.7) | 127 | – | – |
| | Node.js | **OOM-killed** in the correctness pass | 51.0 (46.8) | 128 | – | – |

Container memory is `memory.current` (includes page cache: Node.js reads
its JavaScript through the page cache, NextRS's file-backed memory is the
mapped binary). Process PSS at idle is about 68 MiB for both.

### Where NextRS's memory is

From NextRS's `/metrics.json` and the samples, 1 GiB / 2 vCPU (512 MiB in
brackets):

| Moment | Anonymous | V8 heap committed (used) | V8 external | Allocator in use | Allocator free | Threads |
|---|---:|---:|---:|---:|---:|---:|
| Idle | 31 (30) | 23 (14) | 1 | 3 | 4 (3) | 11 (9) |
| End of mixed 30/s | 223 (218) | 132 (84) [130 (57)] | 18 | 55 [26] | 33 [60] | 13 |
| End of saturation | 305 (276) | 184 (104) [169 (111)] | 28 | 77 [71] | 42 [32] | 14 (16) |
| 120 s after load | 173 (153) | 52 (48) | 3 | 18 | **101 [85]** | 11 (9) |

- Virtual size is about 18 GiB at all times: V8's pointer-compression cage
  and sandbox reservations. Not resident and not a cost.
- The file-backed part of RSS (37–42 MiB) is the binary and libraries,
  shared and the same in every configuration.
- **Under load**, V8's committed-but-unused heap (48–80 MiB) and the
  allocator's free memory (32–60 MiB) together are about half of the
  anonymous memory: headroom, not live data.
- **After load**, V8 returns to about its idle-plus-caches size by itself
  (deno_core runs V8's delayed tasks, including the memory reducer, on
  Tokio: `runtime/setup.rs` `spawn_delayed_task`), but glibc does not
  return its free arenas.
- At the 256 MiB kill: V8 committed 118 (used 67), allocator 32 in use + 59
  free, external 9 (`base-256m-1cpu-r1`, end of the mixed phase).

**Not a leak.** Live V8 data after full collections is 47–52 MiB after
every run (Series 1 found the same over longer windows); everything else
is returned or reusable. Unbounded growth would need a soak to rule out
(RE-8).

### CPU (container CPU per successful request, fixed rates)

| Workload | 1 GiB / 2 vCPU NextRS | Node.js | 512 MiB / 1 vCPU NextRS | Node.js | 256 MiB / 1 vCPU NextRS | Node.js |
|---|---:|---:|---:|---:|---:|---:|
| Static, 200/s | 0.99 ms | 6.17 | 1.03 | 4.59 | 1.19 | 4.71 |
| RSC, 30/s | 5.27 | 7.62 | 5.38 | 8.02 | 5.96 | 8.57 |
| SSR search, 5/s | 78.8 | 111.9 | 71.0 | 146.0 | 66.6 | 92.7 |
| Route handlers, 15/s | 8.99 | 14.4 | 9.01 | 14.9 | 8.91 | 13.1 |
| Mixed, 30/s | 9.59 | 20.3 | 10.2 | 23.7 | 9.99 | 16.4 |
| Saturation throughput (mixed, closed loop) | 217 req/s | 81 | 144 | 53 | OOM | 70 |

- **What costs CPU on NextRS:** SSR searches (66–79 ms each, JavaScript:
  React rendering of a 111 KiB page with streamed results) dominate the
  mixed workload: 2 of 20 requests and about 75 % of its CPU. RSC requests
  cost 5–6 ms (about half of it gzip, Series 1), static files about 1 ms.
- Node.js compresses static files per request (Series 1): 4.6–6.2 ms each.
- One NextRS isolate saturates near 1 CPU of JavaScript: at 2 vCPU the
  saturation phase used 1.4 cores.
- Idle CPU (Series 1): NextRS 18–19 millicores (isolate heartbeat at 40 Hz,
  supervisor at 20 Hz), Node.js 42.

Latency is not authoritative on this host. Within these runs NextRS's p99
at fixed rates was 3–137 ms against Node.js's 15–7000 ms (Node.js's tail
comes from its per-request compression and SSR at 1 vCPU).

## Largest contributors

| Rank | Memory | Evidence | Addressed in |
|---|---|---|---|
| 1 | V8 heap growth unconstrained by the container (heap limit ~2 GiB) | 48–80 MiB committed-unused under load; OOM at 256 MiB | RE-3: `--js-heap-limit auto` |
| 2 | glibc arena retention | 85–101 MiB free after load; 32–60 MiB under load | RE-2: idle and pressure trim, arena cap |
| 3 | Live V8 data (Next.js, React, application modules, caches) | 47–52 MiB after full GCs | Application and framework state: not engine overhead (RE-6) |
| 4 | Per-isolate duplication | Series 1: +20 MiB idle, +150–175 MiB peak per extra isolate | RE-4 |
| 5 | Binary and libraries (file-backed) | 37–42 MiB, shared, constant | Not addressed |

| Rank | CPU | Evidence | Addressed in |
|---|---|---|---|
| 1 | JavaScript rendering (SSR, RSC) | 66–79 ms per SSR search; 75 % of mixed-workload CPU | Application work; V8 tuning only (RE-5) |
| 2 | gzip of dynamic responses | ~half of RSC CPU (Series 1) | Matches `next start`; RE-5 |
| 3 | Static serving | 1 ms/request | Already 4–6× below Node.js |

## Invalid runs

- **Round 2 was discarded.** It overlapped a Docker build of the candidate
  binary on other VM CPUs. CPU accounting inside the VM inflated under host
  contention (Node.js's SSR correctness pass read 11.2 s of CPU per request
  against 0.68 s in round 1), so its CPU figures are meaningless; its
  memory figures are probably fine but the runs were stopped part-way.
  Files kept in the scratch `re/discarded/`. Rule adopted: no build or
  other heavy work in the VM during a measured series.
- **First attempt of the 128 MiB Node.js run** hung for hours: the load
  generator never gives up on a target that died during a fixed-count run.
  The harness now kills it after its duration plus 60 s; the run was
  repeated.

## Limits

- One round so far: no variability estimate. Memory results are
  deterministic enough to act on (the OOM reproduced in both 256 MiB
  configurations); CPU differences of under ~15 % are not established.
- One application. Velasia is a realistic Next.js 16 App Router
  application (PPR, Cache Components, ISR, Server Actions, route handlers),
  but other applications differ in live heap and rendering cost.
- Server Actions are not part of the workload (they need a shopping
  session per client and mutate the shared API's state); their memory
  behaviour is covered only by the compatibility suites.
- Latency and throughput come from a contended host (see Environment).
