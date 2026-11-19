#!/usr/bin/env bash
# Phase 3D final soak of the v0.1.0 release candidate: the Phase 2G mixed
# soak (App and Pages Router, images, actions, ISR, proxy, mutation ledger,
# restart injection every 5 min, client aborts, a second i18n process) and
# the Velasia soak (API fault storms, client cancellation, signed-in
# identity checks) side by side, against one exact binary, then post-load
# idle sampling and a SIGTERM drain. Both drivers use the Phase 3D client
# (benchmarks/phase3d/soak-http.mjs).
#
#   benchmarks/phase3d/final-soak.sh --binary target/dist/nextrs [--isolates 4]
#     [--minutes 360] [--idle 10] [--out benchmarks/results/phase3d/final-soak-i4]
#
# The binary is copied (and its checksum recorded) before anything starts,
# so a rebuild during the soak cannot change what is measured.
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
BINARY="" ISOLATES=4 MINUTES=360 IDLE=10 OUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) BINARY="$2"; shift 2 ;;
    --isolates) ISOLATES="$2"; shift 2 ;;
    --minutes) MINUTES="$2"; shift 2 ;;
    --idle) IDLE="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -x $BINARY ]] || { echo "usage: $0 --binary PATH" >&2; exit 2; }
OUT=${OUT:-$ROOT/benchmarks/results/phase3d/final-soak-i$ISOLATES}
[[ -e $OUT ]] && mv "$OUT" "$OUT-previous-$(date +%s)"
mkdir -p "$OUT"
SHA=$(git rev-parse --short HEAD)
SUM=$(sha256sum "$BINARY" | cut -d' ' -f1)
BIN=$ROOT/target/phase3d-soak/nextrs-${SUM:0:12}
mkdir -p "$(dirname "$BIN")"
cp "$BINARY" "$BIN"
{
  echo "commit $SHA ($(git rev-parse HEAD))"
  echo "binary $BIN"
  echo "binary_sha256 $SUM"
  echo "version $("$BIN" --version)"
  echo "isolates $ISOLATES; minutes $MINUTES; idle $IDLE"
  echo "started $(date -u +%FT%TZ); load $(cut -d' ' -f1-3 /proc/loadavg)"
  echo "node $(node --version); host $(uname -r); cpu $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs)"
} > "$OUT/README.txt"
# Rebuild the soak fixtures from their pinned lockfiles before anything
# starts (a zero-minute run of the wrapper with --build), so no soak
# serves a stale build or overlaps a build.
benchmarks/phase2g-soak.sh --out "$OUT/build" --nextrs "$BIN" --isolates 1 --minutes 0 --build > "$OUT/build.log" 2>&1
grep -q '"version": "16.4.6"' fixtures/next-soak-mixed/node_modules/next/package.json || { echo "soak fixtures are not on the pinned Next.js" | tee -a "$OUT/README.txt"; exit 1; }
benchmarks/phase2g-soak.sh --out "$OUT/mixed" --nextrs "$BIN" --isolates "$ISOLATES" --minutes "$MINUTES" --idle-minutes "$IDLE" > "$OUT/mixed-wrapper.log" 2>&1 &
mixed=$!
sleep 20
node benchmarks/phase3c/velasia-soak.mjs --isolates "$ISOLATES" --minutes "$MINUTES" --idle-minutes "$IDLE" --nextrs "$BIN" --out "$OUT/velasia" > "$OUT/velasia-driver.log" 2>&1
echo "velasia exit $?" >> "$OUT/README.txt"
wait "$mixed"
echo "mixed exit $?" >> "$OUT/README.txt"
echo "finished $(date -u +%FT%TZ)" >> "$OUT/README.txt"
