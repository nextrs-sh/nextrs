# RE-3: container-aware V8 heap budgeting

Status: implemented
on `feat/re-2-3-memory`, opt-in (`--js-heap-limit auto`, profiles); mechanism derived from the
pinned V8 source; effect measured in
[RE-23-COMBINED-BENCHMARKS](RE-23-COMBINED-BENCHMARKS.md).

## Why V8 outgrew the container (pinned engine: rusty_v8 150.4.0)

V8 sizes a heap from "physical memory" (`ResourceConstraints::ConfigureDefaults`,
`v8/src/api/api.cc:581`, `v8/src/heap/heap.cc`). Without configuration,
deno_core passes nothing and V8 uses the machine's memory: in the 3.82 GiB
Docker VM, whatever the container limit. Four parameters follow from it:

| Parameter (source) | Rule | 3.82 GiB VM (RE-1 baseline) | 16 GiB host |
|---|---|---:|---:|
| Old generation limit (`OldGenerationSizeFromPhysicalMemory`) | clamp(mem/2, 256 MiB, 4 GiB) | 1956 MiB | 4096 MiB |
| Semi-space max (`YoungGenerationSizeFromPhysicalMemory`, then rounded up to a power of two in `ConfigureHeap`) | clamp(mem/4/32, 2, 32 MiB) | 32 MiB (30.8 rounded up) | 32 MiB |
| Young generation (2 semi-spaces + new large-object space) | 3 × semi-space | 96 MiB | 96 MiB |
| Initial old generation limit (`ConfigureHeap`) | min(512 MiB, old/2) | 512 MiB | 512 MiB |
| Max heap growing factor (`MemoryController::MaxGrowingFactor`) | 4.0 if old ≥ 2 GiB, else 1.3 + 0.7 × (max(old, 256) − 256) / 1792 | 1.96 | 4.0 |

So in RE-1's 256 MiB containers the baseline would not run its first
full GC until the old generation reached ~512 MiB (twice the container),
let the young generation grow to 96 MiB, and grew the heap by up to 2×
its live data between full GCs. The OOM at 118 MiB committed / 67 MiB used
is the expected outcome, not a leak.

The shipped `--js-heap-limit auto` (65397c6, `main`) passed the
container's memory as V8's "physical memory", as Node.js does. That fixes
three of the four parameters but not the limit: V8 never gives an old
generation less than 256 MiB from memory alone, so a 256 MiB container
got a **262 MiB heap limit** (Node.js: 259 MiB). The heap could still grow
to the container's size before V8 failed it, and the process would be
OOM-killed first.

## The budget (`nextrs_runtime::heap_budget`)

```
available      = min(effective cgroup limit, physical memory)    (RE-3-CGROUP-RESOURCE-DETECTION)
reserved       = 64 MiB                      base: binary/library pages charged as page cache (37–42 MiB in RE-1),
                                               HTTP server, Tokio, metrics (~10 MiB), headroom
               + --compression-cache          (32 MiB default, 8 with low-memory)
               + 48 MiB × isolates            per isolate: V8 external memory (9–28 MiB under load in RE-1),
                                               the isolate thread's native allocations and retention
heap limit     = max((available − reserved) / isolates, 64 MiB)  clamped=true below the floor → startup warning
sizing memory  = max(available / isolates, 64 MiB)               V8's "physical memory" for the other parameters
```

The worker (`worker.rs` `heap_params`) configures V8 for `sizing memory`
and then caps the heap: if the cap is below what V8 picked, the young
generation keeps V8's size (at most a quarter of the cap, its semi-space
rounded down to a power of two because V8 rounds it up) and the old
generation gets the rest. The cap never raises V8's own choice. The
budget is computed once at startup (a live container resize is not
followed; pressure reclamation does read the current limit). Each isolate
logs the limit V8 actually applied (`V8 heap limit in effect`): it can be
V8's own, lower, value, or come from `--v8-flags`, which V8 applies after
the embedder's constraints (a warning says so).

**Isolate pools share one budget:** four isolates in 1 GiB get 184 MiB
each (736 MiB of heap + 288 reserved), not 1 GiB each.

### Resulting V8 parameters

| Container | Isolates | Mode | Heap limit | Old gen | Young gen | Initial old limit | Max growth |
|---|---:|---|---:|---:|---:|---:|---:|
| any (3.82 GiB VM) | 1 | default (RE-1 baseline) | 2052 | 1956 | 96 | 512 | 1.96 |
| 256 MiB | 1 | shipped `auto` (sizing only) | 262 | 256 | 6 | 128 | 1.30 |
| 256 MiB | 1 | **budgeted `auto`**, cache 32 | **112** | 106 | 6 | 53 | 1.30 |
| 256 MiB | 1 | budgeted `auto`, cache 8 (`low-memory`) | 136 | 130 | 6 | 65 | 1.30 |
| 256 MiB | 2 | budgeted `auto` | 64 (clamped, warns) | 58 | 6 | 29 | 1.30 |
| 128 MiB | 1 | budgeted `auto` | 64 (clamped, warns) | 58 | 6 | 29 | 1.30 |
| 512 MiB | 1 | budgeted `auto` | 268 (V8's own; cap 368 not reached) | 256 | 12 | 128 | 1.30 |
| 512 MiB | 2 | budgeted `auto` | 160 | 154 | 6 | 77 | 1.30 |
| 1 GiB | 1 | budgeted `auto` | 536 (V8's own; cap 880) | 512 | 24 | 256 | 1.40 |
| 1 GiB | 4 | budgeted `auto` | 184 | 178 | 6 | 89 | 1.30 |

(MiB; computed from the V8 rules above; the measured
`nextrs_executor_heap_limit_bytes` matches within V8's page rounding, see
the runtime test `heap_sized_from_memory_follows_v8s_defaults_for_that_memory`.)

At one isolate the cap only bites below about 400 MiB: at 512 MiB and
above, `auto` behaves exactly like the shipped version (and like Node.js).

### Where the reserve numbers come from

RE-1, 512 MiB / 1 vCPU and 1 GiB / 2 vCPU, end of saturation: anonymous
memory 276–305 MiB with V8 committed 169–184 MiB, so 107–121 MiB of
non-heap anonymous memory, of which glibc free (retained) 32–42 MiB. Live
native memory under load is therefore 70–80 MiB for one isolate,
compression cache included, plus 37–42 MiB of file-backed pages. The
reserve (64 + 32 + 48 = 144 MiB with the default cache) covers file pages
+ live native memory with ~25 MiB to spare, but **not** a large glibc
retention on top: the budget assumes allocator retention is kept down
(RE-2: arena cap, trimming) or that the kernel can reclaim the file pages.
The constants are a first, Velasia-derived setting and are validated only
there ([RE-23-CONSTRAINED-CONTAINER-VALIDATION](RE-23-CONSTRAINED-CONTAINER-VALIDATION.md)).

## Precedence and invalid values

| Given | Result |
|---|---|
| nothing (no profile) | V8 default, unchanged behaviour |
| `--profile balanced` / `low-memory` | `auto` |
| `--js-heap-limit auto` | budgeted as above (wins over the profile) |
| `--js-heap-limit N` (1–1048576 MiB) | exactly N per isolate, V8 derives the young generation from N (unchanged since rc.1); warns if N × isolates ≥ the container limit |
| `--js-heap-limit 0`, `lots`, `> 1048576` | refused at startup (unchanged) |
| `auto` with no readable limit and no physical memory | V8 default, with a warning |
| `auto` where the budget is below 64 MiB per isolate | 64 MiB each, warning naming available/reserved/isolates |

No new flag or configuration source was added: `auto` already existed
(rc.3 / PR #50) and only its policy changed. The new policy is still
opt-in; the default stays V8's own sizing until validated and approved.

## A heap limit is not a process limit

- Reaching the heap limit terminates and replaces **that isolate**
  (`FailureReason::HeapLimit`): its in-flight requests fail, the process
  and other isolates keep serving, the isolate restarts with backoff.
  The near-limit callback grants 64 MiB (unchanged since rc.1) so V8 can
  unwind instead of aborting. A smaller, heap-proportional grant was
  tried and reverted after review: on its last-resort path V8 calls the
  callback **once** and retries the failed allocation **once**
  (`heap-allocator.cc` `RetryCustomAllocate`, then
  `FatalProcessOutOfMemory("CALL_AND_RETRY_LAST")`), so one object larger
  than the grant (a 20–60 MiB string or array) would abort the whole
  process instead of replacing one isolate. Regression test:
  `a_large_allocation_at_the_heap_limit_replaces_the_isolate_instead_of_aborting`.
  The grant is a permission, not memory: V8 uses only what the failing
  allocation needs before the termination lands, so it is not part of the
  reserve; in the worst case that allocation can push a small container
  over its limit (an OOM kill, which in a one-isolate container ends the
  same way as an abort).
- The container limit still applies to the whole process; nothing here
  prevents an OOM kill if native memory, not the heap, grows.
- **Not every allocation failure is recoverable.** V8 aborts the process
  (`FatalProcessOutOfMemory`) on failures outside the heap-limit path
  (e.g. a failed page reservation, a single object larger than the
  remaining limit plus headroom after the callback). These are not
  handled and cannot be from the embedder. In a container they look the
  same as an OOM kill: the process restarts.

## Admission and guardrails

No new admission control was added. Existing bounds that limit heap
demand per isolate: `--max-concurrent` (128 requests executing at once
per isolate), `--max-queued` 512 / `--queue-timeout` 10 s,
`--request-timeout` 60 s, `--max-request-body` 10 MiB. Whether 128
concurrent SSR renders fit a 112 MiB heap is workload-specific; the
saturation phase (16 connections) and the envelope ladder measure what
Velasia actually needs (see the validation report). Guardrails added:
the clamp warning, the oversized-explicit-limit warning, and the startup
log line with available / reserved / per-isolate heap.
