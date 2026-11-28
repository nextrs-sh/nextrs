# RE-4: native Fedora benchmark runbook

For the owner, to run from a **host terminal** on the Fedora machine
recorded in [RE-23B-LINUX-ENVIRONMENT](RE-23B-LINUX-ENVIRONMENT.md)
(i5-9400F, 6 cores, 15.5 GiB, HDD, Fedora 45). The tools' sandbox cannot
run these steps: they install packages, start a system daemon and read
`/sys/fs/cgroup`. Nothing here was run by the tools. Each step says what
it changes on the host.

Results from this host form a **separate Linux series**. Never pool or
compare them as one series with the Mac Docker Desktop results.

## 0. What you need to decide first

| Decision | Recommendation |
|---|---|
| Container engine | Native Docker Engine (Docker CE, cgroup v2, systemd driver), as the harness README requires. Not Docker Desktop (it runs a VM); not Podman's `docker` shim (the drivers were never run with it) |
| Images | Rebuild on Linux from pinned commits (§3), as approved in place of moving the Mac images |
| Exclusive time | About 27 h for §6 and §7, sequential, with the desktop session logged out |

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

## 2. Verify the host before every series

All read-only.

```bash
docker context show                 # default (not desktop-linux)
docker info --format '{{.ServerVersion}} cgroup={{.CgroupVersion}} driver={{.CgroupDriver}} os={{.OperatingSystem}} cpus={{.NCPU}} mem={{.MemTotal}}'
#   expect: cgroup=2 driver=systemd os=Fedora Linux 45 ... (not "Docker Desktop")
stat -fc %T /sys/fs/cgroup          # cgroup2fs
cat /sys/fs/cgroup/cgroup.controllers   # includes cpu memory cpuset
docker run --rm --memory 256m --memory-swap 256m alpine cat /sys/fs/cgroup/memory.max   # 268435456
```

Host contention, recorded into the series directory before and after:

```bash
OUT=results/linux-$(date +%Y%m%d)
mkdir -p "$OUT"
{ date -Is; uname -a; uptime; free -m; swapon --show; cat /proc/pressure/cpu /proc/pressure/memory /proc/pressure/io
  cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort | uniq -c
  ps -eo pid,pcpu,pmem,comm --sort=-pcpu | head -15; } > "$OUT/host-before.txt"
```

Conditions to start a measured run:

- Log out of the desktop session and run from a text console (Ctrl+Alt+F3)
  or over SSH.
- `uptime` load average below 0.5 after 5 minutes idle.
- CPU governor `performance` on all cores (it already is).
- Swap: `swapon --show` shows nothing in use. To clear zram:
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
| NextRS RE-4 (`re4`) | the RE-4 merge commit (or this branch's head while unmerged) | |
| Rust | 1.98.0 | CI's pin |
| Node.js | 22.20.0 for the images (`Dockerfile.node`); 22+ for the harness | |

The host has no C toolchain, so build the binaries in a container
(downloads the prebuilt `rusty_v8` 150.4.0 archive):

```bash
cd ~/Documents/OpenSource/nextrs
for pair in rc3=0e1ec49 re23=c65731b re4=<RE4-COMMIT>; do
  label=${pair%=*}; commit=${pair#*=}
  git worktree add -f ../nextrs-bench-$label $commit
  docker run --rm -v ../nextrs-bench-$label:/src -w /src rust:1.98.0-bookworm \
    cargo build --locked --profile dist -p nextrs-cli
  cp ../nextrs-bench-$label/target/dist/nextrs "$OUT/nextrs-$label"
done
docker run --rm -v "$PWD/benchmarks/loadgen":/src -w /src rust:1.98.0-bookworm cargo build --release
cp benchmarks/loadgen/target/release/nextrs-loadgen "$OUT/"
sha256sum "$OUT"/nextrs-* "$OUT"/nextrs-loadgen > "$OUT/binaries.sha256"
```

Images (one `next build` for every runtime):

```bash
git -C ~/Documents/OpenSource/velasia-airways worktree add ../velasia-d04127a d04127a
cd ~/Documents/OpenSource/velasia-d04127a
docker build -f Dockerfile.node   -t velasia-node:s1 .
docker build -f Dockerfile.nextrs -t velasia-nextrs:s1 .
docker build -f docker/Dockerfile.api -t velasia-api:s1 .
for i in velasia-node:s1 velasia-nextrs:s1 velasia-api:s1; do
  echo "$i $(docker image inspect --format '{{.Id}}' $i)"; done > "$OUT/images.txt"
# Both storefront images must serve the same build:
for i in velasia-node:s1 velasia-nextrs:s1; do docker run --rm --entrypoint cat $i apps/web/.next/BUILD_ID; echo; done
```

Record in `$OUT/differences.md` everything that differs from the Mac
series: the host, the engine, the rebuilt binaries (new hashes; the Mac
`af994c15…` and `052d741d…` binaries are not reproduced bit for bit),
the image IDs, Node.js and base-image digests.

## 4. Harness setup

```bash
cd ~/Documents/OpenSource/nextrs
# 6 cores: storefront on 2-3, tools and probe on 4-5, API on 0-1.
export S1_SF_CPUSET=2-3 S1_TOOLS_CPUSET=4-5 S1_API_CPUSET=0-1
node benchmarks/series1/run.mjs setup --loadgen "$OUT/nextrs-loadgen" \
  --bin rc3="$OUT/nextrs-rc3" --bin re23="$OUT/nextrs-re23" --bin re4="$OUT/nextrs-re4"
node benchmarks/resource/run.mjs setup
node benchmarks/resource/run.mjs discover --runtime re4
# The probe must read the storefront's cgroup (SELinux denials show as
# missing samples, not errors):
node benchmarks/resource/run.mjs profile --variants benchmarks/resource/variants/re4-decisive.json \
  --workload soak --soak-minutes 6 --configs 256m:1 --rounds 1 --out "$OUT/probe-check"
ls -l "$OUT"/probe-check/*.samples.jsonl && head -c 600 "$(ls "$OUT"/probe-check/*.samples.jsonl | head -1)"
# every line must carry the cgroup memory and cpu.stat fields
```

## 5. Correctness gate (before any timing)

```bash
node benchmarks/series1/run.mjs capture --runtimes node,re23,re4 --out "$OUT/capture"
node benchmarks/series1/compare.mjs "$OUT/capture/capture-node.json" "$OUT/capture/capture-re4.json" --labels node,re4
```

Stop if `re4` differs from `re23` anywhere that `re23` did not already
differ from `node`.

## 6. Decisive campaign

Variants (`benchmarks/resource/variants/re4-decisive.json`):

| Label | Binary | Flags |
|---|---|---|
| `node` | Next.js standalone | — |
| `rc3` | `0e1ec49` | defaults |
| `re23` | `c65731b` | `--js-heap-limit auto --memory-reclaim-idle 10 --memory-pressure-reclaim 85` |
| `re4-admission` | RE-4 | as `re23` + `--memory-admission --js-external-memory-limit 32 --native-buffer-budget 32` |
| `re4-admission-young8` | RE-4 | as above + `--js-young-semi-space 8` (drop at 256 MiB unless the 512 MiB results justify it) |

```bash
V=benchmarks/resource/variants/re4-decisive.json
node benchmarks/resource/run.mjs profile --variants $V --configs 256m:1,512m:1,1g:2 \
  --rounds 3 --workload full --out "$OUT/decisive-full"            # about 12 h
node benchmarks/resource/run.mjs profile --variants $V --configs 256m:1,512m:1 \
  --rounds 3 --workload envelope --out "$OUT/decisive-envelope"    # about 4 h
node benchmarks/resource/run.mjs profile --variants $V --configs 256m:1,512m:1 \
  --rounds 3 --workload cycles --out "$OUT/decisive-cycles"        # continuous traffic and recovery, about 2 h
node benchmarks/resource/run.mjs profile --variants benchmarks/resource/variants/re23-gctrace.json \
  --configs 256m:1,512m:1 --rounds 1 --workload memory --save-logs --out "$OUT/gctrace"
for d in "$OUT"/decisive-* "$OUT"/gctrace; do node benchmarks/resource/summarize.mjs "$d" --json "$d/summary.json"; done
node benchmarks/resource/gc-summary.mjs "$OUT/gctrace"
```

Valid only with 3 rounds per cell, every round counted. A round with a
readiness failure or an OOM kill is a result, not a reason to rerun.
Rejected requests (503) are errors, never throughput. Per cell, compare:
average and peak container memory, anonymous memory, post-load recovery,
CPU per **successful** request, GC count and pauses (gctrace), fixed-rate
p95/p99, saturation throughput, and from `/metrics.json`:
`admission.deferred`, `admission.rejected`, `admission.state_transitions`,
`external_memory.refused`, isolate restarts and `last_heap_limit`.

**Fault tests:** the harness has no fault workloads yet. They are
specified here, to be implemented as `--workload faults-*` once the
host qualifies:

| Fault | Driver | Record |
|---|---|---|
| native memory exhaustion | a route handler that holds large `Buffer.alloc` chunks | OOM kill, RangeError (with `--js-external-memory-limit`), or admission refusals |
| large single JS allocation | `heap-faults.js` modes in a test app route (60–256 MiB) | replaced / aborted / OOM-killed, by size |
| concurrent SSR | closed loop at 16, 32, 64, 128 connections on the SSR search | peak memory, restarts, 503 share |
| large uploads | concurrent 8 MiB POSTs to a route handler | peak memory, 413/503 |
| slow response streams | clients reading at 1 KiB/s from a streaming route | memory per stalled stream |
| queue saturation | open loop at 2× saturation | queue length and bytes, 503 codes, recovery |
| several isolates | `--isolates 2` at 256 MiB | heap clamp warning, restarts |
| pressure cycles | 10 × (pressure 60 s, idle 60 s) | does memory return each cycle |

Classify each outcome as **prevented** (no failure), **contained**
(isolate replaced or request refused, process alive) or
**process-fatal** (abort or OOM kill).

## 7. Soaks (selected candidate only)

Staged; each stage must pass before the next:

```bash
SOAK='[{"label":"re4","runtime":"re4","args":"<the selected flags>"}]'
echo "$SOAK" > "$OUT/soak.json"
for m in 30 120; do
  node benchmarks/resource/run.mjs profile --variants "$OUT/soak.json" --workload soak \
    --soak-minutes $m --configs 256m:1 --rounds 1 --out "$OUT/soak-${m}m"
  node benchmarks/resource/soak-summary.mjs "$OUT/soak-${m}m"
done
# After 30 min and 2 h pass and the decisive fault tests pass:
node benchmarks/resource/run.mjs profile --variants "$OUT/soak.json" --workload soak \
  --soak-minutes 360 --configs 256m:1 --rounds 1 --out "$OUT/soak-6h"
```

Pass criteria: those of [RE-23-SOAK-TEST-REPORT](RE-23-SOAK-TEST-REPORT.md).
Deferred to RE-8 (the final release campaign): the full configuration
matrix (128 MiB to 1 GiB, 0.5 and 2 vCPU) and the 24-hour soak. The
RE-23B release conditions stay open until they run.

## 8. Handing results back

Copy `$OUT` (without the per-run sample files above 20 MiB, as for the Mac
series) into
`engineering/performance/resource-efficiency/results/linux-<date>/`, or
point the tools at the directory. Reports are written only from data that
exists.

## 9. Cleanup and recovery

```bash
node benchmarks/resource/run.mjs teardown
node benchmarks/series1/run.mjs teardown
docker ps -a --filter name=s1- --filter name=re-probe -q | xargs -r docker rm -f
git worktree remove ../nextrs-bench-rc3 ../nextrs-bench-re23 ../nextrs-bench-re4
sudo swapon -a                       # if swap was disabled
# Back to Docker Desktop:
sudo systemctl disable --now docker
systemctl --user enable --now docker-desktop && docker context use desktop-linux
```

If a run hangs: `docker ps`; `docker logs s1-sf | tail`; kill the
storefront (`docker rm -f s1-sf`). The harness continues with the next
run.
