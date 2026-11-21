#!/usr/bin/env bash
# Prove that http://localhost:3000 is served by the native `nextrs` binary from
# @nextrs/cli@0.1.0-rc.1, and that no Node.js / `next start` process serves the storefront.
#
#   integration/velasia-airways/scripts/verify-runtime.sh            # writes results/runtime-verification.txt
#
# Needs: docker (compose v2), curl, sha256sum on the host. Exit status 0 only if every check passes.
set -uo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
compose=(docker compose -f "$here/compose.yaml")
base=${SMOKE_BASE_URL:-http://localhost:3000}
out=${VERIFY_OUT:-$here/results/runtime-verification.txt}
mkdir -p "$(dirname "$out")"
exec > >(tee "$out") 2>&1

expected_version="0.1.0-rc.1"
expected_sha="44f04e586c7dc9eb0b8f69dfd992143f725472aa5f3619df5a4039d77d2c6956"
fails=0
pass() { printf 'PASS  %s\n' "$*"; }
fail() { printf 'FAIL  %s\n' "$*"; fails=$((fails + 1)); }
section() { printf '\n== %s\n' "$*"; }
web() { "${compose[@]}" exec -T velasia-web "$@"; }
# bash /dev/tcp client inside the container (the runtime image has no curl, wget or node).
ops_get() {
  web bash -c "exec 3<>/dev/tcp/127.0.0.1/9100 && printf 'GET $1 HTTP/1.0\r\nHost: localhost\r\n\r\n' >&3 && cat <&3"
}

echo "NextRS x Velasia Airways runtime verification — $(date -u +%FT%TZ)"
echo "target: $base"

section "1. Containers"
"${compose[@]}" ps --format 'table {{.Service}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}'
web_id=$("${compose[@]}" ps -q velasia-web)
api_id=$("${compose[@]}" ps -q velasia-api)
[ -n "$web_id" ] && pass "velasia-web container running ($web_id)" || { fail "velasia-web not running"; exit 1; }
health=$(docker inspect -f '{{.State.Health.Status}}' "$web_id")
[ "$health" = healthy ] && pass "velasia-web health: $health (NextRS /readyz)" || fail "velasia-web health: $health"

section "2. Installed npm package"
pkg_version=$(web sed -n 's/^  "version": "\(.*\)",$/\1/p' /usr/share/doc/nextrs/npm-package.json)
pkg_name=$(web sed -n 's/^  "name": "\(.*\)",$/\1/p' /usr/share/doc/nextrs/npm-package.json)
echo "package: $pkg_name@$pkg_version"
[ "$pkg_name@$pkg_version" = "@nextrs/cli@$expected_version" ] && pass "npm package is @nextrs/cli@$expected_version" || fail "npm package is $pkg_name@$pkg_version"
resolved=$(web grep -o '"resolved": "[^"]*"' /usr/share/doc/nextrs/integration-package-lock.json)
integrity=$(web grep -o '"integrity": "[^"]*"' /usr/share/doc/nextrs/integration-package-lock.json)
echo "lockfile $resolved"; echo "lockfile $integrity"
case $resolved in *'https://registry.npmjs.org/@nextrs/cli/-/cli-0.1.0-rc.1.tgz'*) pass "installed from registry.npmjs.org tarball";; *) fail "lockfile not resolved from the npm registry";; esac
web cat /usr/share/doc/nextrs/BUILD-INFO.json

section "3. Executable"
ver=$(web /usr/local/bin/nextrs --version)
[ "$ver" = "nextrs $expected_version" ] && pass "nextrs --version: $ver" || fail "nextrs --version: $ver"
sha=$(web sha256sum /usr/local/bin/nextrs | cut -d' ' -f1)
echo "sha256 /usr/local/bin/nextrs = $sha"
[ "$sha" = "$expected_sha" ] && pass "binary sha256 = BUILD-INFO binary_sha256 (the npm package's native/nextrs)" || fail "binary sha256 mismatch"
entry=$(docker inspect -f '{{json .Config.Entrypoint}} {{json .Args}}' "$web_id")
echo "entrypoint+args: $entry"
case $entry in '["/usr/local/bin/nextrs"]'*) pass "container entrypoint is /usr/local/bin/nextrs";; *) fail "unexpected entrypoint";; esac

section "4. Processes inside velasia-web (/proc)"
procs=$(web bash -c 'for p in /proc/[0-9]*; do pid=${p#/proc/}; exe=$(readlink "$p/exe" 2>/dev/null || echo "?"); cmd=$(tr "\0" " " < "$p/cmdline" 2>/dev/null); [ -n "$cmd" ] && printf "%s\t%s\t%s\n" "$pid" "$exe" "$cmd"; done')
printf 'PID\tEXE\tCMDLINE\n%s\n' "$procs"
echo "$procs" | awk -F'\t' '$2=="/usr/local/bin/nextrs" && $3 ~ /^\/usr\/local\/bin\/nextrs start \/app\/apps\/web /' | grep -q . \
  && pass "a process runs /usr/local/bin/nextrs start /app/apps/web ... --js-runtime embedded-v8" || fail "no nextrs start process"
echo "$procs" | grep -q -- '--js-runtime embedded-v8' && pass "embedded V8 runtime enabled" || fail "--js-runtime embedded-v8 missing"
if echo "$procs" | awk -F'\t' '{print $2" "$3}' | grep -Eiq '(^|/)(node|nodejs|npm|npx|pnpm)( |$)|next start|server\.js'; then
  fail "a Node.js / next process is running in velasia-web"
else
  pass "no node / npm / pnpm / next start / server.js process in velasia-web"
fi
web bash -c 'command -v node || command -v npm || command -v pnpm' >/dev/null 2>&1 \
  && fail "a Node.js executable exists in the web image" || pass "no node/npm/pnpm executable in the web image"
echo "host view (docker top):"; docker top "$web_id" -o pid,user,args 2>/dev/null || docker top "$web_id"
listen=$(web bash -c 'cat /proc/net/tcp /proc/net/tcp6 2>/dev/null' | awk '$4=="0A"{print $2}' | sort -u | tr '\n' ' ')
echo "listening sockets (hex addr:port): $listen"
echo "$listen" | grep -qi ':0BB8' && pass "port 3000 (0x0BB8) is listening inside velasia-web" || fail "port 3000 not listening inside velasia-web"

section "5. Host port 3000 -> velasia-web only"
docker port "$web_id" 3000/tcp
mapped=$(docker ps --filter publish=3000 --format '{{.ID}} {{.Names}}')
echo "containers publishing 3000: $mapped"
[ "$(echo "$mapped" | grep -c .)" = 1 ] && echo "$mapped" | grep -q "${web_id:0:12}" && pass "only velasia-web publishes port 3000" || fail "port 3000 mapping unexpected"
api_ports=$(docker inspect -f '{{json .NetworkSettings.Ports}}' "$api_id")
echo "velasia-api ports: $api_ports"
echo "$api_ports" | grep -q HostPort && fail "velasia-api publishes a host port" || pass "velasia-api publishes no host port (internal network only)"
echo "velasia-api process (declared supporting API, Node.js allowed): $(docker inspect -f '{{json .Config.Cmd}}' "$api_id")"

section "6. Requests reach NextRS (operations counters before/after)"
metrics() { ops_get /metrics | tr -d '\r' | sed -n '/^$/,$p'; }
before=$(metrics)
probe="nextrs-probe-$(date +%s)-$RANDOM"
codes=""
for path in "/?$probe" "/destinations?$probe" "/api/airports?$probe" "/flights?$probe" "/_next/static/__missing-$probe.js"; do
  c=$(curl -s -o /dev/null -w '%{http_code}' "$base$path"); codes="$codes $path=$c"
done
echo "probe responses:$codes"
sleep 1
after=$(metrics)
total() { echo "$1" | grep -E '^nextrs_http_requests_total' | awk '{s+=$NF} END {print s+0}'; }
b=$(total "$before"); a=$(total "$after")
echo "nextrs_http_requests_total: before=$b after=$a"
[ "$a" -ge $((b + 5)) ] 2>/dev/null && pass "NextRS request counter rose by $((a - b)) for 5 browser-facing probes" || {
  echo "(metric families present:)"; echo "$after" | grep -E '^# TYPE' | head -40
  fail "NextRS request counter did not rise as expected"
}
echo "$after" | grep -E '^nextrs_(http_requests_total|js_|isolate|render|route)' | head -40

section "7. Response headers from the host"
for path in / /api/airports /flights; do
  echo "--- GET $path"; curl -s -D - -o /dev/null "$base$path" | tr -d '\r' | grep -Eiv '^(content-security-policy|permissions-policy):'
done

section "8. NextRS log lines"
"${compose[@]}" logs --no-color --tail=40 velasia-web

echo
if [ "$fails" -eq 0 ]; then echo "RESULT: PASS (all runtime checks)"; else echo "RESULT: FAIL ($fails check(s) failed)"; fi
exit $(( fails > 0 ))
