# Resource-efficiency measurements

Memory, CPU and failures of one Velasia Airways storefront under container
limits: NextRS builds, their runtime settings, and the Next.js standalone
Node.js server, on the [Series 1](../series1/README.md) local stack (one
application build for every runtime, the retailing API, a tools container
with the load generator).

## What runs

| Container | Role | CPUs (Docker VM) |
|---|---|---|
| `s1-api` | Velasia retailing API (shared backend, not measured) | 6 |
| `s1-sf` | the storefront under test, one at a time, with the run's memory and CPU limits | 2–3 |
| `s1-tools` | load generator (`nextrs-loadgen`), scripts | 4–5 |
| `re-probe` | samples `s1-sf` from outside: host PID and cgroup namespaces, read-only `/sys/fs/cgroup`, `CAP_SYS_PTRACE` for `smaps_rollup` | 4–5 |

The probe reads the storefront's cgroup (`memory.current`, `memory.peak`,
`memory.events`, `memory.stat`, `cpu.stat`, `memory.pressure`) and its main
process (`/proc/PID/status`, every 5th sample `smaps_rollup`) once a second
(`sampler.mjs`). Sampling from outside costs the measured container no CPU
and no memory, unlike `docker exec` inside it.

## Commands

```bash
node benchmarks/series1/run.mjs setup --loadgen PATH/nextrs-loadgen --bin base=PATH/nextrs-base ...
node benchmarks/resource/run.mjs setup                  # the probe
node benchmarks/resource/run.mjs discover               # every spec, from a running storefront
node benchmarks/resource/run.mjs profile --variants benchmarks/resource/variants/baseline.json \
  --configs 1g:2,512m:1,256m:1,256m:0.5,128m:0.5 --rounds 2 --workload full --out DIR
node benchmarks/resource/summarize.mjs DIR [--json DIR/summary.json]
node benchmarks/resource/gc-summary.mjs DIR       # runs with --v8-flags --trace-gc and --save-logs
node benchmarks/resource/soak-summary.mjs DIR     # --workload soak runs
node benchmarks/resource/run.mjs teardown
```

`discover` also writes the Server Action specs (`discover-action.mjs`:
Velasia's booking-lookup form submitted as a browser without JavaScript
submits it; no credentials, so nothing is written anywhere and every
response is the same "not found" page).

| Option | Meaning |
|---|---|
| `--workload full` | static 200/s, RSC 30/s, SSR 5/s, route handlers 15/s, mixed 30/s, closed-loop mixed, Server Actions 5/s |
| `--workload memory` | mixed 30/s, then closed-loop mixed (screening) |
| `--workload cycles` | three times: mixed 30/s 45 s, closed loop 20 s, 60 s idle |
| `--workload envelope` | mixed at 15, 30, 45, 60, 80, 100, 120/s (40 s each), stopping at the first rate with failures or under 95 % achieved; then closed loop |
| `--workload soak --soak-minutes N` | 6-minute blocks: mixed 30/s 210 s, closed loop 30 s, Server Actions 30 s, 90 s idle |
| `--recovery 10,30,60,120,300` | readings after the load (default shown) |
| `--save-logs` | keep each container's whole log (`*.log`) |

- `--configs` lists `memory:cpus` pairs (`docker run --memory M
  --memory-swap M --cpus C`; no swap).
- A variants file lists what to compare: a runtime (`node`, `rc2` or a
  NextRS binary label built by `series1 setup --bin`), `nextrs start` flags
  (`args`, passed as `NEXTRS_EXTRA_ARGS`), environment (`env`, e.g.
  `MALLOC_ARENA_MAX=2`, `LD_PRELOAD` of another allocator) and an image
  (`image`). See `variants/`.
- `--rounds` alternates the variant order between rounds; `--first-round
  N` continues a series.

## One run

A fresh container per (variant, configuration, round):

1. readiness (at most 120 s; failing it is recorded with the container's
   state and log tail), then 30 s idle;
2. **correctness**: every SSR search and route-handler request once, with
   identity encoding and body checks (`ssr-identity.json`,
   `api-identity.json`): flight results must be present;
3. **cold**: every request of the mixed workload once, one connection;
4. the workload: `full` runs static assets (200/s), RSC navigations
   (30/s), SSR searches (5/s), route handlers (15/s), the mixed workload
   (30/s) at fixed open-loop rates, then the mixed workload closed-loop on
   16 connections per isolate (saturation); `memory` runs only the mixed
   workload at 30/s and the saturation phase;
5. **recovery**: readings 10, 30, 60 and 120 s after the load.

The fixed rates sit below one isolate's saturation on one CPU, so CPU per
request compares equal work. Open-loop latency is measured from the
scheduled send time (coordinated omission accounted for). The mixed
workload interleaves, per 20 requests, 8 static assets, 5 RSC
navigations, 2 prerendered HTML pages, 2 SSR searches and 3 route
handlers (`discover-dynamic.mjs`).

Every load generator run is killed after its duration plus 60 s: it does
not give up on a target that died during a fixed-count run.

## What is reported

`summarize.mjs`, per configuration and variant (medians over rounds, range
when rounds differ by more than 10 %):

- runs that were **OOM-killed** or failed readiness, and failed requests
  per run: never dropped;
- container memory (`memory.current`, includes page cache) and its
  anonymous part: idle, peak, after 10, 60 and 120 s of recovery; idle PSS;
- after recovery: V8 heap used and committed, native allocator free bytes
  (NextRS's `/metrics.json`);
- per phase: achieved rate, failures, p50/p99/p99.9, **container CPU per
  successful request**, CPU cores used, peak memory.

## A soak on a Linux host

The long soak runs on a dedicated Linux machine (x86_64, Docker with
cgroup v2, ideally 8 or more CPUs; otherwise set `S1_SF_CPUSET`,
`S1_TOOLS_CPUSET`, `S1_API_CPUSET`). The probe finds the container's
cgroup under `docker/ID` (cgroupfs driver) or `system.slice/docker-ID.scope`
(systemd driver).

On Fedora, use Docker CE (not Podman's `docker` compatibility command,
which these drivers have not been run with). Fedora's default is the
systemd driver. Before a long run, check that the probe can read the
storefront's `memory.current` and `cpu.stat`: a one-block `soak` run
(`--soak-minutes 6`) shows them in its samples. Under SELinux enforcing, a
denial there shows as missing memory samples, not as an error.

```bash
# where the images were built
docker save velasia-node:s1 velasia-nextrs:s1 velasia-api:s1 | gzip > velasia-s1.tar.gz
# on the Linux host, with this repository checked out and Node.js 22+
docker load < velasia-s1.tar.gz
node benchmarks/series1/run.mjs setup --loadgen PATH/nextrs-loadgen --bin cand=PATH/nextrs
node benchmarks/resource/run.mjs setup
node benchmarks/resource/run.mjs discover --runtime cand
echo '[{"label":"cand","runtime":"cand","args":"--js-heap-limit auto --memory-reclaim-idle 10 --memory-pressure-reclaim 90"}]' > soak.json
node benchmarks/resource/run.mjs profile --variants soak.json --workload soak --soak-minutes 360 \
  --configs 256m:1 --rounds 1 --out results/soak-6h
node benchmarks/resource/soak-summary.mjs results/soak-6h
```

## Limits of these measurements

They run on one Docker Desktop VM on a shared Mac. Memory figures are
reliable (the cgroup counts what it counts); CPU per request is
comparable between runs of one series, which interleave variants;
latency and throughput are not authoritative (other work on the host
takes physical cores the VM cannot see: `contention` in each run file
records the host's and the VM's load). The backend is shared and not
limited; SSR and route-handler requests depend on it.
