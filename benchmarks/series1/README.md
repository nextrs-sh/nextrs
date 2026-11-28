# Series 1 local reproduction

Controlled, local versions of the two workloads of Velasia Airways' first
Railway benchmark series (2026-11-25): `static-assets` and `rsc`. They
compare `next start`'s server (Next.js standalone `server.js` on Node.js
24.18.0), NextRS 0.1.0-rc.2 and later NextRS builds on **one application
build**. The findings are in
[`engineering/performance/series1/`](../../engineering/performance/series1/).

## What runs

Everything runs in Docker on one host, on one bridge network:

| Container | Image | CPUs | Memory |
|---|---|---|---|
| `s1-api` | Velasia retailing API | CPU 6 (1 CPU) | 768 MiB |
| `s1-sf` (the runtime under test, one at a time) | see below | CPUs 2–3 (`--cpus 2`) | 1 GiB, no swap |
| `s1-tools` (load generator, scripts) | the Node.js image, `nextrs-loadgen` mounted | CPUs 4–5 | |

Series 1 gave each storefront 2 vCPU and 1 GB on Railway; the storefront
gets the same here, on its own two CPUs of the Docker VM. The load
generator never shares a CPU with the server. The CPU sets can be changed
with `S1_SF_CPUSET`, `S1_TOOLS_CPUSET` and `S1_API_CPUSET`.

### Images: one build for every runtime

Velasia Airways at `d04127a` (the commit Series 1 ran):

```bash
git -C velasia-airways worktree add ../velasia-d04127a d04127a
cd velasia-d04127a
docker build -f Dockerfile.node   -t velasia-node:s1 .
docker build -f Dockerfile.nextrs -t velasia-nextrs:s1 .   # NextRS 0.1.0-rc.2 from npm
docker build -f docker/Dockerfile.api -t velasia-api:s1 .
```

The two storefront Dockerfiles have identical `build` stages, so the
second build reuses the first one's `next build` from the layer cache.
Check that both images carry the same `apps/web/.next/BUILD_ID`. The
cache is not a guarantee (a pruned cache, another builder or `--no-cache`
gives each image its own build): the resource harness builds the
application once and copies it into both images instead, and checks every
file
([`../resource/docker/build-images.sh`](../resource/README.md#images-one-build-per-application),
`run.mjs verify-build`).

Other NextRS builds are `velasia-nextrs:s1` with only
`/usr/local/bin/nextrs` replaced (`run.mjs setup --bin LABEL=PATH`), so
every runtime serves byte-identical build output. Build the binaries as
the release does (`cargo build --locked --profile dist -p nextrs-cli`, on
Linux x86_64), from a commit, never from a dirty tree.

`nextrs-loadgen` (`benchmarks/loadgen`) must be a Linux build:
`cargo build --release` in `benchmarks/loadgen` inside a Linux container.

## Commands

```bash
node benchmarks/series1/run.mjs setup --loadgen PATH/nextrs-loadgen \
  --bin head=PATH/nextrs-head --bin candidate=PATH/nextrs-candidate
node benchmarks/series1/run.mjs capture --runtimes node,rc2,head --out DIR
node benchmarks/series1/compare.mjs DIR/capture-node.json DIR/capture-head.json --labels node,head
node benchmarks/series1/run.mjs matrix --runtimes node,rc2,head,candidate --rounds 3 --out DIR
node benchmarks/series1/run.mjs memory --runtimes node,head --isolates 1,2,4 --no-js --out DIR
node benchmarks/series1/run.mjs fills --runtimes node,head,candidate --out DIR
node benchmarks/series1/summarize.mjs DIR
node benchmarks/series1/run.mjs teardown
```

- **`capture`** records every request the workloads make, plus HTML,
  HEAD, conditional and range requests, as each runtime answers them:
  status, caching and encoding headers, wire size and a SHA-256 of the
  decoded body. `compare.mjs` lists every difference.
- **`matrix`**: per runtime and round, a fresh container; 15 s settle,
  30 s idle (idle CPU), every asset and every RSC path once on one
  connection (cold: compression-cache misses, cache seeding), a warm-up,
  then open-loop runs at fixed rates (`--static-rates`, `--rsc-rates`,
  `--seconds` each) and closed-loop runs (16 connections static, 8 RSC),
  then 60 s idle (recovery). Round 1 runs the runtimes in the order given,
  round 2 reversed, and so on.
- **`memory`**: memory by isolate count (1, 2, 4) after start, after
  static and RSC load and after recovery; `--no-js` adds NextRS without an
  executor (no V8 isolate), whose readiness is the first discovered static
  asset, because Velasia's readiness route (and its prerendered pages)
  need JavaScript; `--js none` runs only that case.
- **Contention gate:** `--max-host-load N` (with `matrix`, `memory`,
  `fills`) waits before each measured window until the host's 1-minute
  load average is below N (at most `--gate-wait` seconds; a window measured
  anyway is flagged). `summarize.mjs DIR --max-host-load N` leaves out
  windows at or above N and lists them.
- **`fills`**: compression-cache behaviour on fresh containers: 32
  simultaneous requests for the largest asset, every asset at once on 32
  connections, then a fixed 400 requests/s, with the default cache and
  with a 1 MiB cache (eviction and refill).

## What is measured

Per measured window (`summarize.mjs` explains each column):

- **latency** from the load generator. Open loop is measured from the
  scheduled send time (coordinated-omission aware); closed loop is service
  time. Open-loop and closed-loop results are reported separately and
  never mixed.
- **CPU**: the storefront container's cgroup (`cpu.stat`: usage, user,
  system, throttling) before and after the window, per successful request.
  This counts the whole container, whatever the runtime.
- **memory**: cgroup `memory.current`, `memory.peak` and `memory.stat`
  (anonymous vs file-backed), and the main process's `VmRSS`, `VmHWM`,
  `RssAnon` and `RssFile` (PID 1 in both images). cgroup memory includes
  the page cache of files the container read; process RSS does not.
- **NextRS's own metrics** (`/metrics.json` on its operations listener):
  what answered each request (`via`), executor time, cache bytes copied
  to JavaScript, reused cache entries, streamed-gzip work, heap and malloc.
- **contention**: the macOS host's load average (on Linux, the host's
  `/proc/loadavg`) and the Docker VM's load
  and CPU use around every window. Results from windows where the VM was
  busy with other work must not be compared.
