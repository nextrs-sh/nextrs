#!/usr/bin/env bash
# Phase 3B final verification on the committed tree: the Velasia
# confirmation runs and NextRS's existing regression matrix (as CI runs it,
# plus Next.js 16.4.6, Velasia's version). Each step logs to $OUT/<step>.log
# and appends "<step> <exit code>" to $OUT/steps.txt; a failing step does not
# stop the others.
#
#   benchmarks/certification/scripts/phase3b-regression.sh <velasia clone> <out dir>
set -u
SRC=$1
OUT=$2
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
mkdir -p "$OUT"
cd "$ROOT"
: > "$OUT/steps.txt"
step() {
  local name=$1
  shift
  echo "[$(date -u +%H:%M:%S)] $name: $*" >> "$OUT/steps.txt.log"
  "$@" > "$OUT/$name.log" 2>&1
  echo "$name $?" >> "$OUT/steps.txt"
}

step build cargo build --release -p nextrs-cli
C=benchmarks/certification/applications/velasia-airways/scenarios/playwright.isolation.config.ts
step velasia-e2e-i1 node benchmarks/certification/scripts/certify.mjs e2e --src "$SRC" --runtime nextrs --isolates 1 --workers 2 --db-root /tmp --label final-nextrs-i1
step velasia-e2e-i4 node benchmarks/certification/scripts/certify.mjs e2e --src "$SRC" --runtime nextrs --isolates 4 --workers 2 --db-root /tmp --label final-nextrs-i4
step velasia-isolation-i4 node benchmarks/certification/scripts/certify.mjs e2e --src "$SRC" --runtime nextrs --isolates 4 --db-root /tmp --label final-isolation-i4 --config "$C" -- isolation.spec.ts
step velasia-verify node benchmarks/certification/scripts/certify.mjs verify --src "$SRC"

# CI: the executor tests that need fixtures/next-app built.
step next-app-build bash -c '(cd fixtures/next-app && npm ci --no-audit --no-fund && npx next build)'
step ignored-tests cargo test -p nextrs-server --test dynamic --test reliability --test memory --test proxy -- --ignored

# CI's compatibility matrix.
step compat-pinned-i1 scripts/compat-suite.sh --isolates 1 --out "$OUT/compat-pinned-i1" --browser --cache-components --proxy --proxy-combinations --images --pages --adapter
step realworld-pinned-i1 scripts/realworld-suite.sh --isolates 1 --out "$OUT/realworld-pinned-i1" --browser
step compat-pinned-i4 scripts/compat-suite.sh --isolates 4 --out "$OUT/compat-pinned-i4" --browser --cache-components --proxy --proxy-combinations --images --pages --adapter
step realworld-pinned-i4 scripts/realworld-suite.sh --isolates 4 --out "$OUT/realworld-pinned-i4" --browser
for v in 16.4.4 16.4.5 16.4.6; do
  step "compat-$v-i1" scripts/compat-suite.sh --next "$v" --isolates 1 --out "$OUT/compat-$v-i1" --cache-components --proxy --images --pages
  step "realworld-$v-i1" scripts/realworld-suite.sh --next "$v" --isolates 1 --out "$OUT/realworld-$v-i1"
done
echo done >> "$OUT/steps.txt"
