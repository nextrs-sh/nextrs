#!/usr/bin/env bash
# Image memory workloads (benchmarks/image_memory.mjs), each against a fresh
# server on fixtures/next-image (default build, upstream port 4317):
#
#   benchmarks/image-memory.sh --server next|PATH_TO_NEXTRS --label NAME --out DIR
#                              [--workloads "burst24 thumbs24 full24 mixed"] [-- EXTRA_NEXTRS_FLAGS...]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
project="$root/fixtures/next-image"
server="" label="" out="" workloads="burst24 thumbs24 full24 mixed" extra=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --server) server="$2"; shift 2 ;;
    --label) label="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --workloads) workloads="$2"; shift 2 ;;
    --) shift; extra=("$@"); break ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
mkdir -p "$out"; out="$(cd "$out" && pwd)"
port=5910 metrics_port=5911
listener_pid() { ss -ltnpH "sport = :$1" | sed -n 's/.*pid=\([0-9]*\).*/\1/p' | head -1; }
stop() {
  for p in $port $metrics_port; do local pid; pid="$(listener_pid $p)"; [[ -n $pid ]] && kill "$pid" 2>/dev/null || true; done
  for _ in $(seq 1 40); do [[ -z "$(listener_pid $port)" ]] && return; sleep 0.25; done
}
trap stop EXIT
for workload in $workloads; do
  stop
  if [[ $server == next ]]; then
    rm -rf "$project/.next/cache/images"
    (cd "$project" && NEXT_TELEMETRY_DISABLED=1 exec setsid npx next start -H 127.0.0.1 -p $port) >> "$out/$label-server.log" 2>&1 &
    metrics=()
  else
    "$server" start "$project/.next" --port $port --js-runtime embedded-v8 --metrics-addr 127.0.0.1:$metrics_port \
      --net-allow "http://127.0.0.1:4317" --net-allow-addr 127.0.0.1 "${extra[@]}" >> "$out/$label-server.log" 2>&1 &
    metrics=(--metrics "http://127.0.0.1:$metrics_port")
  fi
  for _ in $(seq 1 120); do curl -s --max-time 2 -o /dev/null "http://127.0.0.1:$port/" && break; sleep 0.25; done
  sleep 1
  node "$root/benchmarks/image_memory.mjs" --server "http://127.0.0.1:$port" "${metrics[@]}" --workload "$workload" \
    --out "$out/$label-$workload.json" > /dev/null
  python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print(sys.argv[2], d['workload'], d['statuses'], 'peak', d['peak'], 'after', d['after']['rssMiB'], 's', d['seconds'])" "$out/$label-$workload.json" "$label"
done
