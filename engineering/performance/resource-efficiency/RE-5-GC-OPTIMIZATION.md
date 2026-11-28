# RE-5G: young-generation sizing

This report evaluates `--js-young-semi-space` (RE-4G: a cap-preserving
floor for each isolate's semi-space, through `CreateParams`, with no
production V8 flag) against V8's own sizing, at the heap caps that
`--js-heap-limit auto` gives one isolate in 512 MiB and 1 GiB containers.
**Process level, not a container measurement.** It is no basis for a
default, and no default changed.

## 1. Method

- **Driver:** `benchmarks/phase3a/gc.mjs`, with two options added in RE-5:
  - `--heap-limit H` passes `--js-heap-limit H`;
  - `--young-api` sets the semi-space through `--js-young-semi-space N`
    instead of raw V8 flags.
- **GC tracing:** V8's `--trace-gc-nvp` still reports every collection
  with its pause and type. The tracing costs a little, so configurations
  are compared within one run only.
- **Heap caps:**
  - **268 MiB**, `auto` for 512 MiB with 1 isolate (RE-3 table);
  - **536 MiB**, `auto` for 1 GiB with 1 isolate.
- **Semi-space:** V8's own (default), 4, 8, 16 MiB. The young generation
  is 3 × the semi-space and is taken from the old generation; the total
  cap is unchanged (`a_young_generation_floor_keeps_the_heap_limit`).
- **The process-level difference:** with an explicit `--js-heap-limit`
  and no container limit, V8 derives its default young generation from
  the cap. In a container, `auto` sizes it from the container's memory
  (`heap_limits_from_system_memory`). So "default" here is not
  necessarily the container's default: the comparison isolates the
  semi-space at a fixed cap.
- **Workloads:** `ssr`, `rsc`, `action`, `streaming` (the JavaScript
  routes, where GC matters), 1 isolate, closed loop, 32 connections,
  warm-up 5 s, measured 10 s, 2 rounds, every response checked.
- **256 MiB is not run**, per the brief and RE-4G, until larger-container
  evidence justifies it.

## 2. Results

Candidate `0b2340e…`; medians of 2 rounds; 64 of 64 runs valid. Raw data
in `results/re5/gc-268/` and `gc-536/`, with `summary.md` per run.

| Cap | Workload | Semi-space | CPU ms/req | Minor GCs / 1k req | Mean minor pause ms | Major / 1k | GC pause % of CPU | p99 ms | Anon peak MiB |
|---|---|---|---|---|---|---|---|---|---|
| 268 | ssr | default | 1.824 | 31.2 | 1.61 | 6.63 | 4.0 | 55.5 | 100.8 |
| 268 | ssr | 4 | 1.837 | 31.2 | 1.69 | 7.26 | 4.0 | 59.0 | 101.8 |
| 268 | ssr | 8 | 1.826 | 31.0 | 1.57 | 6.49 | 3.9 | 56.1 | 102.5 |
| 268 | ssr | 16 | 1.862 | 30.8 | 1.70 | 7.19 | 4.0 | 62.1 | 98.6 |
| 268 | rsc | default | 1.011 | 17.0 | 2.11 | 3.41 | 4.8 | 34.2 | 99.4 |
| 268 | rsc | 16 | 1.023 | 16.8 | 2.14 | 3.48 | 4.8 | 34.6 | 102.4 |
| 268 | action | default | 0.685 | 12.4 | 2.03 | 3.11 | 4.8 | 20.9 | 97.2 |
| 268 | action | 16 | 0.684 | 12.5 | 2.13 | 3.14 | 5.1 | 20.7 | 96.1 |
| 268 | streaming | default | 2.270 | 37.3 | 1.83 | 9.24 | 4.3 | 81.4 | 98.5 |
| 268 | streaming | 16 | 2.285 | 38.5 | 1.88 | 9.50 | 4.5 | 82.5 | 95.3 |
| 536 | ssr | default | 1.717 | 16.0 | 2.56 | 4.60 | 3.3 | 63.2 | 135.1 |
| 536 | ssr | 16 | 1.712 | 16.3 | 2.51 | 4.61 | 3.3 | 61.0 | 138.1 |
| 536 | rsc | default | 0.966 | 8.5 | 3.40 | 2.85 | 4.1 | 33.1 | 139.3 |
| 536 | rsc | 16 | 0.968 | 8.5 | 3.42 | 2.85 | 4.2 | 33.2 | 136.1 |
| 536 | action | default | 0.647 | 6.5 | 3.57 | 2.16 | 4.5 | 20.3 | 127.2 |
| 536 | streaming | default | 2.087 | 19.1 | 3.00 | 5.48 | 3.7 | 82.1 | 126.1 |

The 4 and 8 MiB rows for every workload, and all of 536 MiB, are in the
raw data. They differ from the default by less than the variation
between rounds, as the rows shown do. Every response of every run was
correct, and each run's GC trace came from a single isolate. These rows
do not record executor restarts, so a heap-limit replacement is not
excluded by them, though one would have failed in-flight requests.

**No setting changed the minor-GC rate.** The arithmetic explains it:

- each SSR request allocates 532 KiB in the young generation, so 1 000
  requests allocate about 520 MiB;
- at 268 MiB, 31 scavenges per 1 000 requests means about 17 MiB per
  scavenge;
- at 536 MiB, 16 scavenges means about 32 MiB.

So with an explicit cap and no container memory to size from, V8 already
gives the isolate 16–32 MiB semi-spaces. Floors of 4, 8 and 16 MiB are at
or below V8's own size, and by design (RE-4G: "a floor below V8's own
young generation changes nothing") they are no-ops.

**Rejected experiment.** I suspected the floor raised only the maximum
young-generation size, which V8 grows into only under high survival. I
also set the initial young-generation size to the floor
(`set_initial_young_generation_size_in_bytes`, a supported API) and
reran ssr and rsc at 268 MiB with default, 8 and 16 MiB:

- candidate `f4779b0…`, `results/re5/gc-268-initial/`;
- minor GCs per 1 000 requests stayed 30.6–31.3 (ssr) and 14.0–17.2
  (rsc).

The change was reverted: no evidence supports it.

## 3. Assessment

- **The question cannot be answered at process level.**
  - The young-generation sizes the brief asks about (V8's 2–4 MiB
    semi-spaces at 256–512 MiB, RE-4G) appear only when `auto` sizes the
    heap from a container's memory.
  - Outside a container `auto` reads the host or sandbox memory, and
    there is no override for the memory it budgets for.
  - An explicit `--js-heap-limit` gives V8's large-heap sizing instead.
- **GC costs 3.3–5 % of CPU** on these routes at both caps. A larger
  young generation could recover part of that where V8's own is small.
  The RE-2/3 Mac VM measurement at 512 MiB (8 MiB semi-spaces: about two
  thirds of the closed-loop CPU regression) is the evidence that it can.
- **Young-generation tuning is not justified as a default by RE-5's
  data, and is not refuted.** The container campaign's GC comparison at
  512 MiB and 1 GiB (the `re4-admission-young8` variant) decides it.
- **256 MiB was not run**, per the brief.
