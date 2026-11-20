#!/usr/bin/env bash
# Records every filesystem path a NextRS release binary touches while it
# starts, serves requests and stops, and fails if any of them belongs to
# the build machine: a Cargo registry or home, a rustup toolchain, a Cargo
# target directory or this checkout (outside the application served).
#
#   scripts/runtime-file-audit.sh --archive FILE.tar.gz --app DIR [--isolates N] [--out DIR]
#
# Unlike a clean-machine test it runs where those files exist, so it finds a
# dependency on them even though every read would succeed here. This is the
# check that would have stopped the withdrawn 621da204 build (it opened
# ~/.cargo/registry/src/*/deno_web-0.291.0/*.js at every isolate start).
#
# Needs strace and curl. Writes to --out: strace.log (raw), paths.txt (each
# distinct path with its syscall and result), violations.txt, summary.json.
set -uo pipefail
ARCHIVE="" APP="" ISOLATES=2 OUT="" PORT=3980
while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) ARCHIVE="$2"; shift 2 ;;
    --app) APP="$2"; shift 2 ;;
    --isolates) ISOLATES="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -f $ARCHIVE && -d $APP/.next ]] || { echo "usage: $0 --archive FILE.tar.gz --app DIR [--isolates N] [--out DIR]" >&2; exit 2; }
command -v strace > /dev/null || { echo "strace is required" >&2; exit 2; }
ROOT=$(cd "$(dirname "$0")/.." && pwd)
APP=$(cd "$APP" && pwd)
OUT=${OUT:-$(mktemp -d)}
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/nextrs-audit.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
tar -xzf "$ARCHIVE" -C "$WORK"
BIN=$(echo "$WORK"/nextrs-v*/nextrs)
METRICS=$((PORT + 1))

strace -f -qq -e trace=%file -e signal=none -s 4096 -o "$OUT/strace.log" \
  env -i PATH=/usr/bin:/bin HOME="$WORK/home" "$BIN" start "$APP" --port "$PORT" --js-runtime embedded-v8 \
  --isolates "$ISOLATES" --metrics-addr "127.0.0.1:$METRICS" --cache-dir "$WORK/cache" --image-cache-dir "$WORK/images" \
  > "$OUT/server.log" 2>&1 &
PID=$!
ready=1
for _ in $(seq 1 240); do [[ $(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$METRICS/readyz") == 200 ]] && { ready=0; break; }; kill -0 $PID 2>/dev/null || break; sleep 0.25; done
codes=""
if [[ $ready == 0 ]]; then
  # Exercise rendering, RSC, streaming and route handlers on every isolate.
  for i in $(seq 1 $((ISOLATES * 4))); do
    for path in / /books/$i /dynamic /api/health "/api/stream?size=65536"; do
      codes+="$path=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 "http://127.0.0.1:$PORT$path") "
    done
    codes+="rsc:/books/$i=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 -L -H 'RSC: 1' "http://127.0.0.1:$PORT/books/$i") "
  done
fi
# strace's child is env, then nextrs: signal the server itself.
srv=$(pgrep -P $PID -f "$BIN" || echo $PID)
kill -TERM "$srv" 2>/dev/null
wait $PID
exit_code=$?

# Every absolute path argument of a file syscall, with the syscall (lines
# that strace splits across threads, "<unfinished ...>", included).
sed -nE 's/^[0-9]+ +([a-z_0-9]+)\([^"]*"(\/[^"]*)".*/\2\t\1/p' "$OUT/strace.log" | sort -u > "$OUT/paths.txt"

cargo_home=${CARGO_HOME:-$HOME/.cargo}
rustup_home=${RUSTUP_HOME:-$HOME/.rustup}
# A path is a violation when it is under the Cargo home or rustup, names a
# registry or crate source directory, or lies in this checkout outside the
# application (for example target/ or crates/). The application's parent
# directories are allowed: resolving its real path visits them.
awk -F'\t' -v ch="$cargo_home/" -v rh="$rustup_home/" -v root="$ROOT/" -v app="$APP" '
  { p = $1 }
  index(p, ch) == 1 || index(p, rh) == 1 || p ~ /\/registry\/src\// || p ~ /\/(deno_core|deno_web|deno_webidl|deno_ops|v8)-[0-9][^\/]*\// { print; next }
  index(p, root) == 1 && index(p, app) != 1 && index(app, p) != 1 { print; next }
' "$OUT/paths.txt" > "$OUT/violations.txt"

nv=$(grep -c . "$OUT/violations.txt")
total=$(grep -c . "$OUT/paths.txt")
bad_codes=$(echo "$codes" | tr ' ' '\n' | grep -v '=200$' | grep -c .)
pass=$([[ $ready == 0 && $nv == 0 && $bad_codes == 0 && $exit_code == 0 ]] && echo true || echo false)
cat > "$OUT/summary.json" <<EOF
{"archive":"$(basename "$ARCHIVE")","binary_sha256":"$(sha256sum "$BIN" | cut -d' ' -f1)","isolates":$ISOLATES,"ready":$([[ $ready == 0 ]] && echo true || echo false),"requests":$(echo "$codes" | wc -w),"non_200":$bad_codes,"exit_code":$exit_code,"distinct_paths":$total,"violations":$nv,"passed":$pass}
EOF
echo "ready=$([[ $ready == 0 ]] && echo yes || echo no) requests=$(echo "$codes" | wc -w) non-200=$bad_codes exit=$exit_code paths=$total violations=$nv"
[[ $nv == 0 ]] || { echo "build-machine paths touched at run time:"; head -30 "$OUT/violations.txt"; }
[[ $ready == 0 ]] || { echo "server did not become ready:"; tail -5 "$OUT/server.log"; }
[[ $bad_codes == 0 ]] || echo "non-200 responses: $(echo "$codes" | tr ' ' '\n' | grep -v '=200$' | sort | uniq -c | tr '\n' ' ')"
[[ $pass == true ]]
