#!/usr/bin/env bash
# Negative controls for the Pages Router suites: each deliberate defect
# (crates/nextrs-server/src/pages_controls.rs, compiled only with the
# `negative-controls` feature) must make at least one suite fail.
#
#   scripts/pages-negative-controls.sh --binary target/negative/nextrs [--out DIR]
#
# Build the binary with
#   cargo build --release -p nextrs-cli --features nextrs-server/negative-controls
# and copy it out of target/release (release builds never enable the feature).
# fixtures/next-pages must be built (default variant) against the test
# upstream this script starts (NEXTRS_UPSTREAM is baked into nothing; the
# pages read it at request time).
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
binary=""
out=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) binary="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ -x "$binary" ]] || { echo "--binary must be the negative-controls build" >&2; exit 2; }
out="${out:-$(mktemp -d)}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
project="$root/fixtures/next-pages"

port_in_use() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
for _ in $(seq 1 50); do
  base_port=$((5100 + RANDOM % 600))
  free=1
  for offset in $(seq 0 4); do
    if port_in_use $((base_port + offset)); then free=0; break; fi
  done
  [[ $free == 1 ]] && break
done
next_port=$base_port
nextrs_port=$((base_port + 1))
upstream_port=$((base_port + 2))
export NEXTRS_UPSTREAM="http://127.0.0.1:$upstream_port"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill -- "-$pid" 2>/dev/null || kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
}
trap cleanup EXIT
node "$root/tests/compat/upstream.mjs" --port "$upstream_port" > "$out/upstream.log" 2>&1 &
pids+=($!)
rm -rf "$project/.next/server/route-cache"
(cd "$project" && NEXT_TELEMETRY_DISABLED=1 exec setsid npx next start -H 0.0.0.0 -p "$next_port") > "$out/next-start.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 120); do curl -s --max-time 2 -o /dev/null "http://127.0.0.1:$next_port/api/health" && break; sleep 0.25; done

baseline="http://127.0.0.1:$next_port"
candidate="http://127.0.0.1:$nextrs_port"
# control -> the suites that must catch it
declare -A suites=(
  [wrong-page]="pages"
  [skip-ssr]="pages isolation"
  [skip-api]="pages isolation"
  [wrong-params]="pages isolation"
  [cross-user]="isolation"
  [replay-post]="pages isolation"
  [skip-proxy-data]="pages"
  [wrong-data-route]="pages"
)
caught=0
missed=0
summary="$out/summary.txt"
: > "$summary"
for control in wrong-page skip-ssr skip-api wrong-params cross-user replay-post skip-proxy-data wrong-data-route; do
  NEXTRS_PAGES_NEGATIVE_CONTROL="$control" "$binary" start "$project" --port "$nextrs_port" --js-runtime embedded-v8 \
    --js-env NEXTRS_UPSTREAM --net-allow "$NEXTRS_UPSTREAM" --net-allow-addr 127.0.0.1 > "$out/nextrs-$control.log" 2>&1 &
  pid=$!
  for _ in $(seq 1 120); do curl -s --max-time 2 -o /dev/null "$candidate/api/health" && break; sleep 0.25; done
  failed=()
  for suite in ${suites[$control]}; do
    case "$suite" in
      pages) cmd=(node "$root/tests/compat/pages.mjs" --baseline "$baseline" --candidate "$candidate" --upstream "$NEXTRS_UPSTREAM" --dist "$project/.next") ;;
      isolation) cmd=(node "$root/tests/compat/pages-isolation.mjs" --target "$candidate" --upstream "$NEXTRS_UPSTREAM" --dist "$project/.next" --requests 400) ;;
    esac
    if ! "${cmd[@]}" > "$out/$control-$suite.txt" 2>&1; then failed+=("$suite"); fi
  done
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  for _ in $(seq 1 40); do curl -s --max-time 1 -o /dev/null "$candidate/" || break; sleep 0.25; done
  if [[ ${#failed[@]} -gt 0 ]]; then
    caught=$((caught + 1)); echo "CAUGHT  $control  (${failed[*]})" | tee -a "$summary"
  else
    missed=$((missed + 1)); echo "MISSED  $control" | tee -a "$summary"
  fi
done
echo "negative controls: $caught caught, $missed missed" | tee -a "$summary"
[[ $missed == 0 ]]
