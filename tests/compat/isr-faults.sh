#!/usr/bin/env bash
# ISR under faults, NextRS only (next start has no isolates or stall
# watchdog to compare with). Needs a built fixtures/next-app and
# target/release/nextrs.
#
#   tests/compat/isr-faults.sh [--out DIR]
#
# 1. isolate-terminated: a background regeneration of /isr/flaky spins the
#    CPU past --js-stall-timeout; the isolate is replaced. The previous
#    page keeps being served, nothing partial is stored, and a later
#    regeneration succeeds.
# 2. killed-before-publication: the process is killed (SIGKILL) while a
#    regeneration renders; after a restart on the same --cache-dir the
#    previous complete entry is served and no file is corrupt.
# 3. write-failure: the entries directory becomes read-only; a
#    regeneration is served from memory and the failed write is counted;
#    what a restart then serves is reported (the last durable state).
# Prints one JSON line per case; exits non-zero if a check fails.
set -uo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
project="$root/fixtures/next-app"
nextrs="$root/target/release/nextrs"
out="$(mktemp -d)"
[[ "${1:-}" == "--out" ]] && out="$2"
mkdir -p "$out"
port=$((5600 + RANDOM % 200))
metrics=$((port + 1))
upstream_port=$((port + 2))
export NEXTRS_UPSTREAM="http://127.0.0.1:$upstream_port"
cache="$out/cache"
pids=()
cleanup() {
  chmod -R u+w "$cache" 2>/dev/null || true
  for pid in "${pids[@]}"; do kill -9 "$pid" 2>/dev/null || true; done
}
trap cleanup EXIT

node "$root/tests/compat/upstream.mjs" --port "$upstream_port" > "$out/upstream.log" 2>&1 &
pids+=($!)
server=""
start() {
  "$nextrs" start "$project" --port "$port" --js-runtime embedded-v8 --isolates 2 \
    --js-stall-timeout 2 --request-timeout 15 --metrics-addr "127.0.0.1:$metrics" \
    --js-env NEXTRS_UPSTREAM --net-allow "$NEXTRS_UPSTREAM" --net-allow-addr 127.0.0.1 \
    --cache-dir "$cache" >> "$out/nextrs.log" 2>&1 &
  server=$!
  pids+=($server)
  for _ in $(seq 1 120); do curl -sf -o /dev/null "http://127.0.0.1:$port/api/health" && return; sleep 0.25; done
  echo "nextrs did not start" >&2; tail -20 "$out/nextrs.log"; exit 1
}
flag() { curl -s -X POST "$NEXTRS_UPSTREAM/flag?name=$1&on=$2" > /dev/null; }
page() { # path -> "status cache stamp"
  local h; h=$(curl -s -D - -o "$out/body" "http://127.0.0.1:$port$1")
  echo "$(echo "$h" | head -1 | awk '{print $2}') $(echo "$h" | tr -d '\r' | awk 'tolower($1)=="x-nextjs-cache:"{print $2}') $(grep -o 'stamp-[0-9]*' "$out/body" | head -1)"
}
metric() { curl -s "http://127.0.0.1:$metrics/metrics" | awk -v m="$1" '$1==m{print $2}'; }
status=0
check() { if [[ "$2" == true ]]; then echo "  ok   $1"; else echo "  FAIL $1"; status=1; fi; }

# --- 1. isolate terminated mid-regeneration ---------------------------------
start
page /isr/flaky > /dev/null; sleep 1.2; page /isr/flaky > /dev/null; sleep 0.5
good=$(page /isr/flaky | awk '{print $3}')
restarts0=$(curl -s "http://127.0.0.1:$metrics/metrics.json" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).executors.reduce((a,e)=>a+e.restarts,0)))')
flag isr-spin 1
sleep 1.2
stale=$(page /isr/flaky)            # stale page; its regeneration spins
sleep 3                              # watchdog (2 s) replaces the isolate
during=$(page /isr/flaky)
flag isr-spin 0
sleep 3.5
after1=$(page /isr/flaky); sleep 1; after2=$(page /isr/flaky)
restarts1=$(curl -s "http://127.0.0.1:$metrics/metrics.json" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).executors.reduce((a,e)=>a+e.restarts,0)))')
echo "{\"case\":\"isolate-terminated\",\"good\":\"$good\",\"stale\":\"$stale\",\"during\":\"$during\",\"after\":[\"$after1\",\"$after2\"],\"restarts\":$((restarts1 - restarts0))}"
check "isolate replaced" "$([[ $((restarts1 - restarts0)) -ge 1 ]] && echo true || echo false)"
check "stale page served while regenerating" "$([[ "$stale" == "200 STALE $good" ]] && echo true || echo false)"
check "previous page still served after the failure" "$([[ "$during" == 200*"$good" ]] && echo true || echo false)"
check "a later regeneration succeeds" "$([[ "$after2" == 200* && "$after2" != *"$good" ]] && echo true || echo false)"

# --- 2. killed between rendering and publication ----------------------------
before=$(page /isr/flaky | awk '{print $3}')
sleep 1.2
flag isr-spin 1
page /isr/flaky > /dev/null          # starts a spinning regeneration
sleep 0.5
kill -9 "$server"; wait "$server" 2>/dev/null
flag isr-spin 0
start
first=$(page /isr/flaky)
corrupt=$(metric 'nextrs_cache_errors_total{reason="corrupt"}')
leftovers=$(ls "$cache"/*/tmp 2>/dev/null | wc -l)
echo "{\"case\":\"killed-before-publication\",\"before\":\"$before\",\"afterRestart\":\"$first\",\"corrupt\":${corrupt:-0},\"tmpLeftovers\":$leftovers}"
check "previous entry served after restart" "$([[ "$first" == 200*"$before" ]] && echo true || echo false)"
check "no corrupt entry" "$([[ "${corrupt:-0}" == 0 ]] && echo true || echo false)"

# --- 3. write failure --------------------------------------------------------
slug="wf-$RANDOM"
v1=$(page "/isr/$slug" | awk '{print $3}')    # stored on disk
sleep 0.3
chmod u-w "$cache"/*/entries "$cache"/*/tmp
# Expire its tag (isr-slug-<slug>); the regeneration cannot be written.
curl -s -o /dev/null "http://127.0.0.1:$port/api/revalidate?tag=isr-slug-$slug"
v2=$(page "/isr/$slug"); v3=$(page "/isr/$slug")
io=$(metric 'nextrs_cache_errors_total{reason="io"}')
chmod u+w "$cache"/*/entries "$cache"/*/tmp
kill -TERM "$server"; wait "$server" 2>/dev/null
start
v4=$(page "/isr/$slug")
echo "{\"case\":\"write-failure\",\"stored\":\"$v1\",\"afterRevalidation\":[\"$v2\",\"$v3\"],\"ioErrors\":${io:-0},\"afterRestart\":\"$v4\"}"
check "regenerated page served from memory" "$([[ "$v2" == 200*MISS* && "$v2" != *"$v1" && "$v3" == "200 HIT"* ]] && echo true || echo false)"
check "failed write counted" "$([[ "${io:-0}" -ge 1 ]] && echo true || echo false)"
# Reported, not checked: with the directory unwritable, neither the new
# page nor the tag revalidation reached the disk, so a restart serves the
# last state that did (next start, whose tags live in memory only, would
# too). Persistence lags a failing disk; it is never torn or corrupt.
if [[ "$v4" == *" $v1" ]]; then
  echo "  note after a restart the last durably written page is served again (the revalidation was memory-only)"
else
  echo "  note after a restart the revalidated state was kept"
fi
kill -TERM "$server"
exit $status
