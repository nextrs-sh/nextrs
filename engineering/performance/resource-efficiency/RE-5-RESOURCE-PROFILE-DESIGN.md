# RE-5I: coordinated resource profile design

This report refines the RE-4.1F proposal
([RE-4.1-RESOURCE-BUDGET-COORDINATION](RE-4.1-RESOURCE-BUDGET-COORDINATION.md))
into a design that the container campaign can test. It covers design and
candidate configurations only:

- **nothing is implemented;**
- the `low-memory` and `balanced` profiles are unchanged;
- the candidate numbers are arithmetic from process-level measurements,
  not container results.

## 1. Four quantities, kept apart

| Quantity | Example | Where it comes from |
|---|---|---|
| **Configured ceiling** | `--js-heap-limit 136`, `--compression-cache 8` | flags and profiles |
| **Reserved demand** | admission's 2 MiB per request; image work estimated from the header; native budget permits | reservations held now |
| **Actual allocation** | V8 heap used, cache bytes, permits granted, `nextrs_prerender_cache_bytes` | each subsystem's own counter |
| **Measured resident memory** | the cgroup working set (anon + active file) | the kernel |

- **Working set:** the working set already contains every actual
  allocation. A policy that adds ceilings or allocations to it counts
  memory twice (RE-4-RESOURCE-ACCOUNTING §3).
- **Reserved demand:** demand not yet allocated is the only thing worth
  adding to the working set, and only until it shows up. That is
  `committed` accounting with its TTL (RE-4.1B).

## 2. The envelope

For a container limit `E` with `n` isolates:

```
fixed(n)        measured per release: binary + runtime + n × isolate floor
metadata(build) measured per build: routes, prerender index, manifests
heap            n × per-isolate heap ceiling (V8: actual + overshoot ≤ 3 × 64 MiB per episode)
native          native buffer budget + image sources + image work
caches          compression + data/ISR + image + prerender cache
unenforced      encoded strings, resizable buffers, adopted Rust buffers,
                small ArrayBuffers, zlib streams (≈ 256 KiB per concurrent gzip response),
                glibc retention between trims
headroom        E × (1 − critical)
invariant (target): fixed + metadata + heap + native + caches + E_unenforced + headroom ≤ E
```

**What RE-5 adds to RE-4.1F:**

- **`metadata(build)` is now a separate, measurable term.** With
  `--prerender-cache`, prerendered output no longer sits in `fixed`; what
  remains grows at about 4.5 KiB per prerendered route (RE-5B). It is
  metadata, not payload.
- **The prerender cache is one more capacity ceiling** among the caches.
  With a native buffer budget its bytes are inside that budget, so the
  two must be sized together: the cache at most a fraction of the
  budget.
- **`E_unenforced` cannot be bounded by configuration** (RE-5H). A
  profile must reserve for it from measurement: its p99 in the campaign's
  memory-intensive and slow-client workloads.

## 3. What stays independent

Each of these limits is still applied on its own. None knows the others
or `E`:

- V8 heap per isolate;
- ArrayBuffer budget (`--js-external-memory-limit`);
- native buffer budget (`--native-buffer-budget`; with `--prerender-cache`
  it also covers the prerender cache's contents and bodies in flight);
- image source budget and image memory budget;
- image cache, data/ISR cache, compression cache, prerender cache;
- per-request ceilings (body, response buffer, fetch response);
- the queue (count and, with admission, bytes);
- admission (decides from the working set) and reclaim (acts on it).

## 4. Design for a future coordinated profile (not implemented)

1. At startup, derive `E` (cgroup limit or flag). Measure `fixed` and
   `metadata` after the app is built: RSS anon then is both, plus the
   first isolate.
2. Allocate the remainder by shares, each still overridable by its flag:

   | Share of `E − fixed − metadata − headroom` | 256 MiB | 512 MiB |
   |---|---|---|
   | heap (all isolates) | 50 % | 55 % |
   | native (budget + images) | 20 % | 20 % |
   | caches (compression + data + image + prerender) | 15 % | 15 % |
   | unenforced reserve | 15 % | 10 % |

3. Replace the three native ceilings (native buffer budget, image
   sources, image work) with **one native work budget** that they all
   reserve from. Admission then sees image work as reserved demand.
4. Log the resulting profile once. Expose each share and its use as
   metrics.

This is a global memory manager in its own right, out of scope for RE-5.

## 5. Candidate configurations for the campaign (UNVALIDATED)

These are the RE-4.1F §5 starting points, with RE-5's additions. Every
value is an existing flag.

| Setting | 256 MiB, 1 isolate | 512 MiB, 1 isolate |
|---|---|---|
| `--profile` | `low-memory` | `low-memory` or `balanced` |
| `--prerender-cache` | **4** (new; a quarter of the native budget, which contains it) | **8** |
| `--native-buffer-budget` | 16 | 32 |
| `--js-external-memory-limit` | 32 | 64 |
| `--image-source-budget` / `--image-memory-budget` | 16 / 48 | 48 / 128 |
| `--image-cache-max-memory` | 8 | 16 |
| `--cache-max-memory` | 16 | 32 |
| `--compression-cache` | 8 | 16 |
| `--memory-admission` | on, 85/95, `committed` accounting, TTL 2 s | same |
| `--memory-admission-emergency` | not set until measured | not set |
| `--js-young-semi-space` | not set (RE-5G) | not set until the campaign's GC comparison |

**The prerender cache:**

- Its value is the hot set, not the build size.
- At 0, every request reads its file, normally from the page cache.
  Page-cache pages count in the working set while active, but are
  reclaimable (inactive file is excluded by admission).
- The campaign should compare 0, 4 and 8 on Velasia (whose prerendered
  output is about 6 MB in total) and on the 10 000-route fixture, with
  memory pressure, so that page-cache eviction and cold reads (RE-5B §4.1:
  7.5–9 ms at p50 on this host's disk) are part of the measurement.

## 6. Questions for the campaign

1. `fixed` and `metadata` per release: RSS anon after startup with
   `--prerender-cache 0`, per isolate count.
2. `E_unenforced`, p99 in the memory-intensive and slow-client workloads.
3. Whether one native work budget (§4.3) would have prevented any OOM in
   the image fault workloads.
4. Sampled against committed accounting: the delayed-allocation workload
   decides (RE-4.1B).
