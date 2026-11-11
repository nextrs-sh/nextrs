#!/usr/bin/env bash
# Builds the fixtures used by benchmarks/phase3a/run.mjs, as next start and
# NextRS will both serve them (the same .next directory):
#
#   benchmarks/phase3a/build-fixtures.sh [fixture ...]
#
# The ledger upstream (tests/compat/upstream.mjs) runs on 4320 during the
# builds, because prerendered ISR pages fetch from it at build time; the
# image fixtures compile in remote port 4317. Uses the fixtures' pinned
# lockfiles (npm ci). Prints each build's BUILD_ID and manifest hash.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
fixtures=("$@")
if [[ ${#fixtures[@]} -eq 0 ]]; then
  fixtures=(next-app next-cache-components next-proxy next-auth-proxy next-pages next-image next-soak-mixed)
fi
export NEXTRS_UPSTREAM="http://127.0.0.1:4320" NEXTRS_IMAGE_UPSTREAM_PORT=4317 NEXT_TELEMETRY_DISABLED=1
export SESSION_SECRET="phase3a-benchmark-session-secret-0123456789"
unset NEXTRS_VARIANT
node "$root/tests/compat/upstream.mjs" --port 4320 > /dev/null 2>&1 &
ledger=$!
trap 'kill $ledger 2>/dev/null || true' EXIT
for _ in $(seq 1 40); do curl -s --max-time 1 -o /dev/null "$NEXTRS_UPSTREAM/hits?key=x" && break; sleep 0.25; done

for f in "${fixtures[@]}"; do
  dir="$root/fixtures/$f"
  echo "== $f"
  (
    cd "$dir"
    npm ci --no-audit --no-fund --loglevel=error
    if [[ -f scripts/make-images.mjs ]]; then node scripts/make-images.mjs > /dev/null; fi
    rm -rf .next
    npx next build > "$root/benchmarks/phase3a/.build-$f.log" 2>&1 || { tail -40 "$root/benchmarks/phase3a/.build-$f.log"; exit 1; }
  )
  echo "   BUILD_ID $(cat "$dir/.next/BUILD_ID")  next $(node -p "require('$dir/node_modules/next/package.json').version")"
done
