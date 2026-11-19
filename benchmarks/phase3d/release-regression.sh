#!/usr/bin/env bash
# Phase 3D release-candidate verification against one exact binary (the
# packaged one, not a fresh target/release build). It runs:
# - Velasia Airways certification: its own Playwright e2e suite at 1 and 4
#   isolates, the cross-customer isolation spec at 4, the integrity check of
#   the read-only clone;
# - the CI compatibility matrix against next start (pinned 16.4.6 at 1 and 4
#   isolates, with the browser suites; 16.4.5 at 1 isolate);
# - the independent applications (HTTP and browser, 1 and 4 isolates);
# - the forwarded-header matrix and the end-to-end fault scenarios.
# Each step logs to $OUT/<step>.log and appends "<step> <exit code>" to
# $OUT/steps.txt. A failing step does not stop the others.
#
#   benchmarks/phase3d/release-regression.sh --binary target/dist/nextrs \
#     --src <velasia clone> --out <dir> [--only "step-prefix ..."]
set -u
BINARY="" SRC="" OUT="" ONLY=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) BINARY="$2"; shift 2 ;;
    --src) SRC="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --only) ONLY="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -x $BINARY && -d $SRC && -n $OUT ]] || { echo "usage: $0 --binary PATH --src VELASIA_CLONE --out DIR" >&2; exit 2; }
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
cd "$ROOT"
SUM=$(sha256sum "$BINARY" | cut -d' ' -f1)
BIN=$ROOT/target/phase3d-soak/nextrs-${SUM:0:12}
mkdir -p "$(dirname "$BIN")"
[[ -x $BIN ]] || cp "$BINARY" "$BIN"
export NEXTRS_BIN=$BIN
: >> "$OUT/steps.txt"
echo "commit $(git rev-parse HEAD) binary $BIN sha256 $SUM version '$("$BIN" --version)' started $(date -u +%FT%TZ)" >> "$OUT/README.txt"
step() {
  local name=$1
  shift
  if [[ -n $ONLY ]]; then
    local match=0 p
    for p in $ONLY; do [[ $name == "$p"* ]] && match=1; done
    [[ $match == 1 ]] || return 0
  fi
  echo "[$(date -u +%H:%M:%S)] $name: $*" >> "$OUT/steps.txt.log"
  "$@" > "$OUT/$name.log" 2>&1
  echo "$name $?" >> "$OUT/steps.txt"
}

C=benchmarks/certification/applications/velasia-airways/scenarios/playwright.isolation.config.ts
step velasia-e2e-i1 node benchmarks/certification/scripts/certify.mjs e2e --src "$SRC" --nextrs "$BIN" --runtime nextrs --isolates 1 --workers 2 --db-root /tmp --label phase3d-rc1-nextrs-i1
step velasia-e2e-i4 node benchmarks/certification/scripts/certify.mjs e2e --src "$SRC" --nextrs "$BIN" --runtime nextrs --isolates 4 --workers 2 --db-root /tmp --label phase3d-rc1-nextrs-i4
step velasia-isolation-i4 node benchmarks/certification/scripts/certify.mjs e2e --src "$SRC" --nextrs "$BIN" --runtime nextrs --isolates 4 --db-root /tmp --label phase3d-rc1-isolation-i4 --config "$C" -- isolation.spec.ts
step velasia-verify node benchmarks/certification/scripts/certify.mjs verify --src "$SRC"

step compat-pinned-i1 scripts/compat-suite.sh --isolates 1 --out "$OUT/compat-pinned-i1" --browser --cache-components --proxy --proxy-combinations --images --pages --adapter
step realworld-pinned-i1 scripts/realworld-suite.sh --isolates 1 --out "$OUT/realworld-pinned-i1" --browser
step compat-pinned-i4 scripts/compat-suite.sh --isolates 4 --out "$OUT/compat-pinned-i4" --browser --cache-components --proxy --proxy-combinations --images --pages --adapter
step realworld-pinned-i4 scripts/realworld-suite.sh --isolates 4 --out "$OUT/realworld-pinned-i4" --browser
step compat-16.4.5-i1 scripts/compat-suite.sh --next 16.4.5 --isolates 1 --out "$OUT/compat-16.4.5-i1" --cache-components --proxy --images --pages
step realworld-16.4.5-i1 scripts/realworld-suite.sh --next 16.4.5 --isolates 1 --out "$OUT/realworld-16.4.5-i1"

APPS=nextjs-blog-starter,nextjs-api-routes-rest,nextjs-iron-session-cache-components,nextjs-image-component
for i in 1 4; do
  for a in ${APPS//,/ }; do step "independent-$a-i$i" node benchmarks/certification/scripts/compare-app.mjs compare --app "$a" --isolates "$i" --nextrs "$BIN"; done
  step "independent-browser-i$i" node benchmarks/certification/scripts/browser-apps.mjs --apps "$APPS" --isolates "$i" --nextrs "$BIN"
done

step forwarded-matrix node benchmarks/phase3c/forwarded-matrix.mjs --src "$SRC" --nextrs "$BIN" --out "$OUT/forwarded-matrix.json"
step faults node benchmarks/phase3c/faults.mjs --src "$SRC" --nextrs "$BIN" --out "$OUT/faults.json"
echo "done $(date -u +%FT%TZ)" >> "$OUT/steps.txt"
