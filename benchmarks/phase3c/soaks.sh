#!/usr/bin/env bash
# Phase 3C long soaks: for each isolate count, the Phase 2G mixed soak (App
# and Pages Router, images, actions, ISR, proxy, mutation ledger, restart
# injection, client aborts, plus a second i18n process) and the Velasia
# soak (velasia-soak.mjs) run side by side for MINUTES, then IDLE minutes
# of post-load sampling. One binary, copied and named by commit.
#
#   benchmarks/phase3c/soaks.sh [--isolates "4 1"] [--minutes 360] [--idle 10]
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
ISOLATES="4 1" MINUTES=360 IDLE=10
while [[ $# -gt 0 ]]; do
  case "$1" in
    --isolates) ISOLATES="$2"; shift 2 ;;
    --minutes) MINUTES="$2"; shift 2 ;;
    --idle) IDLE="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
SHA=$(git rev-parse --short HEAD)
BIN=$ROOT/target/phase3c-soak/nextrs-$SHA
mkdir -p "$(dirname "$BIN")"
cp target/release/nextrs "$BIN"
# The soak fixtures, rebuilt once from their pinned lockfiles before any
# soak starts (a zero-minute run of the wrapper with --build, so the build
# sees the same upstream and environment as the soak), so no soak serves a
# stale build or overlaps a build.
benchmarks/phase2g-soak.sh --out "$ROOT/benchmarks/results/phase3c/soak-build" --nextrs "$BIN" --isolates 1 --minutes 0 --build > "$ROOT/benchmarks/results/phase3c/soak-build.log" 2>&1
grep -q '"version": "16.4.6"' fixtures/next-soak-mixed/node_modules/next/package.json || { echo "soak fixtures are not on the pinned Next.js"; exit 1; }
for i in $ISOLATES; do
  O=$ROOT/benchmarks/results/phase3c/soak-i$i
  [[ -e $O ]] && mv "$O" "$O-previous-$(date +%s)"
  mkdir -p "$O"
  echo "commit $SHA (binary $(sha256sum "$BIN" | cut -c1-12)); isolates $i; minutes $MINUTES; idle $IDLE; started $(date -u +%FT%TZ); load $(cut -d' ' -f1-3 /proc/loadavg)" > "$O/README.txt"
  benchmarks/phase2g-soak.sh --out "$O/mixed" --nextrs "$BIN" --isolates "$i" --minutes "$MINUTES" --idle-minutes "$IDLE" > "$O/mixed-wrapper.log" 2>&1 &
  mixed=$!
  sleep 20
  node benchmarks/phase3c/velasia-soak.mjs --isolates "$i" --minutes "$MINUTES" --idle-minutes "$IDLE" --nextrs "$BIN" --out "$O/velasia" > "$O/velasia-driver.log" 2>&1
  echo "velasia exit $?" >> "$O/README.txt"
  wait "$mixed"
  echo "mixed exit $?" >> "$O/README.txt"
  echo "finished $(date -u +%FT%TZ)" >> "$O/README.txt"
done
