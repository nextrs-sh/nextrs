#!/usr/bin/env bash
# Builds and tests the documented multi-stage Dockerfile (examples/docker)
# with the @nextrs/cli tarball under test and fixtures/next-app.
#
#   scripts/npm-docker-example.sh --tarball dist/npm/nextrs-cli-0.1.0-rc.1.tgz \
#     --expect-sha256 SHA --out DIR [--engine docker|podman] [--port 6400] [--keep-image]
#
# The build context is fixtures/next-app's source with:
#   - next.config.ts set to output: "standalone" (as the Dockerfile requires);
#   - @nextrs/cli added to devDependencies from the tarball (the lockfile
#     records it as file:nextrs-cli-<version>.tgz);
#   - one line added to a copy of the Dockerfile, so that `npm ci` can read
#     that tarball before the package is on the registry:
#       COPY nextrs-cli-<version>.tgz ./
#     This is the only difference from examples/docker/Dockerfile; it is
#     recorded in DIR/Dockerfile.diff.
#
# Checks (JSON in DIR/docker-example.json): the image builds; the runtime
# image has no node, npm, npx, corepack, cargo or rustc, and no
# node_modules/.bin/nextrs or @nextrs/cli; /usr/local/bin/nextrs is the
# release binary; PID 1 is nextrs; the server answers readiness, SSR, RSC, a
# Server Action and a static file; `stop` (SIGTERM) gives exit 0 and a
# graceful stop. Needs node on the host (for npm and the probes).
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARBALL="" EXPECT="" OUT="" ENGINE="${CONTAINER_ENGINE:-docker}" PORT=6400 KEEP=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --tarball) TARBALL="$2"; shift 2 ;;
    --expect-sha256) EXPECT="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --engine) ENGINE="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --keep-image) KEEP=1; shift ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -f $TARBALL && -n $EXPECT && -n $OUT ]] || { echo "usage: $0 --tarball FILE.tgz --expect-sha256 SHA --out DIR [--engine E] [--port P]" >&2; exit 2; }
[[ ! -e $OUT ]] || { echo "$OUT exists; give a new directory" >&2; exit 1; }
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
TARBALL=$(cd "$(dirname "$TARBALL")" && pwd)/$(basename "$TARBALL")
IMAGE=localhost/nextrs-docker-example:test
CTR=nextrs-docker-example-$$
METRICS=$((PORT + 1))
declare -a RESULTS=()
FAILED=0
check() {
  local s=pass; [[ $2 == 0 ]] || { s=fail; FAILED=1; }
  RESULTS+=("$(node -e 'console.log(JSON.stringify({check:process.argv[1],result:process.argv[2],detail:process.argv[3].slice(0,600)}))' "$1" "$s" "$3")")
  printf '%-22s %s  %s\n' "$1" "$s" "$(printf '%s' "$3" | head -c 180 | tr '\n' ' ')"
}
cleanup() {
  $ENGINE rm -f "$CTR" > /dev/null 2>&1
  [[ $KEEP == 1 ]] || $ENGINE rmi -f "$IMAGE" > /dev/null 2>&1
}
trap cleanup EXIT

# 1. Build context.
ctx=$OUT/context
mkdir -p "$ctx"
src=$ROOT/fixtures/next-app
cp -a "$src/app" "$src/package.json" "$src/package-lock.json" "$src/tsconfig.json" "$ctx/"
cat > "$ctx/next.config.ts" <<'EOF'
import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
};

export default nextConfig;
EOF
cp "$TARBALL" "$ctx/"
tgz=$(basename "$TARBALL")
(cd "$ctx" && npm install --save-dev --package-lock-only --no-audit --no-fund --loglevel=error "./$tgz") > "$OUT/lock.log" 2>&1
check lockfile $? "package.json devDependencies: $(node -p "JSON.stringify(require('$ctx/package.json').devDependencies['@nextrs/cli'])")"
cp "$ROOT/examples/docker/.dockerignore" "$ctx/.dockerignore"
sed "s#^RUN npm ci --no-audit --no-fund\$#COPY $tgz ./\n&#" "$ROOT/examples/docker/Dockerfile" > "$ctx/Dockerfile"
diff "$ROOT/examples/docker/Dockerfile" "$ctx/Dockerfile" > "$OUT/Dockerfile.diff"
check dockerfile-diff "$([[ $(grep -c '^[<>]' "$OUT/Dockerfile.diff") == 1 ]] && grep -q "^> COPY $tgz ./" "$OUT/Dockerfile.diff"; echo $?)" "$(grep '^[<>]' "$OUT/Dockerfile.diff" | tr '\n' ' ')"

# 2. Build.
t0=$(date +%s)
$ENGINE build --pull=missing -t "$IMAGE" -f "$ctx/Dockerfile" "$ctx" > "$OUT/build.log" 2>&1
rc=$?
check image-build "$rc" "exit $rc in $(( $(date +%s) - t0 )) s; $(grep -E 'nextrs [0-9]|: OK$' "$OUT/build.log" | tr '\n' ' ' | head -c 200)"
[[ $rc == 0 ]] || { tail -40 "$OUT/build.log"; exit 1; }
size=$($ENGINE image inspect "$IMAGE" --format '{{.Size}}' 2>/dev/null)
check image-size 0 "$(( size / 1048576 )) MiB (runtime stage)"

# 3. What the runtime image contains.
inv=$($ENGINE run --rm --entrypoint /bin/sh "$IMAGE" -c '
  for t in node nodejs npm npx corepack pnpm cargo rustc; do command -v "$t" && echo "found $t"; done
  find / -xdev \( -path /proc -o -path /sys \) -prune -o \( -name "@nextrs" -o -path "*/node_modules/.bin/nextrs" -o -name ".cargo" -o -name "Cargo.toml" \) -print 2>/dev/null
  echo "glibc: $(getconf GNU_LIBC_VERSION)"; echo "user: $(id -un)"; sha256sum /usr/local/bin/nextrs; ls /app; ls /usr/share/doc/nextrs' 2>&1)
echo "$inv" > "$OUT/inventory.txt"
bad=$(echo "$inv" | grep -E '^found |@nextrs|node_modules/.bin/nextrs|\.cargo|Cargo.toml' | tr '\n' ' ')
check no-node-no-rust "$([[ -z $bad ]]; echo $?)" "${bad:-no node, npm, npx, corepack, pnpm, cargo or rustc; no npm package or Rust files}; $(echo "$inv" | grep -E '^(glibc|user):' | tr '\n' ' ')"
check binary-sha256 "$(echo "$inv" | grep -q "^$EXPECT  /usr/local/bin/nextrs"; echo $?)" "$(echo "$inv" | grep /usr/local/bin/nextrs)"
check license-files "$(echo "$inv" | grep -q THIRD-PARTY-LICENSES.txt && echo "$inv" | grep -q LICENSE-APACHE; echo $?)" "/usr/share/doc/nextrs: $(echo "$inv" | grep -E 'LICENSE|THIRD|BUILD-INFO' | tr '\n' ' ')"

# 4. Run.
$ENGINE run -d --name "$CTR" -p "127.0.0.1:$PORT:3000" -p "127.0.0.1:$METRICS:9100" "$IMAGE" > /dev/null
ok=1
for _ in $(seq 1 120); do
  node -e 'fetch(process.argv[1],{signal:AbortSignal.timeout(2000)}).then(r=>process.exit(r.status===200?0:1),()=>process.exit(1))' "http://127.0.0.1:$METRICS/readyz" && { ok=0; break; }
  sleep 0.5
done
check ready $ok "readyz on the operations port"
pid1=$($ENGINE exec "$CTR" /bin/sh -c 'readlink /proc/1/exe; for p in /proc/[0-9]*; do readlink $p/exe; done | sort | uniq -c | tr "\n" " "' 2>&1)
check pid1-native "$([[ ${pid1%%$'\n'*} == /usr/local/bin/nextrs ]]; echo $?)" "PID 1: $(echo "$pid1" | head -1); processes: $(echo "$pid1" | tail -1)"
mkdir -p "$OUT/app"
$ENGINE cp "$CTR:/app/.next" "$OUT/app/.next" > /dev/null 2>&1
node "$ROOT/tests/npm/probe.mjs" --url "http://127.0.0.1:$PORT" --project "$OUT/app" > "$OUT/probe.jsonl" 2>&1
while read -r line; do
  n=$(node -e 'const j=JSON.parse(process.argv[1]);console.log(j.check)' "$line" 2>/dev/null) || { check probe 1 "$line"; continue; }
  r=$(node -e 'const j=JSON.parse(process.argv[1]);console.log(j.result+"\t"+j.detail)' "$line")
  check "http:$n" "$([[ ${r%%$'\t'*} == pass ]]; echo $?)" "${r#*$'\t'}"
done < "$OUT/probe.jsonl"

# 5. Stop (SIGTERM to PID 1).
$ENGINE stop -t 20 "$CTR" > /dev/null
code=$($ENGINE inspect "$CTR" --format '{{.State.ExitCode}}')
$ENGINE logs "$CTR" > "$OUT/server.log" 2>&1
check stop "$([[ $code == 0 ]] && grep -q 'nextrs stopped' "$OUT/server.log"; echo $?)" "docker stop -> exit $code; $(grep -o '"message":"nextrs stopped"' "$OUT/server.log" | head -1)"

{ echo "{\"image_bytes\":${size:-0},\"passed\":$([[ $FAILED == 0 ]] && echo true || echo false),\"checks\":["; IFS=,; echo "${RESULTS[*]}"; echo "]}"; } > "$OUT/docker-example.json"
exit $FAILED
