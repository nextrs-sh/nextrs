#!/usr/bin/env bash
# Prepares the input of a clean-install test: one directory holding a
# release archive, prebuilt applications and the curl-only test scripts.
# Nothing else is given to the clean machine, so it never sees this
# checkout, the Rust build directory or a Cargo registry.
#
#   scripts/clean-install-bundle.sh --archive dist/nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz \
#     --out DIR [--skip-build]
#
# Runs on the builder, which needs Node.js and npm (to run `next build`).
# The clean machine needs neither: it serves the `.next` output.
#
# The bundle:
#   DIR/nextrs-v*.tar.gz, .sha256   the archive under test (and the manifest, if present)
#   DIR/apps/<fixture>/             .next, public, package.json, next.config.ts and
#                                   node_modules (Next.js's runtime JavaScript, which
#                                   NextRS reads from the application, as next start does)
#   DIR/probes.env                  values the probes need from the builds (Server
#                                   Action ids, a static chunk), computed here so the
#                                   clean machine needs no JSON tooling
#   DIR/scripts/                    release-smoke.sh, clean-install-suite.sh,
#                                   clean-install-entry.sh, clean-install-containers.sh
#
# Packages only `next build` needs are left out of node_modules: the SWC
# compiler (@next/swc-*), TypeScript and type definitions, and sharp
# (@img/*, sharp). Leaving sharp out also shows that image optimization
# does not use it.
#
# The fixtures are official `next build` outputs of the pinned Next.js
# version (each fixture's package-lock.json). --skip-build reuses the
# `.next` directories already present.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ARCHIVE="" OUT="" BUILD=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) ARCHIVE="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --skip-build) BUILD=0; shift ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -f $ARCHIVE && -f $ARCHIVE.sha256 && -n $OUT ]] || { echo "usage: $0 --archive FILE.tar.gz (with FILE.tar.gz.sha256) --out DIR [--skip-build]" >&2; exit 2; }
[[ ! -e $OUT ]] || { echo "$OUT exists; give a new directory" >&2; exit 1; }
FIXTURES=(next-app next-cache-components next-pages next-image next-authjs)
export NEXT_TELEMETRY_DISABLED=1

mkdir -p "$OUT/apps" "$OUT/scripts"
OUT=$(cd "$OUT" && pwd)
cp "$ARCHIVE" "$ARCHIVE.sha256" "$OUT/"
manifest="${ARCHIVE%.tar.gz}.manifest.json"
[[ -f $manifest ]] && cp "$manifest" "$OUT/"
cp "$ROOT/scripts/release-smoke.sh" "$ROOT/scripts/clean-install-suite.sh" "$ROOT/scripts/clean-install-entry.sh" \
  "$ROOT/scripts/clean-install-containers.sh" "$OUT/scripts/"

upstream_pid=""
trap '[[ -n $upstream_pid ]] && kill "$upstream_pid" 2>/dev/null' EXIT
if [[ $BUILD == 1 ]]; then
  # next-pages prerenders an ISR page from the test upstream.
  node "$ROOT/tests/compat/upstream.mjs" --port 4999 > /dev/null 2>&1 &
  upstream_pid=$!
  sleep 1
fi
for f in "${FIXTURES[@]}"; do
  src="$ROOT/fixtures/$f"
  if [[ $BUILD == 1 ]]; then
    (cd "$src" && npm ci --no-audit --no-fund --loglevel=error && rm -rf .next && npx next build > "$OUT/build-$f.log" 2>&1) ||
      { tail -30 "$OUT/build-$f.log"; exit 1; }
  fi
  [[ -f $src/.next/BUILD_ID ]] || { echo "$f has no .next build" >&2; exit 1; }
  mkdir -p "$OUT/apps/$f"
  for item in .next public package.json next.config.ts node_modules; do
    [[ -e $src/$item ]] && cp -a "$src/$item" "$OUT/apps/$f/"
  done
  rm -rf "$OUT/apps/$f/.next/cache" "$OUT/apps/$f/node_modules/.cache" "$OUT/apps/$f/node_modules/.bin" \
    "$OUT/apps/$f/node_modules/@next/"swc-* "$OUT/apps/$f/node_modules/typescript" "$OUT/apps/$f/node_modules/@types" \
    "$OUT/apps/$f/node_modules/@img" "$OUT/apps/$f/node_modules/sharp"
  echo "$f: Next.js $(node -p "require('$src/node_modules/next/package.json').version"), build $(cat "$src/.next/BUILD_ID")"
done

# Values the curl probes need. Server Action ids are per build.
node - "$OUT/apps" > "$OUT/probes.env" <<'EOF'
const fs = require("fs"), path = require("path");
const apps = process.argv[2];
const action = (app, name, file) => {
  const m = JSON.parse(fs.readFileSync(path.join(apps, app, ".next/server/server-reference-manifest.json"), "utf8"));
  const hit = Object.entries(m.node).find(([, v]) => v.exportedName === name && v.filename === file);
  if (!hit) throw new Error(`${app}: no Server Action ${name} in ${file}`);
  const page = Object.keys(hit[1].workers)[0].replace(/^app/, "").replace(/\/page$/, "") || "/";
  return [hit[0], page];
};
const [whoId, whoPage] = action("next-app", "whoAmI", "app/actions/server.ts");
const chunks = path.join(apps, "next-app/.next/static/chunks");
const chunk = fs.readdirSync(chunks).find((f) => f.endsWith(".js"));
const out = {
  APP_ACTION_ID: whoId,
  APP_ACTION_PAGE: whoPage,
  APP_STATIC_CHUNK: `/_next/static/chunks/${chunk}`,
  APP_STATIC_CHUNK_BYTES: fs.statSync(path.join(chunks, chunk)).size,
};
for (const [k, v] of Object.entries(out)) console.log(`${k}=${v}`);
EOF
cat "$OUT/probes.env"
du -sh "$OUT"
