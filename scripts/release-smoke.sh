#!/usr/bin/env bash
# Clean-install smoke test of a NextRS release archive.
#
#   scripts/release-smoke.sh --archive nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz \
#     --app fixtures/next-app [--port 3990] [--tls-origin https://example.com] [--no-network] [--json out.json]
#
# Needs only bash, coreutils, tar, gzip, sha256sum and curl. The app is a
# project directory that already holds an official `next build` output
# (.next); building it needs Node.js, running it must not.
#
# Checks, each recorded as pass/fail:
#   checksum       the archive matches its .sha256 file
#   layout         the archive holds nextrs, BUILD-INFO.json, README.md, RELEASE-NOTES.md, LICENSE-MIT,
#                  LICENSE-APACHE, THIRD-PARTY.md, THIRD-PARTY-LICENSES.txt
#   version        `nextrs --version` is "nextrs <BUILD-INFO version>"
#   help           `nextrs --help`, `nextrs start --help` exit 0
#   no-toolchain   the server runs with an empty environment and PATH=/usr/bin:/bin
#                  (reports whether cargo, rustc, node or deno are reachable there)
#   inspect        `nextrs inspect` reads the build
#   read-only-app  the app is served from a read-only copy
#   ready          /readyz and /healthz answer 200 on the metrics address
#   prerendered    GET / is 200
#   dynamic        GET /books/42 is 200 and rendered (V8)
#   tls            fetch to an allowed https origin succeeds with the bundled roots
#   net-denied     fetch to an origin not on --net-allow is refused
#   cache-dir      --cache-dir is created with mode 0700
#   metrics        /metrics is Prometheus text with nextrs_ series
#   shutdown       on SIGTERM /readyz stops answering 200, the process exits 0
set -uo pipefail
ARCHIVE="" APP="" PORT=3990 TLS_ORIGIN="https://example.com" NETWORK=1 JSON=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) ARCHIVE="$2"; shift 2 ;;
    --app) APP="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --tls-origin) TLS_ORIGIN="$2"; shift 2 ;;
    --no-network) NETWORK=0; shift ;;
    --json) JSON="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -f $ARCHIVE && -d $APP/.next ]] || { echo "usage: $0 --archive FILE.tar.gz --app DIR(with .next)" >&2; exit 2; }
ARCHIVE=$(cd "$(dirname "$ARCHIVE")" && pwd)/$(basename "$ARCHIVE")
METRICS=$((PORT + 1))
WORK=$(mktemp -d "${TMPDIR:-/tmp}/nextrs-smoke.XXXXXX")
PID=""
cleanup() { [[ -n $PID ]] && kill -9 "$PID" 2>/dev/null; chmod -R u+w "$WORK" 2>/dev/null; rm -rf "$WORK"; }
trap cleanup EXIT
declare -a RESULTS=()
FAILED=0
check() { # name, status (0 = pass), detail
  local s=pass; [[ $2 == 0 ]] || { s=fail; FAILED=1; }
  RESULTS+=("{\"check\":\"$1\",\"result\":\"$s\",\"detail\":$(printf '%s' "$3" | head -c 600 | sed 's/\\/\\\\/g; s/"/\\"/g; s/\t/ /g' | awk 'BEGIN{printf "\""} {printf "%s\\n", $0} END{printf "\""}')}")
  printf '%-14s %s  %s\n' "$1" "$s" "$(printf '%s' "$3" | head -c 160 | tr '\n' ' ')"
}
http() { curl -s -o "$WORK/body" -w '%{http_code}' --max-time 20 "$@" || true; }

# checksum, layout, version, help
if [[ -f $ARCHIVE.sha256 ]]; then
  out=$(cd "$(dirname "$ARCHIVE")" && sha256sum -c "$(basename "$ARCHIVE").sha256" 2>&1); check checksum $? "$out"
else
  check checksum 1 "no $ARCHIVE.sha256"
fi
tar -xzf "$ARCHIVE" -C "$WORK"
DIR=$(find "$WORK" -mindepth 1 -maxdepth 1 -type d -name 'nextrs-v*' | head -1)
missing=""
for f in nextrs BUILD-INFO.json README.md RELEASE-NOTES.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY.md THIRD-PARTY-LICENSES.txt; do [[ -e $DIR/$f ]] || missing+=" $f"; done
check layout "$([[ -z $missing && -x $DIR/nextrs ]]; echo $?)" "${missing:-all present in $(basename "$DIR")}"
NEXTRS=$DIR/nextrs
want=$(sed -n 's/.*"version": "\(.*\)".*/\1/p' "$DIR/BUILD-INFO.json")
got=$(env -i PATH=/usr/bin:/bin "$NEXTRS" --version 2>&1)
check version "$([[ $got == "nextrs $want" ]]; echo $?)" "$got (BUILD-INFO $want)"
env -i PATH=/usr/bin:/bin "$NEXTRS" --help > /dev/null 2>&1 && env -i PATH=/usr/bin:/bin "$NEXTRS" start --help > /dev/null 2>&1
check help $? "nextrs --help, nextrs start --help"
found=""
for t in cargo rustc node deno; do env -i PATH=/usr/bin:/bin bash -c "command -v $t" > /dev/null 2>&1 && found+=" $t"; done
check no-toolchain 0 "server environment: env -i, PATH=/usr/bin:/bin; reachable there:${found:- none of cargo, rustc, node, deno}"

# read-only copy of the app
cp -a "$APP" "$WORK/app" 2>/dev/null || { mkdir -p "$WORK/app" && cp -a "$APP/.next" "$APP/public" "$APP/package.json" "$WORK/app/" 2>/dev/null; }
rm -rf "$WORK/app/node_modules/.cache" 2>/dev/null
chmod -R a-w "$WORK/app"
out=$(env -i PATH=/usr/bin:/bin "$NEXTRS" inspect "$WORK/app" 2>&1); check inspect $? "$(echo "$out" | head -4)"

net=(--net-allow "$TLS_ORIGIN")
env -i PATH=/usr/bin:/bin HOME="$WORK/home" NO_COLOR=1 "$NEXTRS" start "$WORK/app" --port "$PORT" --js-runtime embedded-v8 \
  --metrics-addr "127.0.0.1:$METRICS" --cache-dir "$WORK/cache" --image-cache-dir "$WORK/images" \
  --strip-forwarded-headers "${net[@]}" > "$WORK/server.log" 2>&1 &
PID=$!
ready=1
for _ in $(seq 1 120); do [[ $(http "http://127.0.0.1:$METRICS/readyz") == 200 ]] && { ready=0; break; }; sleep 0.5; done
check ready $ready "readyz=$(http "http://127.0.0.1:$METRICS/readyz") healthz=$(http "http://127.0.0.1:$METRICS/healthz")"
check read-only-app "$(kill -0 $PID 2>/dev/null; echo $?)" "served from a copy with no write permission"
s=$(http "http://127.0.0.1:$PORT/"); check prerendered "$([[ $s == 200 ]]; echo $?)" "GET / -> $s"
s=$(http "http://127.0.0.1:$PORT/books/42"); check dynamic "$([[ $s == 200 ]] && grep -q '42' "$WORK/body"; echo $?)" "GET /books/42 -> $s"
if [[ $NETWORK == 1 ]]; then
  s=$(http "http://127.0.0.1:$PORT/api/data?target=$TLS_ORIGIN/")
  check tls "$([[ $s == 200 ]] && grep -q '"status":200' "$WORK/body"; echo $?)" "fetch $TLS_ORIGIN -> $s $(head -c 200 "$WORK/body")"
fi
s=$(http "http://127.0.0.1:$PORT/api/data?target=https://not-allowed.invalid/")
check net-denied "$([[ $s == 502 ]] && ! grep -q '"status":200' "$WORK/body"; echo $?)" "fetch https://not-allowed.invalid -> $s $(head -c 200 "$WORK/body")"
mode=$(stat -c %a "$WORK/cache" 2>/dev/null || echo missing)
check cache-dir "$([[ $mode == 700 ]]; echo $?)" "cache dir mode $mode"
s=$(http "http://127.0.0.1:$METRICS/metrics")
check metrics "$([[ $s == 200 ]] && grep -q '^nextrs_' "$WORK/body"; echo $?)" "GET /metrics -> $s, $(grep -c '^nextrs_' "$WORK/body") nextrs_ samples"

# graceful shutdown
kill -TERM "$PID"
t0=$(date +%s%N); notready=1
for _ in $(seq 1 100); do r=$(http "http://127.0.0.1:$METRICS/readyz"); [[ $r != 200 ]] && { notready=0; break; }; sleep 0.02; done
wait "$PID"; code=$?; PID=""
ms=$(( ($(date +%s%N) - t0) / 1000000 ))
check shutdown "$([[ $notready == 0 && $code == 0 ]] && grep -q 'nextrs stopped' "$WORK/server.log"; echo $?)" "readyz -> ${r:-?} after SIGTERM; exit $code in ${ms} ms"

if [[ -n $JSON ]]; then
  { echo "{\"archive\":\"$(basename "$ARCHIVE")\",\"host\":\"$(. /etc/os-release 2>/dev/null; echo "${PRETTY_NAME:-unknown}")\",\"glibc\":\"$(ldd --version 2>/dev/null | head -1 | awk '{print $NF}')\",\"date\":\"$(date -u +%FT%TZ)\",\"passed\":$([[ $FAILED == 0 ]] && echo true || echo false),\"checks\":["; IFS=,; echo "${RESULTS[*]}"; echo "]}"; } > "$JSON"
fi
[[ $FAILED == 0 ]] || { echo "--- server log"; tail -20 "$WORK/server.log"; }
exit $FAILED
