# RE-3: V8 memory optimization

Status: screening complete (one round); GC timing in
[RE-3-GC-AND-CPU-TRADEOFFS](RE-3-GC-AND-CPU-TRADEOFFS.md); repeated
measurements in [RE-23-COMBINED-BENCHMARKS](RE-23-COMBINED-BENCHMARKS.md).
Preliminary (one Docker Desktop VM on a shared Mac). Soak tests run last
(30 min, 2 h and 6 h, on a dedicated
Fedora Linux machine).

## Controls available in the pinned engine (rusty_v8 150.4.0, deno_core 0.413.0)

| Control | API | Used |
|---|---|---|
| Size every heap parameter from a memory amount | `CreateParams::heap_limits_from_system_memory` → `ResourceConstraints::ConfigureDefaults` | yes: `auto` passes container memory ÷ isolates |
| Old / young generation maximum | `set_max_old_generation_size_in_bytes`, `set_max_young_generation_size_in_bytes` | yes: the budget caps the old generation |
| Heap limit from a heap size | `CreateParams::heap_limits(initial, max)` → `ConfigureDefaultsFromHeapSize` | explicit `--js-heap-limit N` (unchanged) |
| Initial old generation size | `set_initial_old_generation_size_in_bytes` | no: V8's min(512 MiB, old/2) is already ≤ the budget's half |
| Near-heap-limit callback | `add_near_heap_limit_callback` | yes (since rc.1): terminate + replace the isolate, 64 MiB of headroom (a smaller grant was reverted: see budgeting) |
| Low-memory notification | `Isolate::low_memory_notification` | yes: idle reclamation |
| Memory pressure | `Isolate::memory_pressure_notification(Critical)` | yes: pressure reclamation |
| Memory reducer, idle tasks | V8 internal, driven by delayed tasks deno_core runs on Tokio | unchanged; it already shrinks the heap after load (RE-1) |
| V8 flags (`--max-semi-space-size`, `--optimize-for-size`, …) | `--v8-flags` (experimental) | screening only, never in defaults |

## Attribution ladder (screening, `memory` workload, 1 vCPU)

Each row adds one change to the one above. Peak = container
`memory.current` peak; MiB.

| Step | Heap limit | Young gen | Initial old limit | Max growth | 256 MiB outcome | 256 MiB peak | 512 MiB peak | 512 MiB after 120 s |
|---|---:|---:|---:|---:|---|---:|---:|---:|
| `base`: V8 sized from the VM (3.82 GiB) | 2052 | 96 | 512 | 1.96 | **OOM** | 256 | 262 | 149.5 |
| `base-semi2`: + young generation 6 MiB (`--max-semi-space-size=2`) | 2048 | 6 | 512 | 1.96 | survived (barely) | 245 | 223 | 151.5 |
| `re1-sizing`: V8 sized for the container (shipped `auto`) | 262 / 268 | 6 / 12 | 128 | 1.30 | survived | 193 | 192 | 135.9 |
| `re2-heap`: + budget cap (this branch) | **112** / 268 | 6 / 12 | 53 / 128 | 1.30 | survived | **165** | 180 | 129.1 |

At 512 MiB `re1-sizing` and `re2-heap` configure V8 identically (the
cap is not reached), so their difference (192 vs 180 peak, 136 vs 129
after) is run-to-run noise: about ±10 MiB in one round.

Reading the ladder:

- The **young generation** is the first-order fix: two 32 MiB semi-spaces
  plus a 32 MiB new large-object space were most of the 48–80 MiB of
  committed-but-unused heap RE-1 saw. Alone it avoids the OOM at 256 MiB
  with 11 MiB to spare.
- The **growth factor and initial limit** (sizing V8 for the container
  instead of the VM) take the next 50 MiB off the 256 MiB peak.
- The **budget cap** takes another 28 MiB off at 256 MiB and, more
  importantly, puts the heap limit (112 MiB) below the container (256),
  so a runaway heap becomes an isolate restart instead of an OOM kill.
  Peak heap use in these runs was 85 MiB; the limit was not reached and
  no isolate restarted.
- V8's live data after full collections is **50–66 MiB in every
  variant**: Next.js, React and Velasia's modules and caches. No setting
  changes it; it is the floor any engine running this application
  carries (RE-6 territory).

## GC frequency

Full (mark-compact) collections by the end of the closed-loop phase,
`full_gcs` per isolate: `base` 12, `base-semi2` 14–15, `re1-sizing`
31–33, `re2-heap` 28–40, `re2-lowmem` 32–38. A 2.5–3× increase in full
GCs is the price of the smaller heaps; whether it costs CPU or latency is
measured with `--trace-gc` in [RE-3-GC-AND-CPU-TRADEOFFS](RE-3-GC-AND-CPU-TRADEOFFS.md).
Startup was not slower (ready in 1.8–2.5 s for the budgeted heaps against
2.0–6.0 s for `base`).

## Rejected or not pursued

| Experiment | Result | Decision |
|---|---|---|
| `--max-semi-space-size=2` as a default flag | avoids the 256 MiB OOM by 11 MiB only; superseded by sizing from the container, which sets the same young generation through a supported API | rejected (and V8 flags stay out of defaults) |
| `--optimize-for-size` | not screened: it changes V8's compiler and inline-cache tiers, a CPU trade the budget does not need | not pursued |
| A smaller young generation than V8's container-sized one | the budget already gives 2 MiB semi-spaces at ≤ 512 MiB | not needed |
| Raising the initial old-generation size to skip startup GCs | startup was not slower | not needed |
| Forcing GC per request | excluded by the milestone rules | not done |

## Heap exhaustion

Not observed in any screening run (peak heap use 85 MiB against a 112 MiB
limit). What happens when it occurs is unchanged since rc.1 and covered by
the compatibility suite (`tests/compat/pages-reliability.mjs`,
`fixtures/next-pages/pages/api/heap.ts`): the isolate is terminated and
replaced, its in-flight requests fail, the process keeps serving, with
64 MiB of headroom while it unwinds (new regression test for a single
large allocation at the limit). V8
allocation failures outside the heap-limit path abort the process and are
not recoverable by the embedder ([budgeting](RE-3-V8-HEAP-BUDGETING.md)).
