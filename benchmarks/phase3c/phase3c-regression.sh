#!/usr/bin/env bash
# Phase 3C final verification on the committed tree. It runs:
# - the workspace gates (fmt, clippy, tests);
# - Phase 3B's Velasia confirmation and CI matrix;
# - the independent applications (HTTP and browser, 1 and 4 isolates);
# - the forwarded-header matrix and the end-to-end fault scenarios.
# Each step logs to $OUT/<step>.log and appends "<step> <exit code>" to
# $OUT/steps.txt. A failing step does not stop the others.
#
#   benchmarks/phase3c/phase3c-regression.sh <velasia clone> <out dir>
set -u
SRC=$1
OUT=$2
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
cd "$ROOT"
: > "$OUT/steps.txt"
echo "commit $(git rev-parse HEAD) started $(date -u +%FT%TZ)" > "$OUT/README.txt"
step() {
  local name=$1
  shift
  echo "[$(date -u +%H:%M:%S)] $name: $*" >> "$OUT/steps.txt.log"
  "$@" > "$OUT/$name.log" 2>&1
  echo "$name $?" >> "$OUT/steps.txt"
}

step fmt cargo fmt --all -- --check
step clippy cargo clippy --workspace --all-targets -- -D warnings
step test cargo test --workspace --no-fail-fast
step build cargo build --release -p nextrs-cli
step build-workspace cargo build --workspace

C=benchmarks/certification/applications/velasia-airways/scenarios/playwright.isolation.config.ts
step velasia-e2e-i1 node benchmarks/certification/scripts/certify.mjs e2e --src "$SRC" --runtime nextrs --isolates 1 --workers 2 --db-root /tmp --label phase3c-nextrs-i1
step velasia-e2e-i4 node benchmarks/certification/scripts/certify.mjs e2e --src "$SRC" --runtime nextrs --isolates 4 --workers 2 --db-root /tmp --label phase3c-nextrs-i4
step velasia-isolation-i4 node benchmarks/certification/scripts/certify.mjs e2e --src "$SRC" --runtime nextrs --isolates 4 --db-root /tmp --label phase3c-isolation-i4 --config "$C" -- isolation.spec.ts
step velasia-verify node benchmarks/certification/scripts/certify.mjs verify --src "$SRC"

step ignored-tests cargo test -p nextrs-server --test dynamic --test reliability --test memory --test proxy -- --ignored

step compat-pinned-i1 scripts/compat-suite.sh --isolates 1 --out "$OUT/compat-pinned-i1" --browser --cache-components --proxy --proxy-combinations --images --pages --adapter
step realworld-pinned-i1 scripts/realworld-suite.sh --isolates 1 --out "$OUT/realworld-pinned-i1" --browser
step compat-pinned-i4 scripts/compat-suite.sh --isolates 4 --out "$OUT/compat-pinned-i4" --browser --cache-components --proxy --proxy-combinations --images --pages --adapter
step realworld-pinned-i4 scripts/realworld-suite.sh --isolates 4 --out "$OUT/realworld-pinned-i4" --browser
# The fixtures pin 16.4.6; 16.4.3 and 16.4.4 no longer build (security
# advisories, see engineering/compatibility.md).
for v in 16.4.5; do
  step "compat-$v-i1" scripts/compat-suite.sh --next "$v" --isolates 1 --out "$OUT/compat-$v-i1" --cache-components --proxy --images --pages
  step "realworld-$v-i1" scripts/realworld-suite.sh --next "$v" --isolates 1 --out "$OUT/realworld-$v-i1"
done

APPS=nextjs-blog-starter,nextjs-api-routes-rest,nextjs-iron-session-cache-components,nextjs-image-component
for i in 1 4; do
  for a in ${APPS//,/ }; do step "independent-$a-i$i" node benchmarks/certification/scripts/compare-app.mjs compare --app "$a" --isolates "$i"; done
  step "independent-browser-i$i" node benchmarks/certification/scripts/browser-apps.mjs --apps "$APPS" --isolates "$i"
done

mkdir -p benchmarks/results/phase3c/security benchmarks/results/phase3c/faults
step forwarded-matrix node benchmarks/phase3c/forwarded-matrix.mjs --src "$SRC" --out "$OUT/forwarded-matrix.json"
step faults node benchmarks/phase3c/faults.mjs --src "$SRC" --out "$OUT/faults.json"
step fault-cache-probe node benchmarks/phase3c/fault-cache-probe.mjs --rounds 60
echo done >> "$OUT/steps.txt"
