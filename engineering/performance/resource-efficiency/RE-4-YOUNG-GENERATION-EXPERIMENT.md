# RE-4G: young-generation sizing (experiment set-up)

**Status: implemented as an experimental option, not measured.** The
CPU and memory measurements that would choose a policy need the
dedicated Linux host ([RE-4-LINUX-BENCHMARK-RUNBOOK](RE-4-LINUX-BENCHMARK-RUNBOOK.md)).
No size is enabled by default or by any profile.

## Why

RE-2/3 traced the closed-loop CPU regression to the small young
generation V8 picks for a container: semi-spaces of 2 MiB at 256 MiB,
4 MiB at 512 MiB, 8 MiB at 1 GiB, against 32 MiB for the VM-sized
baseline. That means more, cheaper scavenges. In one round each on the
Mac VM, at 512 MiB / 1 vCPU, 8 MiB semi-spaces recovered about two
thirds of the CPU gap with no measurable memory cost, and 16 MiB all of
it, for +16 MiB peak ([RE-3-GC-AND-CPU-TRADEOFFS](RE-3-GC-AND-CPU-TRADEOFFS.md)).
Those runs used `--v8-flags --max-semi-space-size=N`. That V8 flag is
unsupported, and it **raised** the heap limit (268 → 280 → 304 MiB).

## What was implemented

`--js-young-semi-space <MiB>` → `Limits::young_semi_space_floor_bytes`,
applied in `heap_params` (`crates/nextrs-runtime/src/worker.rs`) through
the supported `v8::CreateParams` setters
(`set_max_young_generation_size_in_bytes`,
`set_max_old_generation_size_in_bytes`) that the heap budget already
uses. No V8 flags.

| Rule | Why |
|---|---|
| Young generation = 3 × the semi-space: two semi-spaces plus the new large-object space (V8's `YoungGenerationSizeFromSemiSpaceSize`) | the whole young generation is counted |
| The semi-space is rounded down to a power of two | V8 rounds semi-spaces up; rounding down first keeps the total |
| The old generation is reduced by what the young one gains: `young + old` is unchanged | the configured cap (`--js-heap-limit`) never grows |
| The young generation never takes more than half of the total; the floor is halved until it fits | the old generation keeps room for live data |
| A floor below V8's own young generation changes nothing | it is a floor, not a size |
| Only with a configured heap (`--js-heap-limit` or a profile that sets it); otherwise a warning and no effect | without a cap there is no total to keep |

What it does to `--js-heap-limit auto` (unit tests in `worker.rs`):

| Container, isolates | Heap cap | V8's young generation | With 4 MiB floor | 8 MiB | 16 MiB |
|---|---:|---:|---:|---:|---:|
| 256 MiB, 1 | 112 MiB | 6 MiB (2 MiB semi) | 12 young / 100 old | 24 / 88 | 48 / 64 |
| 512 MiB, 1 | (V8's own sizing, cap not reached) | 12 MiB (4 MiB semi) | unchanged | 24 young, old reduced by 12 | 48, old reduced by 36 |
| 32 MiB cap (test) | 32 MiB | — | 12 / 20 | 12 / 20 (halved to fit) | 12 / 20 |

With real V8 (`tests/memory.rs`
`a_young_generation_floor_keeps_the_heap_limit`), the heap limit V8
reports at the 256 MiB budget stayed within 2 MiB of the unfloored limit
for 4, 8 and 16 MiB floors, and requests ran.

## The trade-off to measure

A larger young generation commits more memory up front: up to 3 × the
semi-space, before any old-generation growth. It also leaves less old
generation under the same cap, so a heap-limit termination comes
earlier for an application with a large live set. Velasia's live data
after a full GC is 50–66 MiB (RE-3). At 256 MiB, an 8 MiB floor leaves
88 MiB of old generation over that live set, and 16 MiB leaves 64 MiB,
close to it. The brief's condition is applied: **no larger young
generation at 256 MiB without evidence.** The runbook's
`re4-admission-young8` variant is dropped at 256 MiB unless the 512 MiB
results justify testing it there.

## Experiment plan (Linux host)

| Config | Variants | Measure |
|---|---|---|
| 512 MiB / 1 vCPU | `re4-admission` vs `+ --js-young-semi-space 8` vs `+ 16` | CPU per successful request (closed loop and fixed rate), scavenge count and pause (`--trace-gc` run), peak and post-load anonymous memory, heap-limit terminations |
| 1 GiB / 2 vCPU | same | the same; V8 already uses 8 MiB here, so `8` is a control |
| 256 MiB / 1 vCPU | `+ 4` only, if 512 MiB shows a gain | the same, plus restarts |

Decision rule, proposed: a floor is worth proposing as a default for a
configuration only if, over 3 rounds, it cuts CPU per request by at
least 5 %, adds at most 8 MiB of peak memory, and adds no heap-limit
termination or OOM. Otherwise V8's sizing stays.

## Answer

Young-generation tuning can probably recover part or all of the CPU
regression at 512 MiB and above. That rests on one round each, on the
Mac VM. There is now a supported, cap-preserving way to apply it. Whether
to apply it, and at which size, is unmeasured on dedicated hardware.
