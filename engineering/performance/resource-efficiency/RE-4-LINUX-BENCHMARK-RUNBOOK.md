# RE-4: native Fedora benchmark runbook

For the owner, to run from a **host terminal** on the Fedora machine
recorded in [RE-23B-LINUX-ENVIRONMENT](RE-23B-LINUX-ENVIRONMENT.md)
(i5-9400F, 6 cores, 15.5 GiB, HDD, Fedora 45). The tools' sandbox cannot
run these steps: they install packages, start a system daemon and read
`/sys/fs/cgroup`. Nothing here was run by the tools (RE-4.1 checked the
commands statically and with dry runs only:
[RE-4.1-BENCHMARK-READINESS](RE-4.1-BENCHMARK-READINESS.md)). Each step
says what it changes on the host.

Results from this host form a **separate Linux series**. Never pool or
compare them as one series with the Mac Docker Desktop results.

## 0. What you need to decide first

| Decision | Recommendation |
|---|---|
| Container engine | Native Docker Engine (Docker CE, cgroup v2, systemd driver), as the harness README requires. Not Docker Desktop (it runs a VM); not Podman's `docker` shim (the drivers were never run with it) |
| Images | Rebuilt on Linux from pinned commits (§3), as approved in place of moving the Mac images, with **one** `next build` per application copied into every runtime image |
| RE-4 commit | `RE4_COMMIT`: the RE-4 merge commit on `main` (or the head of the branch under test), as a full SHA |
| Node.js | 24.18.0 everywhere (images, tools container, harness): what the Mac RE-1 to RE-4 series ran (§3) |
| Fault rounds | 3 rounds per fault cell (about 14 h, §6) or fewer for a first pass; a classification from one round is a finding, not a rate |
| `re4-admission-young8` at 256 MiB | excluded in the variants file; run there only if the 512 MiB results justify it (§6) |
| Exclusive time | About 45 h for §5 to §7 (dry-run estimates), sequential, with the desktop session logged out |

## 1. Native Docker Engine (changes the host: packages, a system service)

```bash
# Stop and disable Docker Desktop so its socket and context are not used.
systemctl --user stop docker-desktop || true
systemctl --user disable docker-desktop || true
docker context use default 2>/dev/null || true

# Docker CE from Docker's Fedora repository.
sudo dnf -y install dnf-plugins-core
sudo dnf config-manager addrepo --from-repofile=https://download.docker.com/linux/fedora/docker-ce.repo
sudo dnf -y install docker-ce docker-ce-cli containerd.io docker-buildx-plugin
# If Docker's repository has no Fedora 45 build yet: Fedora's own
# `sudo dnf -y install moby-engine` is the same engine (record which one).
sudo systemctl enable --now docker

# Run docker without sudo. Membership in "docker" is root-equivalent:
# skip this and prefix the commands with sudo if you prefer.
sudo usermod -aG docker "$USER"   # log out and back in
```

Undo (§9): `sudo systemctl disable --now docker; sudo dnf remove docker-ce docker-ce-cli containerd.io docker-buildx-plugin`.

## 2. Session variables and host checks

### Variables (changes the host: one file in your home directory)

Every later block needs these, in every terminal. Write them once, then
`source` the file at the start of each session. Each value is set
explicitly; the checks make every block fail at once if one is missing,
instead of running with an empty or relative path.

```bash
cat > ~/re4-linux.env <<'EOF'
export NEXTRS_SRC="$HOME/Documents/OpenSource/nextrs"              # this repository
export VELASIA_REPO="$HOME/Documents/OpenSource/velasia-airways"
export VELASIA_SRC="$HOME/Documents/OpenSource/velasia-d04127a"    # made in §3
export OUT="$HOME/Documents/OpenSource/re4-linux/linux-20261201"   # absolute; one per series
export RE4_COMMIT=0000000000000000000000000000000000000000         # replace: full SHA of the RE-4 build
export NODE="$HOME/.local/node-v24.18.0-linux-x64/bin/node"        # replace: Node.js 24.18.0 (§3)
export S1_SF_CPUSET=2-3 S1_TOOLS_CPUSET=4-5 S1_API_CPUSET=0-1     # 6 cores: storefront, tools+probe, API
: "${NEXTRS_SRC:?}" "${VELASIA_REPO:?}" "${VELASIA_SRC:?}" "${OUT:?}" "${RE4_COMMIT:?}" "${NODE:?}"
case "$OUT$VELASIA_SRC$NEXTRS_SRC" in *'~'*) echo "use \$HOME, not ~" >&2; return 1 ;; esac
for p in "$OUT" "$VELASIA_SRC" "$NEXTRS_SRC" "$NODE"; do
  case "$p" in /*) ;; *) echo "not absolute: $p" >&2; return 1 ;; esac
done
[ "$RE4_COMMIT" != 0000000000000000000000000000000000000000 ] || { echo "set RE4_COMMIT" >&2; return 1; }
# The binary and flags arguments every harness command below takes.
BINS=(--bin "rc3=$OUT/nextrs-rc3" --bin "re23=$OUT/nextrs-re23" --bin "re4=$OUT/nextrs-re4"
      --commit rc3=0e1ec49 --commit re23=c65731b --commit "re4=$RE4_COMMIT")
EOF
"${EDITOR:-vi}" ~/re4-linux.env    # set RE4_COMMIT and NODE
source ~/re4-linux.env && mkdir -p "$OUT"
```

### Preflight (read-only)

```bash
source ~/re4-linux.env
"$NEXTRS_SRC/benchmarks/resource/preflight.sh"                     # after §1; binaries and images FAIL until §3-§4
"$NEXTRS_SRC/benchmarks/resource/preflight.sh" --container-check   # also starts one throwaway alpine container
```

One line per check, exit 1 on any FAIL: `OUT` absolute; `/sys/fs/cgroup`
is `cgroup2fs` with the cpu, cpuset, memory, io and pids controllers;
`memory some avg60` 0.00; 1-minute load under 0.5; governor `performance`
on every core; no swap in use; 20 GiB free under `OUT`; docker context not
`desktop-linux`; `docker info`: cgroup v2, systemd driver, not Docker
Desktop, as many CPUs as the host; the images and binaries present;
`$NODE` is 24.18.0. With `--container-check`, a 256 MiB container must
see `memory.max` 268435456. The script changes nothing (the optional
container is `--rm --network none`).

Host contention, recorded into the series directory before and after:

```bash
source ~/re4-linux.env
{ date -Is; uname -a; uptime; free -m; swapon --show; cat /proc/pressure/cpu /proc/pressure/memory /proc/pressure/io
  cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort | uniq -c
  ps -eo pid,pcpu,pmem,comm --sort=-pcpu | head -15
  "$NEXTRS_SRC/benchmarks/resource/preflight.sh"; } > "$OUT/host-before.txt" 2>&1
```

Conditions to start a measured run (preflight checks each):

- Log out of the desktop session and run from a text console (Ctrl+Alt+F3)
  or over SSH.
- `uptime` load average below 0.5 after 5 minutes idle.
- CPU governor `performance` on all cores (it already is).
- Swap: nothing in use. To clear zram:
  `sudo swapoff /dev/zram0 && sudo swapon /dev/zram0` (needs enough free
  RAM). Or disable swap for the series (`sudo swapoff -a`) and record that
  you did.
- `memory some avg60` in `/proc/pressure/memory` at 0.00.

## 3. Reproducible images and binaries

Pinned commits:

| Item | Commit | Notes |
|---|---|---|
| Velasia Airways | `d04127a` | the Series 1 application, same for every runtime |
| NextRS rc.3 baseline (`rc3`) | `0e1ec49` | RE-1 baseline. Serving code identical to `91a7729`; the Mac binary's SHA-256 was `af994c15…48a2` |
| NextRS RE-2/3 (`re23`) | `c65731b` | PR #51 merged |
| NextRS RE-4 (`re4`) | `$RE4_COMMIT` | |
| Rust | 1.98.0 | CI's pin |
| Node.js | **24.18.0** | see below |

**Node.js.** Velasia's Dockerfiles at `d04127a` pin
`node:24.18.0-bookworm-slim@sha256:6f7b03f7…1452d` for the build stage, the
Node.js runtime image and the API image; the tools and probe containers run
from the Node.js image, so they are 24.18.0 too. RE-1, RE-1B, RE-23 and the
Mac RE-4 work all ran Node.js 24.18.0 (RE-1B: `velasia-node:s1` "Node.js
24.18.0, Next.js 16.4.6 standalone"). This runbook previously said 22.20.0
for the images: that is CI's pin for its own jobs (`ci.yml`), not what any
benchmark image ran. The harness itself (on the host) should also run
24.18.0, the official nodejs.org tarball (verify its SHASUMS256) unpacked
under `$HOME/.local` (changes the host: one directory); `validate` warns on
any other version, and fails below 22.

The host has no C toolchain, so build the binaries in a container (changes
the host: three git worktrees beside the repository, Docker images, the
cargo registry cache under each worktree; downloads the prebuilt
`rusty_v8` 150.4.0 archive):

```bash
source ~/re4-linux.env
cd "$NEXTRS_SRC"
parent=$(realpath "$NEXTRS_SRC/..")
for pair in rc3=0e1ec49 re23=c65731b "re4=$RE4_COMMIT"; do
  label=${pair%%=*}; commit=${pair#*=}
  wt="$parent/nextrs-bench-$label"                       # absolute: valid as a bind mount
  git -C "$NEXTRS_SRC" worktree add -f "$wt" "$commit"
  docker run --rm --user "$(id -u):$(id -g)" -e CARGO_HOME=/src/target/cargo-home \
    -v "$wt":/src -w /src rust:1.98.0-bookworm \
    cargo build --locked --profile dist -p nextrs-cli
  cp "$wt/target/dist/nextrs" "$OUT/nextrs-$label"
  echo "$label $(git -C "$wt" rev-parse HEAD)" >> "$OUT/binaries.commits"
done
docker run --rm --user "$(id -u):$(id -g)" -e CARGO_HOME=/src/target/cargo-home \
  -v "$NEXTRS_SRC/benchmarks/loadgen":/src -w /src rust:1.98.0-bookworm cargo build --release
cp "$NEXTRS_SRC/benchmarks/loadgen/target/release/nextrs-loadgen" "$OUT/"
sha256sum "$OUT"/nextrs-* > "$OUT/binaries.sha256"
```

**Images: one `next build` per application** (changes the host: one git
worktree of Velasia, Docker images; downloads base images and npm
packages). Velasia's `Dockerfile.node` and `Dockerfile.nextrs` each run
`next build` in identical stages and share one output only through the
BuildKit cache, which nothing guarantees. Instead, Velasia's own `build`
stage becomes an image (`velasia-web-build:s1`) and both runtime images
copy from it (`benchmarks/resource/docker/`: Velasia's runtime stages,
byte for byte; the script checks that against `$VELASIA_SRC`). Velasia is
not modified.

```bash
source ~/re4-linux.env
git -C "$VELASIA_REPO" worktree add "$VELASIA_SRC" d04127a
cd "$NEXTRS_SRC"
benchmarks/resource/docker/build-images.sh --dry-run all     # prints every command; nothing runs
benchmarks/resource/docker/build-images.sh all               # builds, then run.mjs verify-build
cat "$OUT/images.txt" "$OUT"/build/*-build-identity.json
```

Use Docker CE's default builder (`docker buildx ls`: `default`, driver
`docker`): the runtime Dockerfiles start `FROM` the local build image, which
a `docker-container` builder cannot see (it would try to pull it).
`build-images.sh` fails unless `$VELASIA_SRC` is a clean checkout of
`d04127a`, and `verify-build` fails unless every image (the build image,
`velasia-node:s1`, `velasia-nextrs:s1`; and for the fault app
`nextrs-fault-build:s1`, `nextrs-fault:node`, `nextrs-fault-base:s1`) has
the same `BUILD_ID`, the same sha256 over the sorted list of `.next`
files (`.next/cache` excluded) and the same hash over the whole
standalone tree (the runtimes' version files excluded). The per-file
lists are in `$OUT/build/*.sha256`. Optional: `VELASIA_TODAY=YYYY-MM-DD`
pins "today" in the build (recorded in `images.txt`).

The NextRS builds (`nextrs-s1:LABEL`, `nextrs-fault:LABEL`) are built in §4
and checked again by the gate (§5), which hashes every image a variant
uses.

Record in `$OUT/differences.md` everything that differs from the Mac
series: the host, the engine, the rebuilt binaries (new hashes; the Mac
`af994c15…` and `052d741d…` binaries are not reproduced bit for bit), the
image IDs, the new `BUILD_ID` (the Mac build was `gvWJrvYSM71x059u5PMn2`;
a rebuild gets a new one), the image assembly (one build image copied into
both runtime images, instead of a shared cache), the Debian packages the
runtime stages' `apt-get upgrade` installed on the build day, and the
base-image digests (pinned, so unchanged).

## 4. Harness setup

Changes the host: Docker network `nextrs-s1`, volumes `nextrs-s1-work` and
`nextrs-s1-api-data`, containers `s1-api`, `s1-tools`, `re-probe`, images
`nextrs-s1:{rc3,re23,re4}` and `nextrs-fault:{rc3,re23,re4}`.

```bash
source ~/re4-linux.env
cd "$NEXTRS_SRC"
V="$NEXTRS_SRC/benchmarks/resource/variants/re4-decisive.json"
"$NODE" benchmarks/resource/run.mjs validate --variants "$V" "${BINS[@]}" --loadgen "$OUT/nextrs-loadgen" \
  --configs 256m:1,512m:1,1g:2 --workload full --out "$OUT/decisive-full" --for validate
"$NODE" benchmarks/series1/run.mjs setup --loadgen "$OUT/nextrs-loadgen" \
  --bin rc3="$OUT/nextrs-rc3" --bin re23="$OUT/nextrs-re23" --bin re4="$OUT/nextrs-re4"
"$NODE" benchmarks/resource/run.mjs setup
"$NODE" benchmarks/resource/run.mjs setup-faults --bin rc3="$OUT/nextrs-rc3" --bin re23="$OUT/nextrs-re23" --bin re4="$OUT/nextrs-re4"
"$NODE" benchmarks/resource/run.mjs discover --runtime re4
"$NODE" benchmarks/resource/run.mjs validate --variants "$V" "${BINS[@]}" --check-images \
  --configs 256m:1 --workload full --out "$OUT/decisive-full" --for validate
# The probe must read the storefront's cgroup (SELinux denials show as
# missing samples, not errors). Not a measurement, so it needs no gate:
"$NODE" benchmarks/resource/run.mjs profile --variants "$V" --only re4-admission "${BINS[@]}" \
  --workload soak --soak-minutes 6 --configs 256m:1 --rounds 1 --out "$OUT/probe-check" \
  --no-gate-reason "probe check before the gate"
ls -l "$OUT"/probe-check/*.samples.jsonl
head -c 600 "$(ls "$OUT"/probe-check/*.samples.jsonl | head -1)"
# every line must carry memory_current, stat.anon and cpu.usage_usec
```

`validate` (read-only; `--check-images` adds `docker image inspect`)
prints PASS / WARN / FAIL for: every variant's runtime has a `--bin`
(absolute, x86-64 ELF, sha256 printed) or is `node`; its `args` are flags
of that commit's `nextrs start` (read from git), with values, without shell
metacharacters; configurations; exclusions; CPU sets (no overlap, none
beyond CPU 5); absolute `--out`; Node.js 24.18.0.

## 5. Correctness gate (before any timing)

Changes the host: containers started and removed one at a time, files
under `$OUT/gate`.

```bash
source ~/re4-linux.env
cd "$NEXTRS_SRC"
V="$NEXTRS_SRC/benchmarks/resource/variants/re4-decisive.json"
G=(gate --variants "$V" --baseline node --reference re23 --candidates re4-admission,re4-admission-young8
   --gate-configs 1g:2,256m:1 "${BINS[@]}" --out "$OUT/gate")
"$NODE" benchmarks/resource/run.mjs "${G[@]}" --dry-run
"$NODE" benchmarks/resource/run.mjs "${G[@]}"
cat "$OUT/gate/gate.md"
```

The gate checks build identity in every image the variants use, then
captures every variant **with its flags** at 1 GiB / 2 vCPU and at
256 MiB / 1 vCPU (`series1/capture.mjs`: every request of the workloads,
HTML, HEAD, conditional and range requests; status, headers, decoded-body
sha256) and compares them. It fails, and writes `"passed": false`, if a
candidate differs from `re23` anywhere `re23` did not already differ from
`node`. (`series1 capture` alone runs every runtime without its flags and
cannot gate a flag.)

**Every `profile` below refuses to start** unless `--gate "$OUT/gate/gate.json"`
passed and covers each variant with the same runtime, args, env and image
ID (an image rebuilt after the gate fails it). `re4-gctrace.json` names
its gated twins with `gateAs`. Stop here if the gate fails.

## 6. Decisive campaign

Variants (`benchmarks/resource/variants/re4-decisive.json`):

| Label | Binary | Flags | Configurations |
|---|---|---|---|
| `node` | Next.js standalone | — | all |
| `rc3` | `0e1ec49` | defaults | all |
| `re23` | `c65731b` | `--js-heap-limit auto --memory-reclaim-idle 10 --memory-pressure-reclaim 85` | all |
| `re4-admission` | RE-4 | as `re23` + `--memory-admission --js-external-memory-limit 32 --native-buffer-budget 32` | all |
| `re4-admission-young8` | RE-4 | as above + `--js-young-semi-space 8` | **not 256 MiB** (`excludeConfigs`) |

The 256 MiB exclusion of `re4-admission-young8` is in the variants file,
with its reason ([RE-4-YOUNG-GENERATION-EXPERIMENT](RE-4-YOUNG-GENERATION-EXPERIMENT.md)):
the harness skips it and records the skip in meta and in the summary. To
run it there after the 512 MiB results justify it, add
`--allow-config re4-admission-young8=256m --allow-reason "TEXT"` (the
reason is recorded).

Two kinds of test, never mixed in one comparison:

| Kind | Workloads | What it answers |
|---|---|---|
| **Continuous traffic** | `full` (every request class back to back), `envelope` (rising open-loop rates until failure, then closed loop) | memory and CPU under load; sustainable throughput |
| **Load-idle cycles** | `load-idle-cycles` (3 × load 65 s, **idle 60 s**; formerly `cycles`), `faults-pressure-cycles` (10 × load 60 s, idle 60 s) | does memory return after load (reclamation), does anything accumulate |

The previous version of this runbook called `cycles` "continuous traffic
and recovery": it is not continuous, it idles 60 s per cycle. An optional
`continuous` workload (`--continuous-minutes N`, mixed 30/s with no idle
gap) exists for sustained load; it is not part of the decisive set.

```bash
source ~/re4-linux.env
cd "$NEXTRS_SRC"
V="$NEXTRS_SRC/benchmarks/resource/variants/re4-decisive.json"
GATE=(--gate "$OUT/gate/gate.json" "${BINS[@]}")
run() { "$NODE" benchmarks/resource/run.mjs profile --variants "$V" "${GATE[@]}" "$@"; }
run --configs 256m:1,512m:1,1g:2 --rounds 3 --workload full --out "$OUT/decisive-full" --dry-run | tail -3
run --configs 256m:1,512m:1,1g:2 --rounds 3 --workload full --out "$OUT/decisive-full"                  # about 9 h
run --configs 256m:1,512m:1 --rounds 3 --workload envelope --out "$OUT/decisive-envelope"               # about 5.5 h
run --configs 256m:1,512m:1 --rounds 3 --workload load-idle-cycles --out "$OUT/decisive-load-idle-cycles" # about 6 h
"$NODE" benchmarks/resource/run.mjs profile --variants "$NEXTRS_SRC/benchmarks/resource/variants/re4-gctrace.json" \
  "${GATE[@]}" --configs 256m:1,512m:1 --rounds 1 --workload memory --save-logs --out "$OUT/gctrace"     # about 1 h
for d in "$OUT"/decisive-* "$OUT"/gctrace; do "$NODE" benchmarks/resource/summarize.mjs "$d" --json "$d/summary.json" > "$d/SUMMARY.md"; done
"$NODE" benchmarks/resource/gc-summary.mjs "$OUT/gctrace" > "$OUT/gctrace/GC.md"
```

**Optional, RE-4.1 accounting comparison** (owner decision; not in the
decisive set). `RE4_COMMIT` must include RE-4.1 (the flags are checked
against that commit). The admission counters record both totals in every
run (`commitments`, `committed_bytes`, `unrealized_bytes` next to
`reserved_bytes`), so the decisive runs already show what `committed`
accounting would have seen. This pair measures what it does:

```bash
VARS="$NEXTRS_SRC/benchmarks/resource/variants"
"$NODE" benchmarks/resource/run.mjs gate --variants "$VARS/re41-gate.json" --baseline node --reference re23 \
  --candidates re41-sampled,re41-committed --gate-configs 1g:2,256m:1 "${BINS[@]}" --out "$OUT/gate-re41"
G41=(--gate "$OUT/gate-re41/gate.json" "${BINS[@]}")
"$NODE" benchmarks/resource/run.mjs profile --variants "$VARS/re41-accounting.json" "${G41[@]}" \
  --configs 256m:1,512m:1 --rounds 3 --workload full --out "$OUT/re41-accounting-full"
"$NODE" benchmarks/resource/run.mjs profile --variants "$VARS/re41-accounting.json" "${G41[@]}" \
  --configs 256m:1 --rounds 3 --workload faults-concurrent-ssr --out "$OUT/re41-accounting-ssr"
```

`re41-gate.json` is the same pair with `node` and `re23` for the gate's
comparison. Compare OOM kills, peak working set, 503 share and p99
between `re41-sampled` and `re41-committed`
([RE-4.1-ADMISSION-ACCOUNTING](RE-4.1-ADMISSION-ACCOUNTING.md)). Add
`--dry-run` to any of these first.

(The Mac `re23-gctrace.json` names the `base` and `re2b` binaries, which
this series does not build: `validate` fails it. `re4-gctrace.json` is the
same set for `rc3`, `re23` and RE-4.)

Valid only with 3 rounds per cell, every round counted. A round with a
readiness failure or an OOM kill is a result, not a reason to rerun.
Rejected requests (503) are errors, never throughput. Per cell, compare:
average and peak container memory, anonymous memory, post-load recovery,
CPU per **successful** request, GC count and pauses (gctrace), fixed-rate
p95/p99, saturation throughput, and from `/metrics.json` (the summary's
"RE-4 counters" table, raw readings in `*.metrics.jsonl`):
`admission.deferred`, `admission.rejected`, `admission.state_transitions`,
`external_memory.refused`, native buffer refusals, isolate restarts and
`last_heap_limit`.

### Fault tests

`--workload faults-*`, at 256 MiB / 1 vCPU (the default), a fresh
container per case; details and per-case defaults in the
[harness README](../../../benchmarks/resource/README.md#fault-workloads).
Four use Velasia, four the fault app (`benchmarks/resource/fault-app`,
built in §3, images from §4).

| Fault | Workload | App | Driver | Record |
|---|---|---|---|---|
| native memory exhaustion | `faults-native-memory` | fault | 16 MiB `Buffer.alloc` chunks held by the isolate, up to 2 × the limit | OOM kill, RangeError (with `--js-external-memory-limit`), or admission refusals |
| large single JS allocation | `faults-js-allocation` | fault | `heap-faults.js` modes (`heap-single`, `heap-string`) at 64–256 MiB, one per container | replaced / aborted / OOM-killed, by size |
| concurrent SSR | `faults-concurrent-ssr` | Velasia | closed loop at 16, 32, 64, 128 connections on the SSR search | peak memory, restarts, 503 share |
| large uploads | `faults-large-uploads` | fault | 8 and 32 concurrent 8 MiB POSTs, streamed and buffered route | peak memory, 413/503 |
| slow response streams | `faults-slow-streams` | fault | 16 and 64 clients reading at 1 KiB/s from a streaming route | memory per stalled stream |
| queue saturation | `faults-queue-saturation` | Velasia | open loop at 2 × the measured saturation, then 0.5 × | queue length and bytes, 503 codes, recovery |
| several isolates | `faults-isolates` | Velasia | `--isolates 2` at 256 MiB | heap clamp warning, restarts |
| pressure cycles | `faults-pressure-cycles` | Velasia | 10 × (pressure 60 s, idle 60 s): load-idle | does memory return each cycle |

```bash
source ~/re4-linux.env
cd "$NEXTRS_SRC"
V="$NEXTRS_SRC/benchmarks/resource/variants/re4-decisive.json"
for f in native-memory js-allocation concurrent-ssr large-uploads slow-streams queue-saturation isolates pressure-cycles; do
  "$NODE" benchmarks/resource/run.mjs profile --variants "$V" --gate "$OUT/gate/gate.json" "${BINS[@]}" \
    --fault-build "$OUT/build/fault-build-identity.json" --workload "faults-$f" --rounds 3 --out "$OUT/faults-$f"
done                                                                                                     # about 14 h
"$NODE" benchmarks/resource/fault-summary.mjs "$OUT"/faults-* --json "$OUT/faults-summary.json" > "$OUT/FAULTS.md"
```

Each case is classified from what the harness observes: **prevented** (no
failure), **contained** (isolate replaced, per `/metrics.json` restarts or
generation; or requests refused or failed: non-2xx, connection errors,
RE-4 refusal counters, a RangeError the route caught; process alive and
ready again) or **process-fatal** (exited, OOM-killed per `docker
inspect`, restarted, main process changed, or never ready again). Node.js
has no `/metrics.json`; its classification uses HTTP and Docker only.
`--dry-run` prints every container and client command of a fault workload.

## 7. Soaks (selected candidate only)

Staged; each stage must pass before the next. `SELECTED` is the label of
the variant chosen from §6 (it keeps its gated flags; nothing is typed by
hand):

```bash
source ~/re4-linux.env
cd "$NEXTRS_SRC"
SELECTED=re4-admission                     # set explicitly: a label from re4-decisive.json
"$NODE" -e 'const [f, l] = process.argv.slice(1); const v = JSON.parse(require("fs").readFileSync(f, "utf8")).filter((x) => x.label === l);
  if (v.length !== 1) { console.error(`no variant ${l}`); process.exit(1); } console.log(JSON.stringify(v));' \
  "$NEXTRS_SRC/benchmarks/resource/variants/re4-decisive.json" "${SELECTED:?set SELECTED}" > "$OUT/soak.json"
soak() { "$NODE" benchmarks/resource/run.mjs profile --variants "$OUT/soak.json" --gate "$OUT/gate/gate.json" "${BINS[@]}" \
  --workload soak --configs 256m:1 --rounds 1 --soak-minutes "$1" --out "$OUT/soak-$1m"; }
for m in 30 120; do
  soak "$m" || break
  "$NODE" benchmarks/resource/soak-summary.mjs "$OUT/soak-${m}m" > "$OUT/soak-${m}m/SOAK.md"   # check the pass criteria before the next
done
# After 30 min and 2 h pass and the decisive fault tests pass:
soak 360 && "$NODE" benchmarks/resource/soak-summary.mjs "$OUT/soak-360m" > "$OUT/soak-360m/SOAK.md"
```

The soak workload is load-idle blocks (6 minutes: 270 s of traffic, 90 s
idle), not continuous traffic. Pass criteria: those of
[RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md). Deferred to RE-8 (the
final release campaign): the full configuration matrix (128 MiB to 1 GiB,
0.5 and 2 vCPU) and the 24-hour soak. The RE-23B release conditions stay
open until they run.

## 8. Handing results back

Keep **all** raw data. Every run directory holds, per run, the raw
1-second samples (`*.samples.jsonl`), every load generator result
untrimmed (`*.loadgen.jsonl`), every `/metrics.json` reading
(`*.metrics.jsonl`), the fault clients' full results (`*.client.jsonl`)
and the logs, beside the run files and the summaries. The Mac series
committed only summaries; this one must not drop raw files.

```bash
source ~/re4-linux.env
"$NEXTRS_SRC/benchmarks/resource/preflight.sh" > "$OUT/host-after.txt" 2>&1
( cd "$OUT" && find . -type f ! -name MANIFEST.sha256 -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > MANIFEST.sha256 )
tar -C "$(dirname "$OUT")" -czf "$OUT.tar.gz" "$(basename "$OUT")"     # the complete series
sha256sum "$OUT.tar.gz" > "$OUT.tar.gz.sha256"
```

Then either point the tools at `$OUT` (or the archive), or copy the
directory to `engineering/performance/resource-efficiency/results/linux-<date>/`
**with** the raw files; if files above 20 MiB must stay out of git,
compress them (`gzip -k`) rather than dropping them, or keep them only in
the archive and list in `results/linux-<date>/RAW.md` where the archive is
and its sha256. Reports are written only from data that exists.

If a run is interrupted, its probe-side samples are still in the
`nextrs-s1-work` volume:
`docker run --rm -v nextrs-s1-work:/work velasia-node:s1 ls /work/re`.

## 9. Cleanup and recovery

```bash
source ~/re4-linux.env
cd "$NEXTRS_SRC"
"$NODE" benchmarks/resource/run.mjs teardown
"$NODE" benchmarks/series1/run.mjs teardown
docker ps -a --filter name=s1- --filter name=re-probe -q | xargs -r docker rm -f
parent=$(realpath "$NEXTRS_SRC/..")
for l in rc3 re23 re4; do git -C "$NEXTRS_SRC" worktree remove "$parent/nextrs-bench-$l"; done
git -C "$VELASIA_REPO" worktree remove "$VELASIA_SRC"
sudo swapon -a                       # if swap was disabled
# Back to Docker Desktop:
sudo systemctl disable --now docker
systemctl --user enable --now docker-desktop && docker context use desktop-linux
```

If a run hangs: `docker ps`; `docker logs s1-sf | tail`; kill the
storefront (`docker rm -f s1-sf`). The harness continues with the next
run.
