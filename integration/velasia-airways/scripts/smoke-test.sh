#!/usr/bin/env bash
# Browser smoke tests against the running environment (http://localhost:3000 by default).
#
#   integration/velasia-airways/scripts/smoke-test.sh                 # chromium + firefox
#   SMOKE_PROJECTS=chromium integration/velasia-airways/scripts/smoke-test.sh
#
# 1. scripts/verify-runtime.sh: the target must be NextRS (skip with SKIP_VERIFY=1 when docker
#    is not reachable from where this runs).
# 2. Playwright 1.65.1 from smoke/package-lock.json (Node.js >= 20 on this machine; it only
#    drives the browser). Browsers once: (cd smoke && npx playwright install chromium firefox)
# 3. results/smoke/{results.json,html/,summary.md}
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
export SMOKE_BASE_URL=${SMOKE_BASE_URL:-http://localhost:3000}
export SMOKE_OUT=${SMOKE_OUT:-$here/results/smoke}
mkdir -p "$SMOKE_OUT"
if [ "${SKIP_VERIFY:-0}" != 1 ]; then
  "$here/scripts/verify-runtime.sh" || { echo "runtime verification failed: not running smoke tests against an unverified server" >&2; exit 1; }
fi
cd "$here/smoke"
[ -d node_modules/@playwright/test ] || PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 npm ci --no-audit --no-fund
args=()
for p in ${SMOKE_PROJECTS:-chromium firefox}; do args+=(--project "$p"); done
status=0
npx playwright test "${args[@]}" "$@" || status=$?
node summarize.mjs "$SMOKE_OUT/results.json" > "$SMOKE_OUT/summary.md" || true
cat "$SMOKE_OUT/summary.md" | sed -n '1,12p'
echo "full summary: $SMOKE_OUT/summary.md   html report: $SMOKE_OUT/html/index.html"
exit $status
