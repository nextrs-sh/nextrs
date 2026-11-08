#!/usr/bin/env bash
# Image soak: builds fixtures/next-image (default build, upstream port
# 4317), starts `nextrs start` with a fresh image cache directory, and drives
# benchmarks/image_soak.mjs against it (image traffic beside SSR, Server
# Actions, ISR and proxy.ts; restarts injected; the cache directory made
# read-only for a while).
#
#   benchmarks/image-soak.sh --out DIR [--nextrs target/release/nextrs] [--isolates 1]
#        [--minutes 60] [--concurrency 24] [--inject-every 300] [--inject-ms 3000]
#        [--stall-timeout 1] [--readonly-at 20] [--readonly-for 5] [--build]
#
# Writes DIR/image-soak-i<N>.json and the server log. Suspend is blocked
# with systemd-inhibit when available. Exits non-zero if the soak failed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
project="$root/fixtures/next-image"
out="" nextrs="$root/target/release/nextrs" isolates=1 minutes=60 concurrency=24
inject_every=300 inject_ms=3000 stall_timeout=1 readonly_at=20 readonly_for=5 build=0
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
    --readonly-at) readonly_at="$2"; shift 2 ;;
    --readonly-for) readonly_for="$2"; shift 2 ;;
    --build) build=1; shift ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -n $out ]] || { sed -n 2,13p "$0"; exit 2; }
mkdir -p "$out"; out="$(cd "$out" && pwd)"
port=5920 metrics=5921
export NEXTRS_IMAGE_UPSTREAM_PORT=4317
if [[ $build == 1 ]] || ! grep -q '"4317"' "$project/.next/images-manifest.json" 2>/dev/null || [[ ! -d "$project/.next/server/app/soak" ]]; then
  (cd "$project" && npm ci --no-audit --no-fund --loglevel=error && node scripts/make-images.mjs > /dev/null &&
    rm -rf .next && NEXT_TELEMETRY_DISABLED=1 npx next build > "$out/build.log" 2>&1)
fi
cache_dir="$out/image-cache-i$isolates"
rm -rf "$cache_dir"
inhibit=()
command -v systemd-inhibit > /dev/null && inhibit=(systemd-inhibit --what=sleep:idle --who=nextrs-image-soak --why="image soak")
"$nextrs" start "$project/.next" --port "$port" --js-runtime embedded-v8 --isolates "$isolates" \
  --js-stall-timeout "$stall_timeout" --request-timeout 15 --metrics-addr "127.0.0.1:$metrics" \
  --net-allow "http://127.0.0.1:4317" --net-allow-addr 127.0.0.1 \
  --image-cache-dir "$cache_dir" --image-cache-max-disk 64 > "$out/server-i$isolates.log" 2>&1 &
server=$!
trap 'kill $server 2>/dev/null || true; chmod 700 "$cache_dir" "$cache_dir/tmp" 2>/dev/null || true' EXIT
for _ in $(seq 1 120); do curl -s --max-time 2 -o /dev/null "http://127.0.0.1:$port/soak/isr" && break; sleep 0.25; done
status=0
"${inhibit[@]}" node "$root/benchmarks/image_soak.mjs" --target "http://127.0.0.1:$port" --metrics "http://127.0.0.1:$metrics" \
  --project "$project" --minutes "$minutes" --concurrency "$concurrency" --inject-every "$inject_every" --inject-ms "$inject_ms" \
  --cache-dir "$cache_dir" --readonly-at "$readonly_at" --readonly-for "$readonly_for" \
  --json "$out/image-soak-i$isolates.json" > "$out/image-soak-i$isolates.txt" 2>&1 || status=$?
echo "soak exit $status: $out/image-soak-i$isolates.json"
exit $status
