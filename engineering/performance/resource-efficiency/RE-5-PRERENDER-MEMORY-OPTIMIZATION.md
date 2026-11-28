# RE-5B: prerendered build memory

How NextRS holds prerendered output, how that drove idle memory with
build size, and the opt-in file-backed store that stops it. The change
is `b16dfc3` (store), `1ddb050` (build metadata freed after startup) and
the `generate_etag` temporary in `b16dfc3`. **Default behaviour is
unchanged** unless `--prerender-cache` is given.

Every number below is process level (Fedora 45 host, one isolate, no
container); none is a container measurement.

## 1. What the server held

At startup, `AppState::load` read into memory, as `Bytes`, every
prerendered response it serves itself:

- App Router HTML;
- the RSC payload and every segment prefetch, for pages without ISR or
  Partial Prerendering;
- static Pages Router pages;
- the 404 page.

While doing so it also:

- made a temporary of twice each HTML page (UTF-16 units) to compute its
  ETag;
- kept, for the whole process lifetime, the adapter's parsed `BuildInfo`
  (every prerendered file's path and headers), only to log the build ID.

ISR and PPR pages were never loaded this way: Next's handler serves them
from the shared cache.

## 2. Fixtures

`fixtures/next-prerender-scale` is one `/p/[id]` route with
`generateStaticParams` and `dynamicParams = false`, under a nested
layout. `NEXTRS_PRERENDER_ROUTES` sets the count;
`NEXTRS_PRERENDER_DIST` sets the build directory.

| Payload per route | This fixture | Velasia (median) |
|---|---|---|
| HTML | 29.3 KB | 21.4 KB |
| RSC | 18.5 KB | 22.8 KB (all RSC files) |
| Segments | 3 files, 36.7 KB | included above |
| Total on disk | ≈ 84 KB | ≈ 155 KB |

| Routes | Build time | Size on disk |
|---|---|---|
| 100 | 24 s | 11 MB |
| 1 000 | 17 s | 122 MB |
| 10 000 | 51 s | 1.0 GB |
| 50 000 (exploratory) | 234 s | 4.9 GB |

## 3. Options evaluated

| Option | Result |
|---|---|
| 1. Eager (default) | resident memory proportional to the build: about 86.5 KiB per route |
| 2. Lazy file-backed, no cache (`--prerender-cache 0`) | **implemented**: startup records length and mtime; each request reads its file (page cache) |
| 3. Bounded hot cache (`--prerender-cache N`) | **implemented** in the same store: LRU by bytes, one read per entry at a time (64 striped locks; a slow read delays its stripe, review F4) |
| 4. Shared immutable buffers | already the case: one `Bytes` per body, shared with hyper and the compression cache; there was no duplicate payload copy to remove, only the duplicate metadata (§5) |
| 5. Memory mapping | **rejected**: mapped file pages are charged to the cgroup like page-cache reads, a mapping keeps pages resident while referenced (a slow client pins them), and `Bytes` over an mmap needs `unsafe` lifetime management against truncation (SIGBUS). Reads into short-lived buffers give the same page-cache behaviour without those risks |

## 4. Results

Candidate `2845852…` (commits through `1ddb050`), baseline `54daca7…`
(`8d83d03`). Each row is one run, 1 isolate, `embedded-v8`. Columns:

- **idle:** 5 s after the first 200 from `/p/0`;
- **sustained:** after a sweep of up to 10 000 distinct routes
  (8 concurrent), then 5 s idle;
- **first / warm:** latency of 200 distinct routes, sequential, cold and
  then again;
- **digest:** of the sampled bodies, identical in every mode.

| Routes | Mode | Idle anon (MiB) | Sustained anon | Peak (HWM) | First p50 / p99 (ms) | Warm p50 / p99 (ms) |
|---|---|---|---|---|---|---|
| 100 | eager (baseline) | 27.5 | 28.2 | 64.8 | 2.32 / – | 1.84 / – |
| 100 | eager | 27.5 | 28.4 | 66.6 | 2.05 / 3.68 | 1.70 / 2.78 |
| 100 | cache 0 | **18.7** | 20.0 | 58.4 | 2.52 / 4.01 | 1.94 / 3.97 |
| 100 | cache 32 | 18.7 | 22.5 | 60.9 | 2.54 / 3.21 | 1.84 / 3.98 |
| 1 000 | eager (baseline) | 107.5 | 113.0 | 150.2 | 1.98 / – | 1.67 / – |
| 1 000 | eager | 102.7 | 109.0 | 147.4 | 2.00 / 2.96 | 1.63 / 2.14 |
| 1 000 | cache 0 | **22.7** | 30.5 | 68.9 | 2.24 / 3.86 | 1.78 / 2.92 |
| 1 000 | cache 32 | 22.7 | 57.2 | 95.6 | 2.23 / 3.20 | 1.65 / 2.99 |
| 10 000 | eager (baseline) | 896.4 | 936.6 | 973.9 | 1.98 / – | 1.61 / – |
| 10 000 | eager | 863.3 | 905.3 | 943.8 | 1.95 / 2.99 | 1.61 / 3.18 |
| 10 000 | cache 0 | **63.0** | 108.3 | 146.4 | see §4.1 | see §4.1 |
| 10 000 | cache 32 | 63.1 | 146.8 | 185.5 | 2.19 / 3.07 | 1.60 / 2.64 |
| 50 000 | see §4.1 | | | | | |

- **Idle anonymous memory:**
  - −32 % at 100 routes;
  - −78 % at 1 000;
  - −93 % at 10 000 (863 → 63 MiB).
- **Eager in the candidate** is 4 % below the baseline at 1 000 and
  10 000: the freed `BuildInfo`.
- **The slope** falls from **86.5 KiB per route** (eager) to **4.5 KiB
  per route** (file-backed). That is the per-entry index: path, length,
  mtime and converted headers for 5 artifacts per route, plus the page
  map.
- **Sustained** memory rises above idle by the prerender cache's
  contents (≤ 32 MiB for cache 32) and, in this first run, the
  compression cache (gzip negotiated by Node's fetch; see the correction
  in §4.1).
- **Latency:**
  - a cold read adds about 0.2–0.5 ms at p50 (one blocking-pool read from
    the page cache);
  - warm requests are unchanged within noise.
- **Startup:** the file-backed mode does not read the bodies; see §4.1
  for times on a quiet host.
- **Latency columns:** startup and latency in this table were measured
  while other work shared the host (the 10 000-route cache-0 row was
  disturbed). Use §4.1 for latency and startup.

### 4.1 Quiet-host rerun (10 000 and 50 000 routes)

Final candidate `0b2340e…` (`8017043`), the corrected driver (review
F12):

- identity requests, so the compression cache stays empty;
- the build's files dropped from the page cache (`posix_fadvise`) before
  each run;
- readiness on `/`, not a sampled route.

Mode `cache0r` is `--prerender-cache 0` with `--memory-reclaim-idle 3`.
Raw data: `results/re5/prerender-quiet/`.

| Routes | Mode | Startup | Idle anon (MiB) | Sustained anon | HWM | Cold first p50 (ms) | Warm p50 | Cold sweep (req/s) |
|---|---|---|---|---|---|---|---|---|
| 10 000 | eager | 12.4 s | 864.6 | 866.9 | 935.5 | 1.49 | 1.48 | 5 267 |
| 10 000 | cache 0 | 1.9 s | **64.8** | 67.6 | 135.4 | 8.11 | 1.67 | 173 |
| 10 000 | cache 0 + reclaim | 1.0 s | 60.8 | 63.1 | 135.8 | 7.55 | 1.56 | 174 |
| 10 000 | cache 32 | 1.1 s | 64.7 | 104.0 | 142.1 | 7.48 | 1.47 | 172 |
| 50 000 | eager | **98.8 s** | **4 244.8** | 4 247.3 | 4 445.8 | 1.45 | 1.46 | 5 028 |
| 50 000 | cache 0 | 4.4 s | **244.6** | 247.4 | 447.1 | 9.18 | 1.51 | 114 |
| 50 000 | cache 0 + reclaim | 5.0 s | 240.6 | 243.0 | 446.6 | 7.67 | 1.51 | 154 |
| 50 000 | cache 32 | 4.6 s | 244.8 | 283.1 | 446.5 | 7.89 | 1.46 | 156 |

Body digests are identical across modes at each size.

**Memory and startup:**

- Idle anonymous memory is −92.5 % at 10 000 routes and **−94 % at
  50 000** (4.1 GiB → 245 MiB).
- The per-route index stays at 4.5 KiB per route.
- Startup at 50 000 routes is **4.4 s against 98.8 s**: eager mode reads
  4.9 GB of build files first.

**The cost: cold reads.**

- With the page cache dropped, the first request for a route reads its
  file from disk: **7.5–9.2 ms at p50**, against 1.5 ms when resident.
- A sweep of distinct cold routes runs at 114–174 req/s, against about
  5 000. This host's disk is limited, and the sweep reads 10 000 distinct
  files.
- Once read, files stay in the page cache: warm latency equals eager
  (1.46–1.67 ms).

In a container the page cache is charged to the cgroup and evicted under
memory pressure. A file-backed deployment trades resident anonymous
memory for cold reads after eviction. That trade is what
`--prerender-cache N` tunes: N MiB of hot bodies are never re-read.

**Correction to §4:** the cache-0 "sustained" rise there (+45 MiB at
10 000 routes) was the compression cache. Node's fetch negotiated gzip,
so the default 32 MiB cache filled; it was not glibc retention. With
identity requests the rise is about 3 MiB, and idle reclamation returns
about 4 MiB more.

## 5. Where the remaining growth is

At 10 000 routes, with no executor and `--prerender-cache 0`:

- **before `1ddb050`:** 80 MiB of anonymous memory stayed live, and an
  idle malloc trim did not reduce it;
- **after:** the adapter's copy of the metadata is gone (63 MiB with an
  isolate).

What remains is the server's own index:

- **per artifact:** a `PathBuf`, the response headers as
  `HeaderName`/`HeaderValue`, length and mtime;
- **per page:** status, tags, flags.

Halving it again would mean storing paths relative to the build
directory and sharing identical header sets between artifacts. That is a
possible follow-up; at 4.5 KiB per route it is a second-order term
(45 MiB per 10 000 routes).

**Does build size still drive idle memory growth?**

- **With `--prerender-cache`:** only through this index, about 5 % of
  what it was.
- **Without it (the default):** yes, as before.

## 6. Correctness

`tests/prerender_store.rs`, run in CI on the committed snapshots:

- **2 391 comparisons** of status, every header and every byte between
  the default server and file-backed servers with caches of 0, 4 KiB and
  64 MiB;
- the requests: GET, HEAD, POST (405), gzip (compression cache), RSC,
  segment prefetches, `If-None-Match` (304), 404; cold and warm;
- five builds: App Router, adapter output, deployment ID, Pages Router,
  static params;
- **a negative control:** dropping the ETag on the file-backed path fails
  the test at its first comparison.

Store unit tests:

- LRU bound and order;
- an entry larger than the cache is served and not kept, and its read is
  reserved in the native budget and released with the bytes;
- no cache;
- a changed (same length, later mtime), longer or deleted file is refused;
- 32 concurrent requests for one entry read it once.

**Integrity:**

- A build file changed after startup gets a 500, never other bytes.
- "Changed" means:
  - a different length or modification time;
  - or, since the review (F5), a different inode or ctime. This catches
    `cp -p`, `rsync -a` and image layers, which preserve the
    modification time.
- Once a body is cached, the startup bytes keep being served, as the
  eager server serves them.
- **Deploy new builds to a new directory.** An in-place overwrite turns
  file-backed pages into 500s while the old process drains (review F8).

**Memory accounting with a native buffer budget** (review F3): every
body the store hands out, kept or not, is reserved in
`--native-buffer-budget`. That covers the cache's contents, and evicted
bodies that slow responses still send. A body larger than the whole
budget is read without a reservation, as a documented exception.

**Path security:** every path comes from the build's manifests at
startup, never from a request.

**ISR and invalidation:** unchanged. ISR and PPR pages never go through
the store. A prerender revalidated since the build is still rendered by
Next's handler (`Page::is_revalidated`), before any body is read.

## 7. Recommendation (owner decision)

Make `--prerender-cache` the candidate for the coordinated profiles:

- small values (4–8 MiB, within the native buffer budget) for
  `low-memory`;
- a larger value for `balanced`.

Keep the default eager until the container campaign compares the modes
on Velasia and on the 10 000-route fixture. In a container, page-cache
reads are charged to the cgroup, but they are reclaimable (inactive
file), which admission already excludes.
