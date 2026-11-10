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
# fixtures/next-pages and fixtures/next-pages-i18n (default variants) are
# built here, against the test upstream this script starts: the ISR
# scenarios compare the upstream's counts with what the build fetched.
# The i18n controls run tests/compat/pages-i18n.mjs.
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
for _ in $(seq 1 40); do curl -s --max-time 1 -o /dev/null "$NEXTRS_UPSTREAM/hits?key=x" && break; sleep 0.25; done
for fixture in "$project" "$root/fixtures/next-pages-i18n"; do
  (cd "$fixture" && rm -rf .next && NEXT_TELEMETRY_DISABLED=1 npx next build > "$out/build-$(basename "$fixture").log" 2>&1) || {
    tail -30 "$out/build-$(basename "$fixture").log"; exit 1;
  }
done
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
  [skip-preview]="pages"
)
caught=0
missed=0
summary="$out/summary.txt"
: > "$summary"
# `none` first: the same binary without a defect must pass every suite,
# or a "caught" control proves nothing.
suites[none]="pages isolation"
for control in none wrong-page skip-ssr skip-api wrong-params cross-user replay-post skip-proxy-data wrong-data-route skip-preview; do
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
  if [[ $control == none ]]; then
    if [[ ${#failed[@]} -gt 0 ]]; then
      echo "BASELINE FAILED without a defect (${failed[*]})" | tee -a "$summary"; exit 1
    fi
    echo "BASELINE  passes without a defect" | tee -a "$summary"
  elif [[ ${#failed[@]} -gt 0 ]]; then
    caught=$((caught + 1)); echo "CAUGHT  $control  (${failed[*]})" | tee -a "$summary"
  else
    missed=$((missed + 1)); echo "MISSED  $control" | tee -a "$summary"
  fi
done
# Native i18n (fixtures/next-pages-i18n): each control must fail
# tests/compat/pages-i18n.mjs.
kill -- "-${pids[1]}" 2>/dev/null || kill "${pids[1]}" 2>/dev/null || true
for _ in $(seq 1 40); do curl -s --max-time 1 -o /dev/null "$baseline/" || break; sleep 0.25; done
i18n="$root/fixtures/next-pages-i18n"
(cd "$i18n" && NEXTRS_SIDE=baseline NEXT_TELEMETRY_DISABLED=1 exec setsid npx next start -H 0.0.0.0 -p "$next_port") > "$out/next-start-i18n.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 120); do curl -s --max-time 2 -o /dev/null "http://127.0.0.1:$next_port/api/whoami" && break; sleep 0.25; done
for control in none wrong-locale locale-cache-leak skip-preview skip-proxy-locale replay-post cross-user skip-api wrong-data-route; do
  NEXTRS_SIDE=candidate NEXTRS_PAGES_NEGATIVE_CONTROL="$control" "$binary" start "$i18n" --port "$nextrs_port" --js-runtime embedded-v8 \
    --js-env NEXTRS_UPSTREAM --js-env NEXTRS_SIDE --net-allow "$NEXTRS_UPSTREAM" --net-allow-addr 127.0.0.1 > "$out/nextrs-i18n-$control.log" 2>&1 &
  pid=$!
  for _ in $(seq 1 120); do curl -s --max-time 2 -o /dev/null "$candidate/api/whoami" && break; sleep 0.25; done
  if node "$root/tests/compat/pages-i18n.mjs" --baseline "$baseline" --candidate "$candidate" --upstream "$NEXTRS_UPSTREAM" \
    --dist "$i18n/.next" --variant default > "$out/i18n-$control.txt" 2>&1; then
    if [[ $control == none ]]; then echo "BASELINE  i18n passes without a defect" | tee -a "$summary"
    else missed=$((missed + 1)); echo "MISSED  i18n:$control" | tee -a "$summary"; fi
  elif [[ $control == none ]]; then
    echo "BASELINE FAILED  i18n without a defect" | tee -a "$summary"; exit 1
  else
    caught=$((caught + 1)); echo "CAUGHT  i18n:$control  ($(grep -cE '^(FAIL|DIFF)' "$out/i18n-$control.txt") scenarios)" | tee -a "$summary"
  fi
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  for _ in $(seq 1 40); do curl -s --max-time 1 -o /dev/null "$candidate/" || break; sleep 0.25; done
done
echo "negative controls: $caught caught, $missed missed" | tee -a "$summary"
[[ $missed == 0 ]]
