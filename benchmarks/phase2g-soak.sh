#!/usr/bin/env bash
# Phase 2G mixed soak: builds fixtures/next-soak-mixed (App Router, Pages
# Router, next/image, one proxy) and fixtures/next-pages-i18n (native i18n),
# starts the test upstream (mutation ledger), two `nextrs start` processes
# with metrics listeners, and drives benchmarks/phase2g_soak.mjs against
# both (restarts injected into the mixed process).
#
#   benchmarks/phase2g-soak.sh --out DIR [--nextrs target/release/nextrs] [--isolates 4]
#        [--minutes 360] [--concurrency 24] [--inject-every 300] [--inject-ms 3000]
#        [--stall-timeout 1] [--build]
#
# Writes DIR/phase2g-soak-i<N>.json and both server logs. Suspend is blocked
# with systemd-inhibit when available. Exits non-zero if the soak failed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
mixed="$root/fixtures/next-soak-mixed"
i18n="$root/fixtures/next-pages-i18n"
out="" nextrs="$root/target/release/nextrs" isolates=4 minutes=360 concurrency=24
inject_every=300 inject_ms=3000 stall_timeout=1 build=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) out="$2"; shift 2 ;;
    --nextrs) nextrs="$2"; shift 2 ;;
    --isolates) isolates="$2"; shift 2 ;;
    --minutes) minutes="$2"; shift 2 ;;
    --concurrency) concurrency="$2"; shift 2 ;;
    --inject-every) inject_every="$2"; shift 2 ;;
    --inject-ms) inject_ms="$2"; shift 2 ;;
    --stall-timeout) stall_timeout="$2"; shift 2 ;;
    --build) build=1; shift ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -n $out ]] || { sed -n 2,13p "$0"; exit 2; }
mkdir -p "$out"; out="$(cd "$out" && pwd)"
port=5930 metrics=5931 i18n_port=5932 i18n_metrics=5933 ledger_port=5934
export NEXTRS_IMAGE_UPSTREAM_PORT=4317 NEXTRS_UPSTREAM="http://127.0.0.1:$ledger_port" NEXT_TELEMETRY_DISABLED=1
node "$root/tests/compat/upstream.mjs" --port "$ledger_port" > "$out/ledger.log" 2>&1 &
ledger=$!
servers=()
trap 'kill $ledger "${servers[@]}" 2>/dev/null || true' EXIT
for _ in $(seq 1 40); do curl -s --max-time 1 -o /dev/null "$NEXTRS_UPSTREAM/hits?key=x" && break; sleep 0.25; done
if [[ $build == 1 || ! -d "$mixed/.next/server/app/soak" ]]; then
  (cd "$mixed" && npm ci --no-audit --no-fund --loglevel=error && node scripts/make-images.mjs > /dev/null &&
    rm -rf .next && npx next build > "$out/build-mixed.log" 2>&1)
fi
if [[ $build == 1 || ! -f "$i18n/.next/BUILD_ID" ]]; then
  (cd "$i18n" && npm ci --no-audit --no-fund --loglevel=error && rm -rf .next && npx next build > "$out/build-i18n.log" 2>&1)
fi
cache_dir="$out/data-cache-i$isolates" image_cache="$out/image-cache-i$isolates"
rm -rf "$cache_dir" "$image_cache"
common=(--js-runtime embedded-v8 --isolates "$isolates" --js-stall-timeout "$stall_timeout" --request-timeout 15
  --js-env NEXTRS_UPSTREAM --net-allow "$NEXTRS_UPSTREAM" --net-allow-addr 127.0.0.1)
"$nextrs" start "$mixed/.next" --port "$port" --metrics-addr "127.0.0.1:$metrics" "${common[@]}" \
  --net-allow "http://127.0.0.1:4317" --cache-dir "$cache_dir" \
  --image-cache-dir "$image_cache" --image-cache-max-disk 64 > "$out/server-mixed-i$isolates.log" 2>&1 &
servers+=($!)
"$nextrs" start "$i18n/.next" --port "$i18n_port" --metrics-addr "127.0.0.1:$i18n_metrics" "${common[@]}" \
  > "$out/server-i18n-i$isolates.log" 2>&1 &
servers+=($!)
for p in "$port/soak/isr" "$i18n_port/api/whoami"; do
  for _ in $(seq 1 120); do curl -s --max-time 2 -o /dev/null "http://127.0.0.1:$p" && break; sleep 0.25; done
done
inhibit=()
command -v systemd-inhibit > /dev/null && inhibit=(systemd-inhibit --what=sleep:idle --who=nextrs-phase2g-soak --why="mixed soak")
status=0
"${inhibit[@]}" node "$root/benchmarks/phase2g_soak.mjs" --target "http://127.0.0.1:$port" --metrics "http://127.0.0.1:$metrics" \
  --project "$mixed" --ledger "$NEXTRS_UPSTREAM" --pid "${servers[0]}" --i18n-pid "${servers[1]}" \
  --i18n-target "http://127.0.0.1:$i18n_port" --i18n-metrics "http://127.0.0.1:$i18n_metrics" --i18n-dist "$i18n/.next" \
  --minutes "$minutes" --concurrency "$concurrency" --inject-every "$inject_every" --inject-ms "$inject_ms" \
  --json "$out/phase2g-soak-i$isolates.json" > "$out/phase2g-soak-i$isolates.txt" 2>&1 || status=$?
echo "soak exit $status: $out/phase2g-soak-i$isolates.json"
exit $status
