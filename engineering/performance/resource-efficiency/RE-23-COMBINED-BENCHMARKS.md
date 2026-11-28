# RE-2/3: combined benchmarks

Status: complete for the measured configurations. Preliminary: one
Docker Desktop VM on a shared Mac. Memory results are repeatable to about
±10 MiB (peaks ±20 MiB); CPU-per-request differences under ~15 % and
throughput differences under ~25 % are not established on this host
([RE-1B](RE-1B-BASELINE-VERIFICATION.md)). Soak tests run last (30 min,
2 h and 6 h, on a dedicated Fedora Linux machine,
[RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md)).

## Variants

| Label | Binary | Options |
|---|---|---|
| `node` | Next.js 16.4.6 standalone, Node.js 24.18.0 | – |
| `base` | rc.3 (`af994c15…`) | none (V8 sized from the 3.82 GiB VM) |
| heap budget only (`re2b-heap`) | `re2b` (`be739569…`) | `--js-heap-limit auto` |
| idle trim only (`re2b-trim`) | `re2b` | `--memory-reclaim-idle 10` |
| **candidate** (`re2b-combo`, `re2c-combo`) | `re2b`; final `re2c` | `--js-heap-limit auto --memory-reclaim-idle 10 --memory-pressure-reclaim 90` |

The candidate is the `low-memory` profile's mechanisms with the default
32 MiB compression cache; `--profile balanced` (idle 30 s) is measured in
the confirmation stage. All runs: one isolate, the same Velasia build, a
fresh container per run, no swap.

## Workloads

| Workload | Phases | Used for |
|---|---|---|
| `full` | correctness (body-checked SSR, route handlers, one Server Action), cold pass, static 200/s, RSC 30/s, SSR 5/s, route handlers 15/s, mixed 30/s, closed-loop mixed (16 connections), Server Actions 5/s, recovery 10–300 s | key and wide matrices, confirmation |
| `envelope` | mixed at 15, 30, 45, 60, 80, 100, 120/s (40 s each), stop at the first rate with failures or under 95 % achieved, then closed-loop | sustainable rate ([validation](RE-23-CONSTRAINED-CONTAINER-VALIDATION.md)) |
| `cycles` | 3 × (mixed 30/s 45 s, closed loop 20 s, 60 s idle) | repeated load and idle |
| `memory` | mixed 30/s, closed loop | screening, GC trace |

Server Actions: Velasia's booking-lookup form posted as a browser without
JavaScript does (`discover-action.mjs`), 5/s; it writes nothing.

## Single-factor attribution (`full`, round 1)

| Configuration | Variant | Outcome | Peak MiB | Anon after 300 s | Closed-loop req/s |
|---|---|---|---:|---:|---:|
| 256 MiB / 1 vCPU | `base` | survived (this round; OOM in 2 of 3) | 254.7 | 137.8 | 76 |
| | heap budget | survived | 169.4 | 103.5 | 93 |
| | idle trim | **OOM** | 252.6 | – | – |
| | both | survived | 167.5 | 86.5 | 107 |
| 256 MiB / 0.5 vCPU | `base` | **OOM** | 254.9 | – | 24 (until killed) |
| | heap budget | survived | 145.1 | 104.3 | 54 |
| | idle trim | **OOM** | 255.3 | – | – |
| | both | survived | 143.8 | 87.1 | 55 |
| 512 MiB / 1 vCPU | `base` | survived | 296.4 | 156.4 | 106 |
| | heap budget | survived | 181.1 | 106.2 | 104 |
| | idle trim | survived | 293.9 | 78.9 | 120 |
| | both | survived | 182.4 | 77.1 | 113 |

The heap budget sets the peak (−85 to −115 MiB) and is what avoids the
256 MiB OOM; the idle trim sets memory after load (−50 to −80 MiB) and
does nothing for the peak; together they add up. Without the budget
(trim only) the 256 MiB OOM remains. Details:
[RE-3-V8-MEMORY-OPTIMIZATION](RE-3-V8-MEMORY-OPTIMIZATION.md) (the
ladder from V8's default through container sizing to the budget),
[RE-2-NATIVE-MEMORY-OPTIMIZATION](RE-2-NATIVE-MEMORY-OPTIMIZATION.md).

## Key matrix (`full`, 3 rounds, rounds alternate variant order)

Medians, range in brackets; MiB.

| Configuration | Variant | OOM-killed runs | Failed requests | Idle anon | Peak | Anon after 300 s | Closed-loop req/s |
|---|---|---:|---|---:|---:|---:|---:|
| 256 MiB / 1 vCPU | `node` | 0 of 3 | 0 | 46.7 | 236.5 | 153.0 | 64.0 |
| | `base` | **2 of 3** | 0, 1, 1 | 30.5 | 254.7 | 137.8 (1 survivor) | 76.3 (1 survivor) |
| | candidate | 0 of 3 | 0 | 20.4 | **167.5** | **87.0** | **105.4** (72–107) |
| 256 MiB / 0.5 vCPU | `node` | 0 of 3 | 0 | 46.2 | 220.5 | 150.7 | 26.3 |
| | `base` | **2 of 3** | 110, 1, 0 | 30.2 | 254.9 | 134.9 (1 survivor) | 39.8 |
| | candidate | 0 of 3 | 0 | 20.8 | **151.3** (133–149 anon) | **78.3** (75–87) | 44.6 (37–55) |
| 512 MiB / 1 vCPU | `node` | 0 of 3 | 0 | 46.5 | 215.9 | 146.5 | 65.3 |
| | `base` | 0 of 3 | 0 | 30.4 | 302.9 | 156.4 | 136.5 (106–145) |
| | candidate | 0 of 3 | 0 | 21.2 | **182.4** | **83.2** (77–92) | 104.7 (94–113) |

## Wide matrix (`full`, 1 round)

| Configuration | Variant | Outcome | Idle anon | Peak | Anon after 300 s | Closed-loop req/s | CPU ms/req, closed loop |
|---|---|---|---:|---:|---:|---:|---:|
| 128 MiB / 0.5 vCPU | `node` | **OOM** (cold pass) | 46.5 | 127.9 | – | – | – |
| | `base` | **OOM** (cold pass) | 30.2 | 114.6 | – | – | – |
| | candidate | **survived**, all phases; SSR CPU-bound (2.2/s of 5) | 20.0 | 114.5 | 87.4 | 25.0 | 20.7 |
| 512 MiB / 0.5 vCPU | `node` | survived | 46.8 | 240.1 | 148.4 | 32.7 | 15.62 |
| | `base` | survived | 30.4 | 270.1 | 154.9 | 67.0 | 7.57 |
| | candidate | survived | 21.3 | **159.7** | **80.1** | 59.6 | 8.62 |
| 1 GiB / 1 vCPU | `node` | survived | 48.5 | 271.9 | 160.3 | 91.3 | 11.04 |
| | `base` | survived | 30.4 | 297.9 | 160.8 | 149.7 | 6.73 |
| | candidate | survived | 20.3 | **230.8** | **79.2** | 139.0 | 7.29 |
| 1 GiB / 2 vCPU | `node` | survived | 48.4 | 273.4 | 163.7 | 133.7 | 10.41 |
| | `base` | survived | 30.8 | 315.6 | 162.5 | 213.6 | 6.66 |
| | candidate | survived | 21.5 | **246.1** | **100.6** | 174.4 | 8.14 |

At 1 GiB the heap budget does not cap V8 (its own limit, 536 MiB, is
below the budget), so the lower peak there is V8 sized for 1 GiB instead
of the 3.82 GiB VM (young generation 24 instead of 96 MiB) plus reclamation.

## Load and idle cycles (`cycles`, 1 round)

| Configuration | Variant | Outcome | Peak per cycle | Anon after 300 s | Closed-loop req/s per cycle |
|---|---|---|---|---:|---|
| 256 MiB / 1 vCPU | `base` | **OOM** in cycle 1's closed loop | 254.0 | – | – |
| | candidate | 3 of 3 cycles | 161.4, 162.6, 165.6 | 84.1 | 104.6, 109.2, 94.8 |
| 512 MiB / 1 vCPU | `base` | 3 of 3 | 294.3, 291.6, 290.7 | 159.2 | 135.2, 117.7, 130.1 |
| | candidate | 3 of 3 | 174.6, 179.8, 177.4 | 91.0 | 128.7, 123.7, 125.3 |

No growth from cycle to cycle in either variant's peak.

## CPU and latency

**Fixed rates (open loop, the server is not saturated):** candidate and
`base` serve every rate with the same p50 (4.6–6.3 ms on mixed 30/s);
CPU per successful request on mixed 30/s:

| Configuration | `node` | `base` | candidate |
|---|---:|---:|---:|
| 256 MiB / 1 vCPU | 17.3 | 13.6 (11.0–20.9) | 11.8 |
| 256 MiB / 0.5 vCPU | 16.9 | 10.4 | 11.2 |
| 512 MiB / 1 vCPU | 24.1 (16.1–28.4) | 10.1 | 11.2 |
| 512 MiB / 0.5 vCPU | 16.3 | 10.0 | 10.4 |
| 1 GiB / 1 vCPU | 15.4 | 9.8 | 10.6 |
| 1 GiB / 2 vCPU | 18.1 | 9.9 | 10.4 |
| cycles, 512 MiB / 1 vCPU | – | 11.8–12.8 | 11.1–11.2 |

Where `base` has room, the candidate is 0–11 % above it at fixed rates,
within this host's noise, and 35–55 % below Node.js everywhere. At
256 MiB the candidate is lower than `base`, which runs next to its limit
(its 256 MiB / 1 vCPU p99 reached 4.8 s in one round; the candidate's
p99 was 86–201 ms).

**Saturation (closed loop):** with room to spare, `base` is faster.
CPU per request in the closed loop is higher for the candidate in every
configuration where `base` survives: +8 % (1 GiB / 1 vCPU), +14 %
(512 MiB / 0.5), +22 % (1 GiB / 2), +30 % (512 MiB / 1, key matrix
medians), but +3–5 % in the cycles at 512 MiB (7.85–8.21 against
7.46–8.61). Throughput differences follow (−5 % to −23 %), inside this
host's ±25 % range for single runs but consistently in one direction.
The likely cause is the container-sized young generation: 2–8 MiB
semi-spaces instead of 32 MiB, 7–8× as many scavenges
([RE-3-GC-AND-CPU-TRADEOFFS](RE-3-GC-AND-CPU-TRADEOFFS.md)). This is
already the behaviour of rc.3's shipped `auto` at those sizes (the
budget does not cap V8 at 512 MiB and above); the final binary's
confirmation and the young-generation runs measure it directly (below).

## Invalid or contended runs

| Run | Problem | Treatment |
|---|---|---|
| `re2b-combo-256m-0.5cpu-r2` (matrix) | host load 11.5 at start; fixed-rate mixed p99 13.7 s against 161–242 ms in rounds 1 and 3 | kept, memory used, latency excluded from medians' interpretation |
| `re2b-combo-256m-0.5cpu-r1` (envelope) | host load rose to 67 during the run (other work on the Mac); CPU per request read 3× the other rounds | **invalid**; repeated as round 2 with a host-load gate (load 4–9 during the run); round 2 is the one reported |

No run was dropped; every OOM is counted.

## Final binary (`re2c`) confirmation

`re2c` (`052d741d…`) is the code in this pull request, with every
[review](RE-23-INDEPENDENT-REVIEW.md) fix. `full` workload, 2 rounds,
same order rules; medians, range in brackets.

| Configuration | Variant | OOM-killed runs | Failed requests | Peak MiB | Anon after 300 s | Closed-loop req/s | CPU ms/req, mixed 30/s | CPU ms/req, closed loop |
|---|---|---:|---|---:|---:|---:|---:|---:|
| 256 MiB / 1 vCPU | `base` | **2 of 2** (closed loop) | 1, 112 | 256.0 | – | – | 9.67 | – |
| | candidate | 0 of 2 | 0, 0 | 189.9 (166–213) | 84.3 | 118.7 | 10.58 | 8.51 |
| 512 MiB / 1 vCPU | `base` | 0 of 2 | 0, 0 | 297.3 | 155.2 | 127.8 | 9.92 | 7.91 |
| | candidate | 0 of 2 | 0, 0 | 189.9 | 84.5 | 123.9 | 10.50 | 8.14 |

- The final binary reproduces the matrix: no OOM at 256 MiB where `base`
  is killed in both rounds; at 512 MiB −107 MiB of peak and −71 MiB of
  anonymous memory after load.
- One 256 MiB round peaked at 213 MiB in the closed loop (the other at
  166); 43 MiB of margin in the worse round. Anonymous memory stayed at
  157 MiB; the rest was reclaimable page cache.
- CPU per request at fixed rates is 4–11 % above `base` in every phase
  (SSR 72.3 against 69.4 ms, Server Actions 35.9 against 32.4 ms, mixed
  10.5 against 9.9); closed-loop CPU +3 % and throughput −3 % at 512 MiB.
  p50 is equal; p99 at fixed rates equal or lower except SSR at 256 MiB
  (238 ms against 102, one round 392 ms).
- The young-generation runs (`re23-confirm-young`) attribute the
  closed-loop cost to V8's container-sized young generation: at 512 MiB a
  16 MiB semi-space matches `base`'s CPU per request for +16 MiB of peak
  ([RE-3-GC-AND-CPU-TRADEOFFS](RE-3-GC-AND-CPU-TRADEOFFS.md#young-generation-where-the-cpu-cost-comes-from)).
