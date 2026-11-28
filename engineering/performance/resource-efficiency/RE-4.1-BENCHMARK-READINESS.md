# RE-4.1: benchmark readiness

**Status: ready for the owner to run, with conditions.** The native Linux
runbook ([RE-4-LINUX-BENCHMARK-RUNBOOK](RE-4-LINUX-BENCHMARK-RUNBOOK.md))
is corrected, and the harness (`benchmarks/resource/`) now validates a
plan before it starts anything, refuses to time without a passing
correctness gate, builds each application once, keeps every raw
measurement, and has the eight fault workloads.

**No benchmark, no container and no host operation was run.** There is no
Docker daemon in the tools' sandbox and none was used; no package was
installed on the host; no swap, service, context or permission was
changed. Everything below is file edits, static checks, dry runs that
print plans, and unit-level checks of the JavaScript on loopback inside
the sandbox. Branch `fix/re-4.1-resource-controls`, based on `main`
`68927b4`.

## The twelve items

| # | Item | Status | Where |
|---|---|---|---|
| 1 | `OUT` absolute, survives `cd` | **Fixed** | Runbook §2: one env file sets `OUT` and checks it is absolute; `run.mjs` (`validate`, `profile`, `gate`, `verify-build`), `preflight.sh` and `build-images.sh` refuse a relative `--out`/`OUT` |
| 2 | Placeholders parsed as redirections | **Fixed** | `<RE4-COMMIT>` and `<the selected flags>` are gone: `RE4_COMMIT`, `NODE`, `SELECTED` are variables checked with `${VAR:?}`; the soak variants file is selected by label from `re4-decisive.json` with its gated flags, never typed. `validate` also rejects shell metacharacters in any variant's `args` |
| 3 | Relative bind mounts | **Fixed** | Runbook §3: `-v "$wt":/src` with `wt="$(realpath "$NEXTRS_SRC/..")/nextrs-bench-$label"`; the build containers run as the owner's uid with `CARGO_HOME` under the worktree (no root-owned files) |
| 4 | One Next.js build for both images, verified | **Implemented** | `docker/build-images.sh`: Velasia's own `build` stage as `velasia-web-build:s1`, then `docker/Dockerfile.velasia-node` and `Dockerfile.velasia-nextrs` (Velasia's runtime stages at `d04127a`, byte for byte, checked by the script) copy from it. `run.mjs verify-build`: per image, `BUILD_ID`, sha256 over the sorted `.next` file list (`.next/cache` excluded) and over the whole standalone tree; fails on any difference. The gate repeats it for every image a variant uses (`nextrs-s1:*` included). Velasia is not modified |
| 5 | `re4-admission-young8` not at 256 MiB | **Implemented** | `variants/re4-decisive.json`: `excludeConfigs: ["256m"]` with `excludeReason`; the harness skips it (plan, gate and profile), records the skip in meta and the summary. Only `--allow-config LABEL=PATTERN --allow-reason TEXT` lifts it, recorded. Also a `configs` allow-list per variant |
| 6 | Node.js pin and its history | **Fixed** | 24.18.0 everywhere: Velasia's Dockerfiles pin `node:24.18.0-bookworm-slim` by digest (build, Node.js runtime, API, and thus the tools and probe containers); RE-1, RE-1B, RE-23 and the Mac RE-4 work ran 24.18.0. The runbook's "22.20.0 for the images" was CI's pin (`ci.yml`), never a benchmark image's. `NODE_PIN` in the harness; `validate` and `preflight.sh` warn on another version, fail below 22 |
| 7 | Every variant resolves to an image and a binary | **Implemented** | `run.mjs validate` (and every `--dry-run`): runtime → `node`, `rc2` or a `--bin LABEL=PATH` (absolute, regular, executable, x86-64 ELF, sha256 printed); `args` checked against the flags of the binary's commit (`--commit LABEL=SHA`, `start-flags.mjs` reads the clap definitions from `git show`), values, `requires`; the image start scripts' own flags; configurations; CPU sets; absolute `--out`; gate; Node.js. `--check-images` adds `docker image inspect` only. Starts no container |
| 8 | Continuous traffic vs load-idle cycles | **Fixed** | Every workload has a `kind`. `cycles` is renamed `load-idle-cycles` (old name kept as an alias, with a warning): it idles 60 s per cycle and was mislabelled "continuous traffic and recovery". Continuous: `full`, `memory`, new optional `continuous`; ladder: `envelope`; load-idle: `load-idle-cycles`, `faults-pressure-cycles`; soak: load-idle blocks. The runbook separates them in a table |
| 9 | Raw time series kept | **Fixed** | Samples were already raw (`*.samples.jsonl`); now also every load generator result untrimmed (`*.loadgen.jsonl`), every `/metrics.json` reading whole (`*.metrics.jsonl`), fault clients' full results (`*.client.jsonl`), raw written before the run file. Runbook §8 keeps all of it (manifest, archive, sha256) instead of dropping files over 20 MiB; the Mac series committed summaries only (e.g. `results/re23-cycles/` holds meta, log and summaries) |
| 10 | Correctness gate before timing | **Implemented** | `run.mjs gate`: build identity, then a capture of every variant **with its flags** at `--gate-configs` (default 1 GiB; runbook 1 GiB and 256 MiB), fails if a candidate differs from the reference anywhere the reference matched the baseline; writes `gate.json`/`gate.md`. `profile` refuses to start without a passing gate that covers each variant's runtime, args, env and image ID (`gateAs` for `--trace-gc` copies); `--no-gate-reason` only for non-decisive runs, recorded. Fault-app workloads also need its build-identity file |
| 11 | Fault workloads `--workload faults-*` | **Implemented, not run** | `faults.mjs`, `fault-client.mjs`, `fault-app/`: all eight faults of runbook §6, a fresh container per case, classification prevented / contained / process-fatal from HTTP statuses and connection errors, `docker inspect` (Running, OOMKilled, ExitCode, Pid, RestartCount), `/metrics.json` deltas (restarts, generation, heap-limit episodes, admission, external memory, native buffers, queue refusals) and a 30 s readiness window. `--dry-run` prints every container and client command |
| 12 | Native Linux preflight | **Implemented** | `preflight.sh`: read-only; OUT absolute, cgroup2fs, controllers, PSI memory, load, governor, swap, free disk, docker context, `docker info` (cgroup v2, systemd, not Docker Desktop, CPU count = host), images, binaries, Node.js; `--container-check` (separate flag) starts one `--rm --network none` alpine to read `memory.max`. PASS/WARN/FAIL per line, exit 1 on a failure |

## Found on the way (fixed)

| Problem | Effect before | Now |
|---|---|---|
| The harness's `metrics()` kept no RE-4 field | the runbook's comparison of `admission.*`, `external_memory.refused`, `last_heap_limit` had no data | kept, plus `rejected_memory_pressure`, heap-limit episodes, and the native buffer budget (only in the Prometheus text, now read); `summarize.mjs` prints an RE-4 counters table |
| Runbook §6 used `re23-gctrace.json` | names `base`/`re2b`, binaries this series does not build | `variants/re4-gctrace.json` (`rc3`, `re23`, RE-4, with `gateAs`); `validate` fails the old file here |
| Series 1's default `S1_API_CPUSET=6` | CPU 6 does not exist on the 6-core host | `validate` fails a CPU set beyond the host's CPUs or overlapping another |
| `series1 capture` runs every runtime without its flags | a flag could not fail the correctness gate | the gate captures each variant with its flags |
| `contention()` read only macOS `sysctl vm.loadavg` | host load `NaN` on Linux | falls back to `/proc/loadavg` |
| `process.exit()` right after a long dry-run plan | output cut when piped (seen: `… | grep` found nothing) | `process.exitCode`, output drains |
| Build containers ran as root | root-owned `target/` in the worktrees | `--user "$(id -u):$(id -g)"` |

## Validation evidence

All with Node.js 24.18.0 (`/home/dennispaler/.cache/nextrs-phase3a/node-v24.18.0-linux-x64/bin/node`).

| Check | Result |
|---|---|
| `node --check` on every `.mjs` in `benchmarks/resource` and `benchmarks/series1` | all pass |
| `bash -n preflight.sh`, `bash -n docker/build-images.sh`, `sh -n fault-app/docker-start.sh`, the runbook's env file | pass. `shellcheck` is not installed in the sandbox: **not run** |
| `start-flags.mjs` at `0e1ec49`, `c65731b`, `HEAD` | 50, 54, 64 `nextrs start` flags; `--memory-admission` absent before RE-4, `--memory-reclaim-idle` absent at rc.3 |
| Flag and config unit checks (18) | all pass: RE-4 flag on the `re23` commit, `<…>` text, missing value, bool with value, `requires`, `--v8-flags --trace-gc`, `$X`, config patterns, exclusion, lift, allow-list |
| `validate` on `re23-gctrace.json` with a relative `--out` | 5 FAIL (3 runtimes without a binary, relative out, no gate), exit 1 |
| `profile --dry-run`, `re4-decisive.json`, `full`, 256m/512m/1g, 3 rounds | 42 runs planned, `re4-admission-young8` skipped at 256 MiB in each round, about 8.5 h; exit 1 without a gate, exit 0 with a passing gate fixture |
| Gate fixtures | passing fixture: PASS; changed args and a failed candidate: 2 FAIL; `--trace-gc` copies pass through `gateAs`; capture-diff rule on synthetic captures: a known header difference is allowed, a new body/status difference fails |
| `gate --dry-run` | build-identity commands for 4 images, 9 captures (young8 skipped at 256 MiB); without `--reference`: FAIL, exit 1 |
| `verify-build` hash script on two copies of a real standalone build | same hash (1277 files) despite different `.next/cache` and version files; one changed file changes it |
| `build-images.sh --dry-run all` | 9 commands printed; FAIL for "docker is not available" and the absent `VELASIA_SRC`, exit 1; relative `OUT`: exit 2 |
| Velasia `d04127a` Dockerfiles | read from its object store with a read-only reader (git commands on that repository are blocked in the sandbox); identical to its `main` working tree; our runtime stages `diff`-identical from the runtime `FROM` line on |
| Fault dry runs, 1 round, 256m:1 | cases: native-memory 4, js-allocation 40, concurrent-ssr 16, large-uploads 16, slow-streams 8, queue-saturation 4, isolates 4, pressure-cycles 4 (young8 excluded); 3 rounds about 14 h |
| `classify()` on 11 synthetic runs | all as expected: OOM, abort (134), wedged, process replaced, not ready → process-fatal; isolate replaced, 503s, caught RangeError, external refusals → contained; 503 in a measuring step ignored; Node.js without metrics → prevented |
| `fault-client.mjs`, all six modes, against a loopback stand-in server | status tallies, stop-after-fails, open-loop scheduling, streamed uploads, 1 KiB/s readers, health series, dead server (ECONNREFUSED) all reported; two bugs fixed (a field shadowed by the background series; errors after the client's own hang-up counted) |
| Fault app build | `npm ci --offline` from the local npm cache and `next build` (Next.js 16.4.6) in the scratch directory: all 5 routes built, TypeScript passes |
| Fault app, functional smoke on loopback (not a measurement) | Node.js 24.18.0 standalone: every route answers; `heap-single` 160 MiB under `--max-old-space-size=96` aborts Node.js (exit 134; the client sees ECONNRESET). A local NextRS build (`target/release`, `0.1.0-rc.3`, commit unknown, not the RE-4 binary): `nextrs inspect` accepts the app; with `--js-external-memory-limit 32` the second 16 MiB buffer is refused inside JavaScript (RangeError → 500, `external_memory.refused` 8); `heap-single` 160 MiB with `--js-heap-limit 96` → 502, isolate replaced (generation 2, `heap_limit` failure 1), ready again |
| `summarize.mjs`, `fault-summary.mjs` on synthetic directories | tables, RE-4 counters, exclusions and outcome tallies print |
| `preflight.sh` in the sandbox (expected to fail) | 11 FAIL, 2 PASS, exit 1: `/sys/fs/cgroup` unreadable, PSI memory 0.30, load 27.99, 5152 MiB swap in use, docker CLI absent, binaries absent; Node.js and governor pass. Relative `OUT`: FAIL |
| Runbook env file | unset `RE4_COMMIT`, relative or empty `OUT` each stop it with a message; a valid one yields the `BINS` array |

## What remains for the owner

1. §1: install the native engine; §2: write `~/re4-linux.env` (set
   `RE4_COMMIT` and `NODE`), run `preflight.sh` and
   `preflight.sh --container-check` until they pass.
2. §3: build the binaries and run `build-images.sh all`; keep
   `$OUT/build/*-build-identity.json` (must be `"passed": true`).
3. §4: setup, `validate --check-images`, the probe check.
4. §5: the gate; stop if it fails.
5. §6, §7, §8 as written; about 45 h of exclusive time by the dry-run
   estimates (full 8.5 h, envelope 5.4 h, load-idle cycles 5.9 h, gctrace
   1 h, faults 14 h at 3 rounds, soaks 8.5 h), plus builds.

Not validated here (each needs Docker or the host): the probe's cgroup
path under the systemd driver; `docker inspect`'s `RestartCount` format
string; BuildKit resolving `FROM ${BUILD_IMAGE}` from the local image
store (needs the default `docker` builder) and `COPY --chmod`; the
containerised Rust builds with `--user` (rusty_v8 download, `CARGO_HOME`
in the worktree); every fault driver end to end, including how Velasia's
specs behave under `fault-client` rather than `nextrs-loadgen`; the time
estimates; `shellcheck`.

## Owner decisions

| Decision | Options |
|---|---|
| `RE4_COMMIT` | the RE-4 merge on `main`, or this branch's head after it merges |
| Fault rounds | 3 (about 14 h) or fewer for a first pass |
| Gate configurations | 1 GiB and 256 MiB as written, or 1 GiB only (shorter; a flag that changes responses only under a limit would pass) |
| Young generation | whether to add `--js-young-semi-space 16` at 512 MiB and 1 GiB, as the experiment plan lists; whether `young8` runs at 256 MiB after the 512 MiB results (`--allow-config`) |
| `VELASIA_TODAY` | pin the build's "today" for reproducible pages, or leave it unset as the Mac images were (recorded either way) |
| `continuous` workload | optional sustained-load runs (30 min × 9 cells × 3 rounds is about 17 h); not in the decisive set |
| Raw files over 20 MiB | in git compressed, or in the archive only with its sha256 recorded |
