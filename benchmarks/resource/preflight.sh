#!/usr/bin/env bash
# Native Linux preflight for the resource benchmarks. Read-only: it reads /proc, /sys and
# `docker info`/`docker image inspect`, and changes nothing on the host. One line per check,
# PASS / WARN / FAIL; exits 1 if any check fails.
#
#   OUT=/abs/results/linux-YYYYMMDD benchmarks/resource/preflight.sh [--container-check] [--images LIST]
#
#   --container-check  also start ONE throwaway container (alpine, --rm, --network none) to read
#                      its memory.max under --memory 256m: Docker pulls alpine if it is
#                      missing and removes the container; nothing on the host changes. Off by
#                      default because it runs a container.
#   --images LIST      comma-separated images that must be present (default: the Series 1
#                      images, nextrs-s1:{rc3,re23,re4} and the fault-app images).
#   --bins LIST        comma-separated files that must exist and be executable (default:
#                      $OUT/nextrs-rc3, $OUT/nextrs-re23, $OUT/nextrs-re4, $OUT/nextrs-loadgen).
#   --max-load N       highest 1-minute load average to pass (default 0.5).
#   --min-free-gib N   free disk under $OUT (default 20).
#
# Environment: OUT (required, absolute), NODE (the node binary the harness will use; default
# `node`), NODE_PIN (default 24.18.0).
set -uo pipefail

CONTAINER=0
IMAGES="velasia-node:s1,velasia-nextrs:s1,velasia-api:s1,nextrs-s1:rc3,nextrs-s1:re23,nextrs-s1:re4"
IMAGES="$IMAGES,nextrs-fault:node,nextrs-fault:rc3,nextrs-fault:re23,nextrs-fault:re4"
BINS=""
MAX_LOAD=0.5
MIN_FREE=20
while [ $# -gt 0 ]; do
  case "$1" in
    --container-check) CONTAINER=1 ;;
    --images) IMAGES=$2; shift ;;
    --bins) BINS=$2; shift ;;
    --max-load) MAX_LOAD=$2; shift ;;
    --min-free-gib) MIN_FREE=$2; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
NODE=${NODE:-node}
NODE_PIN=${NODE_PIN:-24.18.0}

fails=0
warns=0
pass() { printf 'PASS  %-18s %s\n' "$1" "$2"; }
warn() { printf 'WARN  %-18s %s\n' "$1" "$2"; warns=$((warns + 1)); }
fail() { printf 'FAIL  %-18s %s\n' "$1" "$2"; fails=$((fails + 1)); }
have() { command -v "$1" >/dev/null 2>&1; }

# OUT: absolute, so it survives every cd in the runbook.
if [ -z "${OUT:-}" ]; then fail out "OUT is not set"
else
  case "$OUT" in
    /*) pass out "$OUT" ;;
    *) fail out "OUT=$OUT is not absolute" ;;
  esac
fi
[ -n "$BINS" ] || BINS="${OUT:-/missing}/nextrs-rc3,${OUT:-/missing}/nextrs-re23,${OUT:-/missing}/nextrs-re4,${OUT:-/missing}/nextrs-loadgen"

# Kernel and cgroups (the host's own view).
fs=$(stat -fc %T /sys/fs/cgroup 2>/dev/null || echo unreadable)
[ "$fs" = cgroup2fs ] && pass cgroupfs "/sys/fs/cgroup is cgroup2fs" || fail cgroupfs "/sys/fs/cgroup is $fs (need cgroup v2)"
ctrl=$(cat /sys/fs/cgroup/cgroup.controllers 2>/dev/null || echo "")
missing=""
for c in cpu cpuset memory io pids; do case " $ctrl " in *" $c "*) ;; *) missing="$missing $c" ;; esac; done
[ -z "$ctrl" ] && fail controllers "cannot read /sys/fs/cgroup/cgroup.controllers" \
  || { [ -z "$missing" ] && pass controllers "$ctrl" || fail controllers "missing:$missing (have: $ctrl)"; }
psi=$(sed -n 's/^some avg10=\([0-9.]*\) avg60=\([0-9.]*\).*/\2/p' /proc/pressure/memory 2>/dev/null)
if [ -z "$psi" ]; then fail psi-memory "/proc/pressure/memory unreadable (PSI off?)"
elif awk -v v="$psi" 'BEGIN { exit !(v == 0) }'; then pass psi-memory "memory some avg60=$psi"
else fail psi-memory "memory some avg60=$psi (must be 0.00)"; fi

# Host contention.
load=$(cut -d' ' -f1 /proc/loadavg 2>/dev/null || echo 99)
awk -v l="$load" -v m="$MAX_LOAD" 'BEGIN { exit !(l < m) }' && pass load "1-min load $load (< $MAX_LOAD)" || fail load "1-min load $load (need < $MAX_LOAD after 5 min idle)"
govs=$(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null | sort | uniq -c | tr -s ' ' | tr '\n' ';')
if [ -z "$govs" ]; then warn governor "no cpufreq governors readable"
elif [ "$(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort -u)" = performance ]; then pass governor "$govs"
else fail governor "$govs (need performance on every core)"; fi
swap_used=$(awk '/^SwapTotal/ { t = $2 } /^SwapFree/ { f = $2 } END { print t - f }' /proc/meminfo 2>/dev/null)
[ "${swap_used:-1}" = 0 ] && pass swap "no swap in use" || fail swap "$((${swap_used:-0} / 1024)) MiB of swap in use (clear or disable it and record that)"
if [ -n "${OUT:-}" ]; then
  dir=$OUT; while [ ! -d "$dir" ] && [ "$dir" != / ]; do dir=$(dirname "$dir"); done
  free_gib=$(df -Pk "$dir" 2>/dev/null | awk 'NR == 2 { printf "%d", $4 / 1048576 }')
  [ "${free_gib:-0}" -ge "$MIN_FREE" ] && pass disk "${free_gib} GiB free under $dir" || fail disk "${free_gib:-?} GiB free under $dir (need $MIN_FREE)"
fi

# Docker: native engine, cgroup v2, systemd driver.
if ! have docker; then
  fail docker "docker CLI not found"
else
  ctx=$(docker context show 2>/dev/null || echo "?")
  [ "$ctx" = desktop-linux ] && fail docker-context "context desktop-linux (Docker Desktop VM)" || pass docker-context "$ctx"
  info=$(docker info --format '{{.ServerVersion}}|{{.CgroupVersion}}|{{.CgroupDriver}}|{{.OperatingSystem}}|{{.NCPU}}|{{.MemTotal}}' 2>/dev/null)
  if [ -z "$info" ]; then
    fail docker-info "docker info failed (no daemon reachable)"
  else
    IFS='|' read -r ver cgv drv os ncpu mem <<<"$info"
    [ "$cgv" = 2 ] && pass docker-cgroup "cgroup v$cgv" || fail docker-cgroup "cgroup v$cgv (need 2)"
    [ "$drv" = systemd ] && pass docker-driver "$drv" || fail docker-driver "$drv (need systemd)"
    case "$os" in *"Docker Desktop"*) fail docker-engine "$os $ver (Docker Desktop)" ;; *) pass docker-engine "$os, engine $ver, $ncpu CPUs, $((mem / 1048576)) MiB" ;; esac
    [ "$ncpu" = "$(nproc)" ] && pass docker-cpus "$ncpu = host CPUs" || fail docker-cpus "docker sees $ncpu CPUs, the host $(nproc) (a VM in between?)"
    IFS=',' read -ra want <<<"$IMAGES"
    for i in "${want[@]}"; do
      id=$(docker image inspect --format '{{.Id}}' "$i" 2>/dev/null) && pass image "$i ${id:0:19}" || fail image "$i not present"
    done
    if [ "$CONTAINER" = 1 ]; then
      mm=$(docker run --rm --network none --memory 256m --memory-swap 256m alpine cat /sys/fs/cgroup/memory.max 2>/dev/null)
      [ "$mm" = 268435456 ] && pass memory.max "256m container sees memory.max $mm" || fail memory.max "256m container sees '${mm:-nothing}' (need 268435456)"
    else
      warn memory.max "not checked (pass --container-check: starts one alpine container)"
    fi
  fi
fi

# Binaries and the harness's Node.js.
IFS=',' read -ra bins <<<"$BINS"
for b in "${bins[@]}"; do
  [ -f "$b" ] && [ -x "$b" ] && pass binary "$b" || fail binary "$b missing or not executable"
done
if have "$NODE" || [ -x "$NODE" ]; then
  nv=$("$NODE" --version 2>/dev/null | sed 's/^v//')
  [ "$nv" = "$NODE_PIN" ] && pass node "$NODE $nv" || warn node "$NODE is $nv, the pin is $NODE_PIN (record it)"
else
  fail node "$NODE not found (set NODE=/path/to/node $NODE_PIN)"
fi

echo
echo "$fails fail, $warns warn"
[ "$fails" = 0 ]
