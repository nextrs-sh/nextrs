# Resource-efficiency measurements

Memory, CPU and failures of one Velasia Airways storefront under container
limits: NextRS builds, their runtime settings, and the Next.js standalone
Node.js server, on the [Series 1](../series1/README.md) local stack (one
application build for every runtime, the retailing API, a tools container
with the load generator). The native Linux campaign is in
[RE-4-LINUX-BENCHMARK-RUNBOOK](../../engineering/performance/resource-efficiency/RE-4-LINUX-BENCHMARK-RUNBOOK.md).

## What runs

| Container | Role | CPUs (Docker VM) |
|---|---|---|
| `s1-api` | Velasia retailing API (shared backend, not measured) | 6 |
| `s1-sf` | the storefront under test, one at a time, with the run's memory and CPU limits | 2–3 |
| `s1-tools` | load generator (`nextrs-loadgen`), scripts | 4–5 |
| `re-probe` | samples `s1-sf` from outside: host PID and cgroup namespaces, read-only `/sys/fs/cgroup`, `CAP_SYS_PTRACE` for `smaps_rollup` | 4–5 |

On a host with fewer CPUs, set `S1_SF_CPUSET`, `S1_TOOLS_CPUSET` and
`S1_API_CPUSET`: `validate` fails a CPU set that overlaps another or names a
CPU the host does not have (the default API CPU 6 does not exist on a
6-core host).

The probe reads the storefront's cgroup (`memory.current`, `memory.peak`,
`memory.events`, `memory.stat`, `cpu.stat`, `memory.pressure`) and its main
process (`/proc/PID/status`, every 5th sample `smaps_rollup`) once a second
(`sampler.mjs`). Sampling from outside costs the measured container no CPU
and no memory, unlike `docker exec` inside it.

## Images: one build per application

`docker/build-images.sh velasia|fault|all` (needs `OUT` and, for Velasia,
`VELASIA_SRC`; `--dry-run` prints every command):

| Image | From |
|---|---|
| `velasia-web-build:s1` | Velasia's own `build` stage (`Dockerfile.node --target build`): the one `next build` |
| `velasia-node:s1` | `docker/Dockerfile.velasia-node`: Velasia's `runtime` stage, copying from the build image |
| `velasia-nextrs:s1` | `docker/Dockerfile.velasia-nextrs`: Velasia's NextRS stages (rc.2 from npm), copying from the build image |
| `velasia-api:s1` | Velasia's `docker/Dockerfile.api`, unchanged |
| `nextrs-s1:LABEL` | `velasia-nextrs:s1` with only `/usr/local/bin/nextrs` replaced (`series1/run.mjs setup --bin`) |
| `nextrs-fault-build:s1`, `nextrs-fault:node`, `nextrs-fault-base:s1` | the fault app (`fault-app/`): one build, two runtimes (`Dockerfile.fault-build`, `Dockerfile.fault-runtime`) |
| `nextrs-fault:LABEL` | `nextrs-fault-base:s1` plus a NextRS binary (`run.mjs setup-faults --bin`), checked with `nextrs inspect` |

Velasia's two storefront Dockerfiles each run `next build` and share one
output only through the BuildKit cache; here both runtime images copy from
one build image, and the script checks that our runtime stages are still
byte-for-byte Velasia's at the pinned commit. `run.mjs verify-build` then
lists every file of the standalone output in each image (sorted, sha256;
`.next/cache` and the runtimes' version files excluded) and fails unless
`BUILD_ID`, the `.next` hash and the whole-tree hash are the same in every
image (`OUT/build/*-build-identity.json`, `*.sha256`).

Node.js: every image and the tools container use
`node:24.18.0-bookworm-slim` (digest-pinned in Velasia's Dockerfiles), as
the Mac series did; run the harness on Node.js 24.18.0 too (`validate`
warns otherwise). CI pins 22.20.0 for its own jobs only.

## Commands

```bash
node benchmarks/series1/run.mjs setup --loadgen PATH/nextrs-loadgen --bin base=PATH/nextrs-base ...
node benchmarks/resource/run.mjs setup                  # the probe
node benchmarks/resource/run.mjs setup-faults --bin base=PATH/nextrs-base ...   # fault-app images
node benchmarks/resource/run.mjs discover               # every spec, from a running storefront
node benchmarks/resource/run.mjs validate --variants FILE --configs ... --workload ... --out /ABS/DIR \
  --bin base=PATH --commit base=SHA [--gate GATE] [--check-images]
node benchmarks/resource/run.mjs verify-build --variants FILE --out /ABS/DIR
node benchmarks/resource/run.mjs gate --variants FILE --baseline node --reference REF --candidates A,B \
  [--gate-configs 1g:2,256m:1] --bin ... --commit ... --out /ABS/gate
node benchmarks/resource/run.mjs profile --variants FILE --gate /ABS/gate/gate.json --bin ... --commit ... \
  --configs 1g:2,512m:1,256m:1,256m:0.5,128m:0.5 --rounds 2 --workload full --out /ABS/DIR [--dry-run]
node benchmarks/resource/summarize.mjs DIR [--json DIR/summary.json]
node benchmarks/resource/gc-summary.mjs DIR       # runs with --v8-flags --trace-gc and --save-logs
node benchmarks/resource/soak-summary.mjs DIR     # --workload soak runs
node benchmarks/resource/fault-summary.mjs DIR... # --workload faults-* runs
node benchmarks/resource/run.mjs teardown
```

`discover` also writes the Server Action specs (`discover-action.mjs`:
Velasia's booking-lookup form submitted as a browser without JavaScript
submits it; no credentials, so nothing is written anywhere and every
response is the same "not found" page).

### Validation, dry runs and the correctness gate

- **`validate`** starts nothing and writes nothing (Docker is asked only
  with `--check-images`, and then only `docker image inspect`). It checks
  the variants file; that every runtime resolves to `node`, `rc2` or a
  `--bin LABEL=PATH` (an absolute path to an x86-64 ELF executable, whose
  sha256 it prints); that every variant's `args` are flags of the binary's
  commit (`--commit LABEL=SHA`, read statically from the CLI source with
  `git show`, `start-flags.mjs`), with values where needed and no shell
  metacharacters (the start scripts word-split `NEXTRS_EXTRA_ARGS`); the
  configurations and exclusions; the CPU sets; that `--out` is absolute;
  the gate; and the Node.js version. PASS / WARN / FAIL per check, exit 1
  on any FAIL.
- **`--dry-run`** (`profile`, `gate`, `verify-build`, `setup`,
  `setup-faults`): the same checks, then every container command in order,
  the phases and a time estimate. Nothing runs.
- **`gate`** first checks build identity (as `verify-build`), then captures
  every variant (`series1/capture.mjs`, with the variant's flags, at each
  `--gate-configs` configuration) and compares: for every candidate, each
  difference from `--reference` must be one the reference already has
  from `--baseline` (otherwise the gate fails). It writes `gate.json` and
  `gate.md`.
- **`profile` refuses to start** unless `--gate` names a passing
  `gate.json` that covers every variant with the same runtime, args, env
  and image ID (a copy with extra instrumentation names its gated variant
  with `gateAs`). `--no-gate-reason TEXT` overrides it for non-decisive
  runs (the probe check); the reason is recorded in meta. The fault-app
  workloads also need `--fault-build OUT/build/fault-build-identity.json`.

### Variants files

A JSON array of variants:

| Field | Meaning |
|---|---|
| `label`, `runtime` | required; `runtime` is `node`, `rc2` or a NextRS binary label (`nextrs-s1:<label>`) |
| `args` | `nextrs start` flags (`NEXTRS_EXTRA_ARGS`; NextRS only) |
| `env` | `NAME=value` entries (allocator and runtime experiments) |
| `image` | another Velasia image (`faultImage` for the fault app) |
| `isolates` | `NEXTRS_ISOLATES` (default 1) |
| `configs` | allow-list of configuration patterns (`512m:1`, `256m` = any CPUs, `*:0.5`) |
| `excludeConfigs`, `excludeReason` | configurations this variant never runs in, and why (required). Lifted only by `--allow-config LABEL=PATTERN --allow-reason TEXT`, recorded in meta |
| `gateAs` | the gated variant this one is equivalent to for correctness (same runtime, env, image) |

`variants/re4-decisive.json` excludes `re4-admission-young8` from 256 MiB
([RE-4-YOUNG-GENERATION-EXPERIMENT](../../engineering/performance/resource-efficiency/RE-4-YOUNG-GENERATION-EXPERIMENT.md));
`variants/re4-gctrace.json` is its `--trace-gc` copy for the Linux series
(`re23-gctrace.json` names the Mac series' `base` and `re2b` binaries).

### Workloads

| `--workload` | Kind | What |
|---|---|---|
| `full` | continuous | static 200/s, RSC 30/s, SSR 5/s, route handlers 15/s, mixed 30/s, closed-loop mixed, Server Actions 5/s, back to back |
| `memory` | continuous | mixed 30/s, then closed-loop mixed (screening) |
| `continuous --continuous-minutes N` | continuous | mixed 30/s for N minutes (default 30) without idle gaps, 5-minute phases ending in 10 s closed loop |
| `envelope` | ladder | mixed at 15, 30, 45, 60, 80, 100, 120/s (40 s each), stopping at the first rate with failures or under 95 % achieved; then closed loop |
| `load-idle-cycles` (old name `cycles`) | load-idle | three times: mixed 30/s 45 s, closed loop 20 s, **60 s idle**: does memory return after each cycle? |
| `soak --soak-minutes N` | soak | 6-minute blocks: mixed 30/s 210 s, closed loop 30 s, Server Actions 30 s, 90 s idle |
| `faults-*` | fault | see [Fault workloads](#fault-workloads) |

| Option | Meaning |
|---|---|
| `--recovery 10,30,60,120,300` | readings after the load (default shown) |
| `--save-logs` | keep each container's whole log (`*.log`) |

- `--configs` lists `memory:cpus` pairs (`docker run --memory M
  --memory-swap M --cpus C`; no swap).
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
4. the workload's phases;
5. **recovery**: readings 10, 30, 60, 120 and 300 s after the load.

The fixed rates sit below one isolate's saturation on one CPU, so CPU per
request compares equal work. Open-loop latency is measured from the
scheduled send time (coordinated omission accounted for). The mixed
workload interleaves, per 20 requests, 8 static assets, 5 RSC
navigations, 2 prerendered HTML pages, 2 SSR searches and 3 route
handlers (`discover-dynamic.mjs`).

Every load generator run is killed after its duration plus 60 s: it does
not give up on a target that died during a fixed-count run.

### Files per run (raw data is kept)

| File | Content |
|---|---|
| `LABEL-MEM-CPUScpu-rN.samples.jsonl` | every 1-second probe sample, raw (cgroup memory, `memory.stat`, `cpu.stat`, PSI, `/proc/PID/status`, `smaps_rollup`) |
| `….loadgen.jsonl` | every load generator result of the run, untrimmed |
| `….metrics.jsonl` | every NextRS `/metrics.json` reading, whole, plus the native buffer budget from `/metrics` |
| `….json` | the run: phases (load generator results with the check samples cut to 3), cgroup readings, the RE-4 counters, recovery, final state |
| `….log` | with `--save-logs` (always for faults): the container's whole log |
| `meta-*.json` | command, commit, host, Node.js, variants, configurations, exclusions, gate file and its sha256, image IDs |

The raw files are written before the run file. If the harness dies during
a run, the probe's copy of the samples stays in the `nextrs-s1-work`
volume (`/work/re/s1-sf-LABEL-….jsonl`) until a run of the same name.

## What is reported

`summarize.mjs`, per configuration and variant (medians over rounds, range
when rounds differ by more than 10 %):

- runs that were **OOM-killed** or failed readiness, and failed requests
  per run: never dropped;
- container memory (`memory.current`, includes page cache) and its
  anonymous part: idle, peak, after each recovery mark; idle PSS;
- after recovery: V8 heap used and committed, native allocator free bytes
  (NextRS's `/metrics.json`);
- per phase: achieved rate, failures, p50/p99/p99.9, **container CPU per
  successful request**, CPU cores used, peak memory;
- the RE-4 counters per round (admission deferred, rejected, state
  transitions; external memory refused; native buffers refused;
  memory-pressure 503; isolate restarts; heap-limit episodes);
- the configurations a variant was excluded from, with the reason.

## Fault workloads

`profile --workload faults-NAME` runs the faults of the RE-4 runbook §6.
Each case gets a fresh container at the run's limits (default `256m:1`),
the sampler, 15 s to settle, its steps (`fault-client.mjs` in the tools
container: per-status counts, connection errors, health probes and
`/metrics.json` polled during the fault), 30 s of readiness probes, and a
classification:

| Outcome | When |
|---|---|
| **process-fatal** | the container exited or was OOM-killed (`docker inspect`), restarted, its main process changed, it never became ready, or it never answered readiness again (wedged) |
| **contained** | the process lived, and an isolate was replaced (`/metrics.json` restarts or generation) or requests were refused or failed (non-2xx, connection errors, RE-4 refusal counters, a RangeError caught by the route) |
| **prevented** | the process lived and every fault request succeeded |

Node.js has no `/metrics.json`: its outcome comes from HTTP and Docker only.

| Workload | App | Cases (defaults) | Records |
|---|---|---|---|
| `faults-native-memory` | fault | 16 MiB `Buffer.alloc` chunks held, one at a time, up to 2 × the limit; then released | held MiB at the first failure; OOM kill, RangeError or refusals |
| `faults-js-allocation` | fault | one allocation per container: `heap-single`, `heap-string` × 64, 96, 128, 192, 256 MiB | replaced / aborted / OOM-killed, by size |
| `faults-concurrent-ssr` | Velasia | closed loop on the SSR search at 16, 32, 64, 128 connections, 60 s, a container per level | peak memory, restarts, 503 share |
| `faults-large-uploads` | fault | 8 MiB POSTs, 8 and 32 at once, streamed and buffered route | peak memory, 413/503 |
| `faults-slow-streams` | fault | 16 and 64 readers of a 64 MiB stream at 1 KiB/s for 120 s | memory per stalled stream, health meanwhile |
| `faults-queue-saturation` | Velasia | closed loop 30 s (saturation), open loop at 2 × for 60 s, 60 s idle, 0.5 × for 30 s | queue length and bytes, 503 codes, recovery |
| `faults-isolates` | Velasia | `--isolates 2`: mixed 30/s 60 s, closed loop 32 connections 30 s | heap clamp warnings, restarts |
| `faults-pressure-cycles` | Velasia | 10 × (closed loop 60 s, idle 60 s): a load-idle cycle test | does anonymous memory return each cycle |

Overrides: `--fault-levels`, `--fault-sizes`, `--fault-modes`,
`--fault-duration`, `--fault-chunk-mib`, `--fault-cap-mib`,
`--fault-size-mib`, `--fault-rate-bps`, `--fault-factor`,
`--fault-isolates`, `--fault-cycles`, `--fault-load-seconds`,
`--fault-idle-seconds`. Per case: `*.json` (classification, derived
figures), `*.samples.jsonl`, `*.client.jsonl` (every client result,
whole), `*.metrics.jsonl`, `*.log`.

The fault app (`fault-app/`, Next.js 16.4.6, the versions of
`fixtures/next-app`) has route handlers only: `/api/fault/native`,
`/api/fault/heap` (the `heap-faults.js` modes), `/api/fault/upload`,
`/api/fault/stream` and Velasia's readiness path
`/api/internal/health/ready`. Its build needs registry.npmjs.org
(`docker/build-images.sh fault`).

## A soak on a Linux host

The long soak runs on a dedicated Linux machine (x86_64, Docker with
cgroup v2 and the systemd driver; run `preflight.sh` first). The probe
finds the container's cgroup under `docker/ID` (cgroupfs driver) or
`system.slice/docker-ID.scope` (systemd driver).

On Fedora, use Docker CE (not Podman's `docker` compatibility command,
which these drivers have not been run with). Fedora's default is the
systemd driver. Before a long run, check that the probe can read the
storefront's `memory.current` and `cpu.stat`: a one-block `soak` run
(`--soak-minutes 6`) shows them in its samples. Under SELinux enforcing, a
denial there shows as missing memory samples, not as an error. The
sequence is in the RE-4 runbook §4–§7.

## Limits of these measurements

The Mac series ran on one Docker Desktop VM on a shared Mac. Memory
figures are reliable (the cgroup counts what it counts); CPU per request
is comparable between runs of one series, which interleave variants;
latency and throughput are not authoritative there (other work on the host
takes physical cores the VM cannot see: `contention` in each run file
records the host's and the VM's load). On a native Linux host,
`contention` records the host's own `/proc/loadavg`. The backend is shared
and not limited; SSR and route-handler requests depend on it.
