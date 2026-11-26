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
node benchmarks/resource/run.mjs teardown
```

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

## Limits of these measurements

They run on one Docker Desktop VM on a shared Mac. Memory figures are
reliable (the cgroup counts what it counts); CPU per request is
comparable between runs of one series, which interleave variants;
latency and throughput are not authoritative (other work on the host
takes physical cores the VM cannot see: `contention` in each run file
records the host's and the VM's load). The backend is shared and not
limited; SSR and route-handler requests depend on it.
