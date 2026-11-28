#!/bin/bash
# RE-4 end-to-end: the release CLI binary serving fixtures/next-app with
# each opt-in resource control. Usage: run.sh <label> <port> <flags...>
set -u
BIN=/home/dennispaler/Documents/OpenSource/nextrs/target/release/nextrs
APP=/home/dennispaler/Documents/OpenSource/nextrs/fixtures/next-app
OUT=/home/dennispaler/.cache/nextrs-phase3a/re4/e2e
label=$1; port=$2; shift 2
mport=$((port + 1))
CHUNK=/_next/static/chunks/39_j1bdzreq0m.js
RUST_LOG=info "$BIN" start "$APP" --js-runtime embedded-v8 --port $port --metrics-addr 127.0.0.1:$mport "$@" > "$OUT/$label.log" 2>&1 &
pid=$!
for i in $(seq 1 120); do curl -fs "http://127.0.0.1:$mport/readyz" >/dev/null 2>&1 && break; sleep 0.5; done
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
{
echo "label=$label flags=$*"
echo "readyz $(code http://127.0.0.1:$mport/readyz)"
echo "healthz $(code http://127.0.0.1:$mport/healthz)"
for p in / /dynamic /books/42 /api/health /data-fetch /request-info "$CHUNK"; do echo "GET $p $(code http://127.0.0.1:$port$p)"; done
# Concurrent dynamic requests.
seq 1 24 | xargs -P 24 -I{} curl -s -o /dev/null -D - "http://127.0.0.1:$port/dynamic?i={}" 2>/dev/null \
  | awk 'toupper($1) ~ /^HTTP/ {s[$2]++} tolower($1)=="x-nextrs-error:" {e[$2]++} tolower($1)=="retry-after:" {r[$2]++}
         END {for (k in s) printf "concurrent-status %s %d\n", k, s[k]; for (k in e) printf "concurrent-error %s %d\n", k, e[k]; for (k in r) printf "concurrent-retry-after %s %d\n", k, r[k]}'
# Static and health while that happens.
echo "static-after $(code http://127.0.0.1:$port$CHUNK) health-after $(code http://127.0.0.1:$mport/healthz)"
curl -s "http://127.0.0.1:$mport/metrics" | grep -E "^nextrs_(admission_state|admission_rejected|admission_progress|admission_deferred|executor_rejected_total|external_memory|native_buffer|executor_heap_limit_bytes)" | grep -v " 0$" | head -30
grep -E "RssAnon|VmHWM|VmRSS" /proc/$pid/status
} > "$OUT/$label.txt" 2>&1
kill -TERM $pid; wait $pid 2>/dev/null
grep -iE "admission|external|native buffer|young|heap limit|panick|error" "$OUT/$label.log" | head -12 >> "$OUT/$label.txt"
cat "$OUT/$label.txt"
