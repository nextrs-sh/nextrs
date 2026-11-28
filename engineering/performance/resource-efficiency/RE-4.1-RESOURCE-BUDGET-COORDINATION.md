# RE-4.1F: resource budget coordination

NextRS now has about ten independent memory limits. None of them knows
the deployment's memory envelope, and their defaults are sized for
machines with gigabytes. This report:

- says what each limit actually bounds;
- shows where limits overlap, so they are not double-counted;
- proposes how coordinated profiles could work;
- gives **unvalidated** starting points for 256 MiB and 512 MiB.

It is design and documentation only; nothing here is implemented. Source
of the defaults: `crates/nextrs-cli/src/main.rs` (flags and profiles),
`crates/nextrs-runtime/src/lib.rs` (`Limits`).

## 1. What each limit is

| Limit (flag, default) | Bounds | Kind | Counted in the cgroup working set? |
|---|---|---|---|
| V8 heap (`--js-heap-limit`, none; `auto` in balanced / low-memory) | each isolate's JS heap | **capacity ceiling**: V8 terminates the isolate after crossing it; overshoot up to ~3 × 64 MiB per episode (RE-4-V8-HEAP-OVERSHOOT) | yes, as committed pages |
| ArrayBuffer budget (`--js-external-memory-limit`, off) | allocator-made backing stores, all isolates | **actual allocations**, refused at ≥ 64 KiB (RE-4.1-EXTERNAL-MEMORY-COVERAGE) | yes |
| Native buffer budget (`--native-buffer-budget`, off) | static/public file bytes and proxy bodies NextRS holds | **reserved bytes**, strict since RE-4.1A | yes |
| Image source budget (`--image-source-budget`, 512 MiB) | source image bytes held at once | reserved bytes | yes |
| Image memory budget (`--image-memory-budget`, 1 GiB) | decode/transform working memory | **estimated demand** from the image header, reserved before work | yes, as the work allocates |
| Image cache (`--image-cache-max-memory`, 64 MiB) | optimized images in memory | capacity ceiling (eviction) | yes |
| Data / ISR cache (`--cache-max-memory`, 50 MiB) | cache entry bytes | capacity ceiling | yes (plus uncounted transient copies) |
| Compression cache (`--compression-cache`, 32 MiB; 8 in low-memory) | gzip copies | capacity ceiling | yes |
| Queue (`--max-queued` 512; with admission 16 MiB of headers) | waiting requests | count; bytes with admission | yes, small |
| Per request: body 10 MiB, response buffer 64 KiB, fetch response 32 MiB | one request | per-item ceilings | yes, × concurrency |
| Memory admission (`--memory-admission`, off) | decides whether to start requests | **estimated demand** (2 MiB/request) against **measured** working set | it reads the working set |
| Pressure reclaim (`--memory-pressure-reclaim`, off; 85 % in low-memory) | when to collect and trim | trigger on the measured working set | it reads the working set |

Fixed costs no flag controls:

- **Measured on the Fedora host** (results/re4-local, no container):
  - process without an executor: 2.9 MiB anonymous;
  - one isolate: 27.6 MiB anonymous;
  - two isolates: 42.4 MiB anonymous;
  - file-backed pages: about 38 MiB (the binary and V8 snapshot).
- **Not measured:** resident prerenders (fixed after startup, the build's
  size), thread stacks, and glibc retention between trims.

## 2. Overlaps

- **The working set contains everything else.** Admission and reclaim
  compare one measured total. Adding the V8 heap, external, native or
  cache figures to it counts them twice (RE-4-RESOURCE-ACCOUNTING §3).
  Admission's estimates are added only until the memory shows up
  (`committed`) or until the next reading (`sampled`).
- **The ArrayBuffer budget and the V8 heap are disjoint.** Backing stores
  are outside the heap limit. A Buffer counts in the first, its JS
  wrapper object in the second.
- **Native budget, image budgets and caches are disjoint by construction.**
  Each covers different buffers. Their sum is the worst case.
- **The image memory budget is an estimate**, the decoder's own figure,
  not a reservation of real bytes. It can be exceeded by allocator
  overhead.
- **Caches are filled from request buffers.** A compression cache fill
  holds its identity bytes, counted in the native budget since RE-4.1A,
  and then stores only the gzip, counted in the cache.

## 3. Why defaults do not fit small containers

The image defaults alone allow 512 MiB of sources plus 1 GiB of
transformations: six times a 256 MiB container. They are ceilings that
a real deployment rarely approaches. But a burst of large source images
at 256 MiB would be admitted by the image budgets and killed by the
kernel. Memory admission does not see image work, which is native and
not JavaScript, until it shows in the working set.

## 4. Proposal: coordinated resource profiles (future work)

A profile derived from one envelope `E` (the cgroup limit or a flag),
computed once at startup, logged, each value still overridable:

```
fixed     = binary + isolates × per-isolate floor           (measured per release)
heap      = auto heap budget (RE-3), bounded by E − fixed − native share
native    = native buffer budget + image source + image work + caches
headroom  = E × (1 − critical)                              (admission, reclaim)
invariant: fixed + isolates × heap + native + headroom ≤ E
```

- Shares are fractions of what remains after `fixed`, not absolute
  defaults.
- The image budgets would become one native work budget shared by image
  sources, transformations, static/proxy buffers and cache fills. That
  replaces three independent ceilings with one reservation.
- Admission would read the shares, not only the working set: an image
  transformation reserves from the same budget and is visible to it.
- Not done here. It needs the container campaign's per-component
  measurements to set the shares. Implementing it is a global memory
  manager, out of scope for RE-4.1.

## 5. Starting points for small containers (UNVALIDATED)

**Not measured in a container.** These are arithmetic from the RE-3 auto
heap table (RE-3-V8-HEAP-BUDGETING) and the process-level fixed costs
above. They are a starting point for the campaign, not a
recommendation. All are existing flags.

| Setting | 256 MiB, 1 isolate | 512 MiB, 1 isolate |
|---|---|---|
| Profile | `--profile low-memory` (heap `auto` ≈ 136 MiB with cache 8) | `low-memory`, or `balanced` (heap `auto` 268 MiB) |
| `--js-external-memory-limit` | 32 | 64 |
| `--native-buffer-budget` | 16 (whole-read limit 4 MiB, 64 KiB chunks) | 32 |
| `--image-source-budget` | 16 | 48 |
| `--image-memory-budget` | 48 | 128 |
| `--image-cache-max-memory` | 8 | 16 |
| `--cache-max-memory` | 16 | 32 |
| `--compression-cache` | 8 (profile) | 16 |
| `--memory-admission` | on; pressure 85, critical 95; `committed` accounting, TTL 2 s, as the campaign's candidate | same |
| `--memory-admission-emergency` | not set until measured (98 is the first value to try) | not set |
| `--memory-pressure-reclaim` | 85 (profile) | 85 |
| Isolates | 1 (2 clamps the heap to 64 MiB, RE-3) | 1–2 |

The native shares sum to 16 + 16 + 48 + 8 + 16 + 8 = **112 MiB at
256 MiB**. That is a ceiling, reached only if every subsystem is full at
once, next to a 136 MiB heap ceiling and about 30 MiB fixed. The sum of
ceilings is above the envelope, as it is today with any defaults. Making
it fit is the proposal in §4.

## 6. Image processing in small containers

Operators deploying `next/image` into small containers should:

- Lower `--image-source-budget` and `--image-memory-budget` to fractions
  of the container. Each transformation reserves what its header says it
  will allocate. Work larger than the whole budget is served untransformed
  (the original image), not refused, so a small memory budget degrades
  large images, not availability.
- Bound sources: the build's `images.maximumResponseBody` (50 MB by
  default) caps one remote source. Lower it in `next.config` when 50 MB
  sources are not expected.
- Prefer `--image-cache-dir` over memory caching at 256 MiB, so that
  optimized images are not held in RAM.
- Keep `--malloc-mmap-threshold` at its default (1 MiB). Image buffers are
  then returned to the system when freed.

None of these settings are validated at 256 MiB yet. The runbook's fault
workloads include image work only through the Velasia application.
