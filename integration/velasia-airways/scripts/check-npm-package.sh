#!/bin/sh
# Clean-room check of the published package: `npm ci` this directory's lockfile in a fresh
# node:24 container (glibc), then verify version, source, checksum and linked libraries.
#   integration/velasia-airways/scripts/check-npm-package.sh > evidence/npm-install-checks.txt
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
img=node:24.18.0-bookworm-slim@sha256:6f7b03f7c2c8e2e784dcf9295400527b9b1270fd37b7e9a7285cf83b6951452d
if [ "${1:-}" != --inside ]; then
  tar -C "$here" -c package.json package-lock.json scripts/check-npm-package.sh |
    docker run --rm -i "$img" sh -c 'mkdir /w && cd /w && tar -x && sh scripts/check-npm-package.sh --inside'
  exit
fi
pkg=node_modules/@nextrs/cli
echo "# clean install $(date -u +%FT%TZ) in $img"
echo "# node $(node -v), npm $(npm -v), $(getconf GNU_LIBC_VERSION), $(uname -sm)"
npm ci --no-audit --no-fund --ignore-scripts --registry=https://registry.npmjs.org/ 2>&1 | grep -v notice
node -e '
const p = require("./node_modules/@nextrs/cli/package.json");
const l = require("./package-lock.json").packages["node_modules/@nextrs/cli"];
const b = require("./node_modules/@nextrs/cli/BUILD-INFO.json");
console.log(`package: ${p.name}@${p.version} (os=${p.os} cpu=${p.cpu} libc=${p.libc}, scripts=${JSON.stringify(p.scripts ?? {})})`);
console.log(`resolved: ${l.resolved}\nintegrity: ${l.integrity}`);
console.log(`BUILD-INFO: binary_sha256=${b.binary_sha256} binary_commit=${b.binary_commit} min_glibc=${b.min_glibc} target=${b.target}`);'
echo "files: $(ls $pkg | tr '\n' ' ')"
echo "bin/nextrs: $(wc -c < $pkg/bin/nextrs) bytes, first line: $(head -1 $pkg/bin/nextrs)"
echo "$(node -p 'require("./node_modules/@nextrs/cli/BUILD-INFO.json").binary_sha256')  $pkg/native/nextrs" | sha256sum -c -
echo "sha256 bin/nextrs (the file the package README says to checksum): $(sha256sum $pkg/bin/nextrs | cut -c1-64)"
echo "nextrs --version (via node_modules/.bin): $(./node_modules/.bin/nextrs --version)"
echo "ldd native/nextrs:"; ldd $pkg/native/nextrs
