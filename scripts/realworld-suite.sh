#!/usr/bin/env bash
# Builds the real-world fixtures (applications that use maintained
# libraries in proxy.ts the way their documentation shows) against one
# Next.js version and compares each with `next start`:
#
#   scripts/realworld-suite.sh [--next VERSION] [--isolates N] [--out DIR] [--browser]
#                              [--only auth,intl,platform]
#
#   auth      fixtures/next-auth-proxy      jose session cookies (tests/compat/auth-proxy.mjs)
#   intl      fixtures/next-intl-proxy      next-intl locale routing (tests/compat/intl-proxy.mjs)
#   platform  fixtures/next-platform-proxy  a proxy that fetches, external rewrites, instrumentation,
#                                           waitUntil, draft mode, dynamicParams (tests/compat/platform-proxy.mjs)
#
# Without --next each fixture's lockfile is used (npm ci, in place). With
# --next the fixture is copied to a temporary directory and that version is
# installed there. Needs target/release/nextrs, Node.js and npm; --browser
# also runs tests/browser/realworld.mjs (playwright-core, see tests/browser).
# next start listens on 0.0.0.0 (see tests/compat/proxy.mjs). Exits non-zero
# if any suite fails.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
next_version=""
isolates=1
out=""
browser=0
only="auth,intl,platform"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --next) next_version="$2"; shift 2 ;;
    --isolates) isolates="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --browser) browser=1; shift ;;
    --only) only="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

work=""
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill -- "-$pid" 2>/dev/null || kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  if [[ -n "${work:-}" ]]; then rm -rf "$work"; fi
}
trap cleanup EXIT

nextrs="$root/target/release/nextrs"
[[ -x "$nextrs" ]] || { echo "build target/release/nextrs first" >&2; exit 2; }
[[ -n "$next_version" ]] && work="$(mktemp -d)"
out="${out:-$(mktemp -d)}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"

base_port=$((5100 + RANDOM % 400))
next_port=$base_port
nextrs_port=$((base_port + 1))
upstream_port=$((base_port + 2))
closed_port=$((base_port + 3))
# A second upstream that NextRS's network policy does not list (the
# platform fixture's "unlisted" destinations): next start reaches it,
# NextRS must refuse it.
unlisted_port=$((base_port + 4))
export NEXTRS_UPSTREAM="http://127.0.0.1:$upstream_port"
export NEXTRS_CLOSED_PORT="$closed_port"
export NEXTRS_UNLISTED_PORT="$unlisted_port"
# Test-only secrets, fresh per run and never printed: the auth fixture's
# session key, the platform fixture's draft-mode secret, and the Server
# Action key both servers share.
SESSION_SECRET="$(head -c 32 /dev/urandom | base64 | tr -d '/+=')"
DRAFT_SECRET="$(head -c 18 /dev/urandom | base64 | tr -d '/+=')"
export SESSION_SECRET DRAFT_SECRET

node "$root/tests/compat/upstream.mjs" --port "$upstream_port" > "$out/upstream.log" 2>&1 &
pids+=($!)
node "$root/tests/compat/upstream.mjs" --port "$unlisted_port" > "$out/upstream-unlisted.log" 2>&1 &
pids+=($!)

# prepare FIXTURE -> sets $project (the directory both servers serve).
prepare() {
  local fixture="$1"
  if [[ -z "$next_version" ]]; then
    project="$root/fixtures/$fixture"
    (cd "$project" && npm ci --no-audit --no-fund --loglevel=error)
  else
    project="$work/$fixture"
    mkdir -p "$project"
    (cd "$root/fixtures/$fixture" && git ls-files -z . | xargs -0 -I{} cp --parents {} "$project/")
    (cd "$project" && npm install --no-audit --no-fund --loglevel=error --save-exact "next@$next_version")
  fi
  (cd "$project" && rm -rf .next && NEXT_TELEMETRY_DISABLED=1 npx next build > "$out/build-$fixture.log" 2>&1) || {
    tail -40 "$out/build-$fixture.log"; exit 1;
  }
  echo "$fixture: Next.js $(node -p "require('$project/node_modules/next/package.json').version") built"
}

next_pid=""
nextrs_pid=""
# start FIXTURE [nextrs flags...]
start() {
  local fixture="$1"; shift
  for port in "$next_port" "$nextrs_port"; do
    if curl -s -o /dev/null "http://127.0.0.1:$port/"; then echo "port $port is still in use" >&2; exit 1; fi
  done
  (cd "$project" && NEXT_TELEMETRY_DISABLED=1 exec setsid npx next start -H 0.0.0.0 -p "$next_port") >> "$out/next-start-$fixture.log" 2>&1 &
  next_pid=$!
  pids+=($next_pid)
  "$nextrs" start "$project" --port "$nextrs_port" --js-runtime embedded-v8 --isolates "$isolates" \
    --js-stall-timeout 3 --request-timeout 15 \
    --js-env NEXTRS_UPSTREAM --js-env NEXTRS_CLOSED_PORT --js-env NEXTRS_UNLISTED_PORT \
    --js-env SESSION_SECRET --js-env DRAFT_SECRET \
    --net-allow "http://127.0.0.1:$upstream_port" --net-allow "http://127.0.0.1:$closed_port" --net-allow-addr 127.0.0.1 "$@" >> "$out/nextrs-$fixture.log" 2>&1 &
  nextrs_pid=$!
  pids+=($nextrs_pid)
  for port in "$next_port" "$nextrs_port"; do
    for _ in $(seq 1 120); do
      curl -s -o /dev/null "http://127.0.0.1:$port/api/health" && break
      sleep 0.25
    done
    curl -s -o /dev/null "http://127.0.0.1:$port/api/health" || { echo "server on $port did not start" >&2; tail -20 "$out"/*-"$fixture".log; exit 1; }
  done
}
stop() {
  kill -TERM -- "-$next_pid" 2>/dev/null || kill -TERM "$next_pid" 2>/dev/null || true
  kill -TERM "$nextrs_pid" 2>/dev/null || true
  for port in "$next_port" "$nextrs_port"; do
    for _ in $(seq 1 80); do
      curl -s -o /dev/null "http://127.0.0.1:$port/" || break
      sleep 0.25
    done
  done
  if curl -s -o /dev/null "http://127.0.0.1:$next_port/" || curl -s -o /dev/null "http://127.0.0.1:$nextrs_port/"; then
    kill -KILL -- "-$next_pid" 2>/dev/null || true
    kill -KILL "$nextrs_pid" 2>/dev/null || true
    sleep 1
  fi
  for port in "$next_port" "$nextrs_port"; do
    if curl -s -o /dev/null "http://127.0.0.1:$port/"; then echo "a server on port $port did not stop" >&2; exit 1; fi
  done
  wait "$next_pid" "$nextrs_pid" 2>/dev/null || true
}

baseline="http://127.0.0.1:$next_port"
candidate="http://127.0.0.1:$nextrs_port"
status=0
run() {
  local name="$1"; shift
  if node "$@" > "$out/$name.txt" 2>&1; then
    echo "PASS  $name ($(tail -1 "$out/$name.txt"))"
  else
    echo "FAIL  $name"; tail -25 "$out/$name.txt"; status=1
  fi
}
cd "$root"
wants() { [[ ",$only," == *",$1,"* ]]; }

if wants auth; then
  prepare next-auth-proxy
  start next-auth-proxy
  run auth-proxy tests/compat/auth-proxy.mjs --baseline "$baseline" --candidate "$candidate" --upstream "$NEXTRS_UPSTREAM" \
    --dist "$project/.next" --json "$out/auth-proxy.json"
  if [[ $browser == 1 ]]; then
    run browser-auth tests/browser/realworld.mjs --app auth --baseline "$baseline" --candidate "$candidate" \
      --upstream "$NEXTRS_UPSTREAM" --json "$out/browser-auth.json"
  fi
  stop
fi

if wants intl && [[ -d "$root/fixtures/next-intl-proxy" ]]; then
  prepare next-intl-proxy
  start next-intl-proxy
  run intl-proxy tests/compat/intl-proxy.mjs --baseline "$baseline" --candidate "$candidate" --upstream "$NEXTRS_UPSTREAM" \
    --dist "$project/.next" --json "$out/intl-proxy.json"
  if [[ $browser == 1 ]]; then
    run browser-intl tests/browser/realworld.mjs --app intl --baseline "$baseline" --candidate "$candidate" \
      --upstream "$NEXTRS_UPSTREAM" --json "$out/browser-intl.json"
  fi
  stop
fi

if wants platform && [[ -d "$root/fixtures/next-platform-proxy" ]]; then
  prepare next-platform-proxy
  # External rewrites and the proxy's fetches reach the upstream and the
  # closed port only: they are the allowed destinations (127.0.0.1 the one
  # allowed non-public address); the unlisted upstream is refused.
  start next-platform-proxy
  run platform-proxy tests/compat/platform-proxy.mjs --baseline "$baseline" --candidate "$candidate" --upstream "$NEXTRS_UPSTREAM" \
    --dist "$project/.next" --isolates "$isolates" --json "$out/platform-proxy.json"
  stop
fi

echo "{\"isolates\":$isolates,\"status\":$status}" > "$out/summary.json"
exit $status
