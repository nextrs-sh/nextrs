#!/usr/bin/env bash
# Phase 3C.9 final performance comparison, on a quiet machine:
#
# 1. Velasia (unchanged build): next standalone, Phase 3B (952935b) and
#    Phase 3C at 1 and 4 isolates (velasia-focus.mjs, alternated rounds).
#    Phase 3A cannot serve Velasia (it predates the monorepo fixes).
# 2. Phase 3A fixtures: next start, Phase 3A (6c1101d), Phase 3B (952935b)
#    and Phase 3C, closed and open loop (benchmarks/phase3a/run.mjs).
# 3. Image capacity: Phase 3C with --image-concurrency 2, 3 (this
#    machine's default, half the CPUs) and 6, on image and mixed traffic.
#
#   benchmarks/phase3c/final-bench.sh [OUT] [--quick]
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=${1:-$ROOT/benchmarks/results/phase3c/final-bench-$(git -C "$ROOT" rev-parse --short HEAD)}
QUICK=${2:-}
ROUNDS=3
[[ $QUICK == --quick ]] && ROUNDS=1
mkdir -p "$OUT"
cd "$ROOT"
P3A=target/bench-bins/nextrs-6c1101d
P3B=target/phase3c-base/nextrs
P3C=target/release/nextrs
{
  echo "commit $(git rev-parse HEAD)"
  echo "started $(date -u +%FT%TZ)"
  echo "p3a $P3A ($(sha256sum $P3A | cut -c1-12)) p3b $P3B ($(sha256sum $P3B | cut -c1-12)) p3c $P3C ($(sha256sum $P3C | cut -c1-12))"
  echo "node $(node --version); cpus $(nproc); $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2); load $(cut -d' ' -f1-3 /proc/loadavg)"
} > "$OUT/README.txt"

node benchmarks/certification/scripts/velasia-focus.mjs \
  --configs node,p3b:1,p3c:1,p3b:4,p3c:4 \
  --workloads home,destination,flight-search,api-route,image-cached \
  --rounds "$ROUNDS" --duration 8 --warmup 3 \
  --bin p3b=$P3B --bin p3c=$P3C --out "$OUT/velasia" > "$OUT/velasia.log" 2>&1

node benchmarks/phase3a/run.mjs --out "$OUT/fixtures" \
  --nextrs $P3A --label p3a --nextrs-b $P3B --label-b p3b --nextrs-c $P3C --label-c p3c \
  --isolates 1 --groups app,cache-components,pages,image,mixed \
  --only ssr,rsc,action,isr-hit,use-cache-hit,pages-ssr,pages-ssr-proxied,image-hit,image-transform,mixed \
  --rounds "$ROUNDS" --duration 10 --warmup 5 > "$OUT/fixtures.log" 2>&1

node benchmarks/phase3a/run.mjs --out "$OUT/images" --no-next \
  --nextrs $P3C --label conc2 --args "--image-concurrency 2" \
  --nextrs-b $P3C --label-b conc3 --args-b "--image-concurrency 3" \
  --nextrs-c $P3C --label-c conc6 --args-c "--image-concurrency 6" \
  --isolates 1 --groups image,mixed --only image-transform,image-hit,mixed \
  --rounds "$ROUNDS" --duration 10 --warmup 5 > "$OUT/images.log" 2>&1

echo "finished $(date -u +%FT%TZ)" >> "$OUT/README.txt"
