#!/usr/bin/env bash
# Prepares the input of the npm installation test (scripts/npm-install-containers.sh):
# one directory holding the npm tarball under test, an official Node.js
# distribution, a pnpm release, the source of a Next.js application and the
# test scripts. The containers see nothing else: no checkout, no Rust, no
# Cargo registry, no build of the application.
#
#   scripts/npm-install-bundle.sh --tarball dist/npm/nextrs-cli-0.1.0-rc.1.tgz \
#     --manifest dist/nextrs-v0.1.0-rc.1-linux-x86_64.manifest.json --out DIR \
#     [--node-version 22.20.0] [--node-dist node-vX-linux-x64.tar.xz --node-shasums SHASUMS256.txt] \
#     [--pnpm-version 10.23.0]
#
# Node.js is downloaded from nodejs.org (or taken from --node-dist) and
# checked against its SHASUMS256.txt. pnpm is fetched with `npm pack`, which
# checks the registry's integrity hash. The application is
# fixtures/next-app's source (package.json, package-lock.json, app/, config):
# the containers install its dependencies and run `next build` themselves.
#
# The bundle:
#   DIR/pkg/       the tarball, its .sha256, .npm.json, and the release manifest
#   DIR/node/      Node.js (bin/node, bin/npm, bin/npx)
#   DIR/pnpm/      the pnpm package (bin/pnpm.cjs), and DIR/bin/pnpm, a shim for it
#   DIR/app/       the application source
#   DIR/scripts/   npm-install-entry.sh, npm-install-suite.sh, probe.mjs
#   DIR/bundle.json  versions and checksums of all of the above
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARBALL="" MANIFEST="" OUT="" NODE_VERSION=22.20.0 NODE_DIST="" NODE_SHASUMS="" PNPM_VERSION=10.23.0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --tarball) TARBALL="$2"; shift 2 ;;
    --manifest) MANIFEST="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --node-version) NODE_VERSION="$2"; shift 2 ;;
    --node-dist) NODE_DIST="$2"; shift 2 ;;
    --node-shasums) NODE_SHASUMS="$2"; shift 2 ;;
    --pnpm-version) PNPM_VERSION="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -f $TARBALL && -f $TARBALL.sha256 && -f $MANIFEST && -n $OUT ]] ||
  { echo "usage: $0 --tarball FILE.tgz (with .sha256) --manifest RELEASE.manifest.json --out DIR [--node-version V] [--pnpm-version V]" >&2; exit 2; }
[[ ! -e $OUT ]] || { echo "$OUT exists; give a new directory" >&2; exit 1; }
mkdir -p "$OUT"/{pkg,scripts,bin,app}
OUT=$(cd "$OUT" && pwd)

# The package under test.
(cd "$(dirname "$TARBALL")" && sha256sum -c --status "$(basename "$TARBALL").sha256") || { echo "$TARBALL does not match its .sha256" >&2; exit 1; }
cp "$TARBALL" "$TARBALL.sha256" "$OUT/pkg/"
npm_json="${TARBALL%.tgz}.npm.json"
[[ -f $npm_json ]] && cp "$npm_json" "$OUT/pkg/"
cp "$MANIFEST" "$OUT/pkg/release.manifest.json"

# Node.js, verified.
dist="node-v$NODE_VERSION-linux-x64.tar.xz"
dl=$(mktemp -d)
trap 'rm -rf "$dl"' EXIT
if [[ -n $NODE_DIST ]]; then
  cp "$NODE_DIST" "$dl/$dist"
  cp "${NODE_SHASUMS:-$(dirname "$NODE_DIST")/SHASUMS256.txt}" "$dl/SHASUMS256.txt"
else
  curl -fsSL -o "$dl/$dist" "https://nodejs.org/dist/v$NODE_VERSION/$dist"
  curl -fsSL -o "$dl/SHASUMS256.txt" "https://nodejs.org/dist/v$NODE_VERSION/SHASUMS256.txt"
fi
(cd "$dl" && grep " $dist\$" SHASUMS256.txt | sha256sum -c --status) || { echo "$dist does not match SHASUMS256.txt" >&2; exit 1; }
node_sha=$(sha256sum "$dl/$dist" | cut -d' ' -f1)
mkdir "$OUT/node"
tar -xJf "$dl/$dist" -C "$OUT/node" --strip-components=1

# pnpm, from the registry (npm pack verifies its integrity).
(cd "$dl" && "$OUT/node/bin/npm" pack "pnpm@$PNPM_VERSION" --json --loglevel=error > pnpm.json)
pnpm_integrity=$("$OUT/node/bin/node" -p 'JSON.parse(require("fs").readFileSync(process.argv[1]))[0].integrity' "$dl/pnpm.json")
mkdir "$OUT/pnpm"
tar -xzf "$dl/pnpm-$PNPM_VERSION.tgz" -C "$OUT/pnpm" --strip-components=1
cat > "$OUT/bin/pnpm" <<'EOF'
#!/bin/sh
exec node "$(dirname "$(readlink -f "$0")")/../pnpm/bin/pnpm.cjs" "$@"
EOF
chmod 0755 "$OUT/bin/pnpm"

# The application source (no node_modules, no .next).
src="$ROOT/fixtures/next-app"
cp -a "$src/app" "$src/package.json" "$src/package-lock.json" "$src/next.config.ts" "$src/tsconfig.json" "$OUT/app/"

cp "$ROOT/scripts/npm-install-entry.sh" "$ROOT/scripts/npm-install-suite.sh" "$ROOT/tests/npm/probe.mjs" "$OUT/scripts/"
chmod -R a+rX "$OUT"

cat > "$OUT/bundle.json" <<EOF
{
  "tarball": "$(basename "$TARBALL")",
  "tarball_sha256": "$(cut -d' ' -f1 "$TARBALL.sha256")",
  "binary_sha256": "$("$OUT/node/bin/node" -p 'JSON.parse(require("fs").readFileSync(process.argv[1])).binary_sha256' "$MANIFEST")",
  "node": "$NODE_VERSION",
  "node_dist_sha256": "$node_sha",
  "pnpm": "$PNPM_VERSION",
  "pnpm_integrity": "$pnpm_integrity",
  "app": "fixtures/next-app (Next.js $("$OUT/node/bin/node" -p 'require(process.argv[1]).dependencies.next' "$src/package.json"))"
}
EOF
cat "$OUT/bundle.json"
du -sh "$OUT"
