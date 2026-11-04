#!/usr/bin/env bash
# Long soak per isolate-pool size: starts `nextrs start` on a built fixture,
# drives benchmarks/soak.mjs against it (mixed traffic, slow and aborted
# clients, periodic CPU-stall injection) and stops it again.
#
#   benchmarks/soak-matrix.sh --project fixtures/next-app --out DIR \
#        [--nextrs target/release/nextrs] [--isolates "1 2 4"] [--minutes 90] \
#        [--concurrency 24] [--inject-every 180] [--inject-ms 4000] [--stall-timeout 2]
#
# Writes DIR/soak-i<N>.json (samples + counts) and DIR/server-i<N>.log per
# pool size. Exits non-zero if any soak run reports a failure (a leak, a
# corrupted body or an unexpected status).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
project=""
out=""
nextrs="$root/target/release/nextrs"
isolates="1 2 4"
minutes=90
concurrency=24
inject_every=180
inject_ms=4000
stall_timeout=2
while [[ $# -gt 0 ]]; do
  case "$1" in
    --project) project="$2"; shift 2 ;;
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
[[ -n "$project" && -n "$out" ]] || { echo "--project and --out are required" >&2; exit 2; }
mkdir -p "$out"
out="$(cd "$out" && pwd)"

server=""
trap '[[ -n "$server" ]] && kill "$server" 2>/dev/null; wait 2>/dev/null || true' EXIT

status=0
for n in $isolates; do
  port=$((4600 + RANDOM % 300))
  metrics=$((port + 1000))
  "$nextrs" start "$project" --port "$port" --js-runtime embedded-v8 --isolates "$n" \
    --js-stall-timeout "$stall_timeout" --metrics-addr "127.0.0.1:$metrics" \
    > "$out/server-i$n.log" 2>&1 &
  server=$!
  for _ in $(seq 1 120); do
    curl -sf -o /dev/null "http://127.0.0.1:$port/api/health" && break
    sleep 0.25
  done
  echo "$(date -Is) isolates=$n pid=$server port=$port: soak for $minutes min"
  node "$root/benchmarks/soak.mjs" --target "http://127.0.0.1:$port" --pid "$server" \
    --metrics "http://127.0.0.1:$metrics" --minutes "$minutes" --concurrency "$concurrency" \
    --inject-every "$inject_every" --inject-ms "$inject_ms" --sample-every 30 \
    --json "$out/soak-i$n.json" > "$out/soak-i$n.log" 2>&1 || status=1
  tail -n 30 "$out/soak-i$n.log" | sed -n '/^{$/,$p'
  kill -TERM "$server"
  wait "$server" 2>/dev/null || true
  server=""
done
exit $status
