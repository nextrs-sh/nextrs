#!/usr/bin/env bash
# Soak of the compiled-proxy path: starts `nextrs start` on a built proxy
# fixture, drives benchmarks/soak.mjs --app against it (proxy traffic,
# aborted clients, periodic CPU stalls injected in the proxy) and stops it.
#
#   benchmarks/proxy-soak.sh --app next-proxy|platform --out DIR \
#        [--nextrs target/release/nextrs] [--isolates 4] [--minutes 60] \
#        [--concurrency 24] [--inject-every 300] [--inject-ms 4000] [--stall-timeout 2]
#
#   next-proxy  fixtures/next-proxy (default build in .next)
#   platform    fixtures/next-platform-proxy, built by this script against
#               the upstreams it starts (its external rewrite destinations
#               are fixed at build time)
#
# Writes DIR/soak-<app>-i<N>.json (samples + counts), the soak log and the
# server log. Suspend is blocked with systemd-inhibit when available (a
# suspended host stops the clock the watchdogs and the samples rely on).
# Exits non-zero if the soak reports a failure.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
app=""
out=""
nextrs="$root/target/release/nextrs"
isolates=4
minutes=60
concurrency=24
inject_every=300
inject_ms=4000
stall_timeout=2
while [[ $# -gt 0 ]]; do
  case "$1" in
    --app) app="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --nextrs) nextrs="$2"; shift 2 ;;
    --isolates) isolates="$2"; shift 2 ;;
    --minutes) minutes="$2"; shift 2 ;;
    --concurrency) concurrency="$2"; shift 2 ;;
    --inject-every) inject_every="$2"; shift 2 ;;
    --inject-ms) inject_ms="$2"; shift 2 ;;
    --stall-timeout) stall_timeout="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ "$app" == next-proxy || "$app" == platform ]] && [[ -n "$out" ]] || { echo "--app next-proxy|platform and --out are required" >&2; exit 2; }
mkdir -p "$out"
out="$(cd "$out" && pwd)"

pids=()
trap 'for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done; wait 2>/dev/null || true' EXIT
base=$((5500 + RANDOM % 200))
port=$base
metrics=$((base + 1000))
upstream_port=$((base + 2))
closed_port=$((base + 3))
unlisted_port=$((base + 4))
export NEXTRS_UPSTREAM="http://127.0.0.1:$upstream_port" NEXTRS_CLOSED_PORT="$closed_port" NEXTRS_UNLISTED_PORT="$unlisted_port"
node "$root/tests/compat/upstream.mjs" --port "$upstream_port" > "$out/upstream-$app-i$isolates.log" 2>&1 &
pids+=($!)
node "$root/tests/compat/upstream.mjs" --port "$unlisted_port" > "$out/upstream-unlisted-$app-i$isolates.log" 2>&1 &
pids+=($!)

if [[ $app == next-proxy ]]; then
  project="$root/fixtures/next-proxy"
  serve="$project/.next"
  soak_flags=(--project "$project")
else
  project="$root/fixtures/next-platform-proxy"
  (cd "$project" && rm -rf .next && NEXT_TELEMETRY_DISABLED=1 npx next build > "$out/build-$app.log" 2>&1)
  serve="$project"
  soak_flags=(--upstream "$NEXTRS_UPSTREAM")
fi
[[ -f "$serve/BUILD_ID" || -f "$serve/.next/BUILD_ID" ]] || { echo "build $project first" >&2; exit 2; }

inhibit=()
command -v systemd-inhibit > /dev/null && inhibit=(systemd-inhibit --what=sleep:idle --who=nextrs-proxy-soak --why="proxy soak")
"$nextrs" start "$serve" --port "$port" --js-runtime embedded-v8 --isolates "$isolates" \
  --js-stall-timeout "$stall_timeout" --request-timeout 15 --metrics-addr "127.0.0.1:$metrics" \
  --js-env NEXTRS_UPSTREAM --js-env NEXTRS_CLOSED_PORT --js-env NEXTRS_UNLISTED_PORT \
  --net-allow "$NEXTRS_UPSTREAM" --net-allow "http://127.0.0.1:$closed_port" --net-allow-addr 127.0.0.1 \
  > "$out/server-$app-i$isolates.log" 2>&1 &
server=$!
pids+=($server)
for _ in $(seq 1 120); do
  curl -sf -o /dev/null "http://127.0.0.1:$port/api/health" && break
  sleep 0.25
done
echo "$(date -Is) app=$app isolates=$isolates pid=$server port=$port: soak for $minutes min"
status=0
"${inhibit[@]}" node "$root/benchmarks/soak.mjs" --app "$app" --target "http://127.0.0.1:$port" --pid "$server" \
  --metrics "http://127.0.0.1:$metrics" --minutes "$minutes" --concurrency "$concurrency" \
  --inject-every "$inject_every" --inject-ms "$inject_ms" --sample-every 30 \
  --json "$out/soak-$app-i$isolates.json" "${soak_flags[@]}" > "$out/soak-$app-i$isolates.log" 2>&1 || status=1
tail -n 40 "$out/soak-$app-i$isolates.log" | sed -n '/^{$/,$p'
kill -TERM "$server"
wait "$server" 2>/dev/null || true
exit $status
