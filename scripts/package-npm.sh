#!/usr/bin/env bash
# Build the @nextrs-sh/cli npm tarball from a verified NextRS release archive.
#
#   scripts/package-npm.sh --archive dist/nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz \
#     [--expect-binary-sha256 SHA] [--out dist/npm] [--allow-dirty]
#
# (--template FILE and --withdrawn FILE replace packages/cli/package.json and
# release/WITHDRAWN.txt; tests/npm/packaging-negative.sh uses them.)
#
# The npm package is a distribution channel for the same native binary as
# the GitHub release archive. Nothing is compiled here: the binary (as
# native/nextrs), its BUILD-INFO.json, NextRS's licenses and the third-party
# notices are copied byte for byte from the archive. bin/nextrs is the
# POSIX launcher from packages/cli/bin/nextrs, which checks the platform
# and execs native/nextrs. The archive must have its .sha256 and
# .manifest.json next to it (as scripts/package-release.sh writes them).
#
# Refuses to pack when:
#   - the archive does not match its .sha256 file or its manifest;
#   - the binary's SHA-256 differs between the extracted file, the
#     manifest, BUILD-INFO.json and --expect-binary-sha256 (the gate
#     record's checksum, when given);
#   - the binary or the archive is listed in release/WITHDRAWN.txt;
#   - the versions differ: packages/cli/package.json, the manifest,
#     BUILD-INFO.json, the Cargo workspace and `nextrs --version`;
#   - the binary is not an x86-64 ELF for glibc, or its glibc floor is not
#     the one the package documents (2.35);
#   - tracked files have uncommitted changes (--allow-dirty for a dry run;
#     the npm manifest then says dirty: true).
#
# Produces, in --out:
#   nextrs-sh-cli-<version>.tgz          the npm tarball (npm pack, deterministic)
#   nextrs-sh-cli-<version>.tgz.sha256   its checksum (sha256sum format)
#   nextrs-sh-cli-<version>.npm.json     package, integrity, binary and archive identity
# and then runs scripts/verify-npm-package.sh on the tarball (results in <name>.verify.json).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
ARCHIVE="" EXPECT="" OUT=dist/npm ALLOW_DIRTY=0 TEMPLATE=packages/cli/package.json WITHDRAWN=release/WITHDRAWN.txt
MIN_GLIBC=2.35
while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) ARCHIVE="$2"; shift 2 ;;
    --expect-binary-sha256) EXPECT="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --allow-dirty) ALLOW_DIRTY=1; shift ;;
    --template) TEMPLATE="$2"; shift 2 ;;
    --withdrawn) WITHDRAWN="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
die() { echo "package-npm: $*" >&2; exit 1; }
[[ -n $ARCHIVE ]] || { echo "usage: $0 --archive FILE.tar.gz [--expect-binary-sha256 SHA] [--out DIR]" >&2; exit 2; }
[[ -f $ARCHIVE ]] || die "no archive $ARCHIVE"
command -v node > /dev/null && command -v npm > /dev/null || die "node and npm are needed to pack (not to install or run)"
ARCHIVE=$(cd "$(dirname "$ARCHIVE")" && pwd)/$(basename "$ARCHIVE")
manifest="${ARCHIVE%.tar.gz}.manifest.json"
[[ -f $ARCHIVE.sha256 ]] || die "missing $ARCHIVE.sha256"
[[ -f $manifest ]] || die "missing $manifest"
json() { node -e 'const v=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"))[process.argv[2]];if(v===undefined)process.exit(3);console.log(v)' "$1" "$2"; }

# 1. The archive is the one its checksum file and manifest describe.
(cd "$(dirname "$ARCHIVE")" && sha256sum -c --status "$(basename "$ARCHIVE").sha256") || die "$(basename "$ARCHIVE") does not match its .sha256"
archive_sha=$(sha256sum "$ARCHIVE" | cut -d' ' -f1)
[[ $(json "$manifest" archive_sha256) == "$archive_sha" ]] || die "the manifest's archive_sha256 is not the archive's ($archive_sha)"

# 2. Extract and identify the binary.
stage=$(mktemp -d "${TMPDIR:-/tmp}/nextrs-npm.XXXXXX")
trap 'rm -rf "$stage"' EXIT
tar -xzf "$ARCHIVE" -C "$stage/"
src=$(find "$stage" -mindepth 1 -maxdepth 1 -type d -name 'nextrs-v*-linux-x86_64')
[[ -n $src && -f $src/nextrs ]] || die "the archive has no nextrs-v*-linux-x86_64/nextrs"
for f in BUILD-INFO.json LICENSE-MIT LICENSE-APACHE THIRD-PARTY.md THIRD-PARTY-LICENSES.txt; do
  [[ -s $src/$f ]] || die "the archive has no $f (package it with the current scripts/package-release.sh)"
done
bin_sha=$(sha256sum "$src/nextrs" | cut -d' ' -f1)
[[ $(json "$manifest" binary_sha256) == "$bin_sha" ]] || die "the binary is $bin_sha, the manifest says $(json "$manifest" binary_sha256)"
[[ $(json "$src/BUILD-INFO.json" binary_sha256) == "$bin_sha" ]] || die "the binary is $bin_sha, BUILD-INFO.json says $(json "$src/BUILD-INFO.json" binary_sha256)"
[[ -z $EXPECT || $EXPECT == "$bin_sha" ]] || die "the binary is $bin_sha, not the expected (gate record) $EXPECT"
if [[ -f $WITHDRAWN ]]; then
  while read -r sha _; do
    [[ $sha =~ ^[0-9a-f]{64}$ ]] || continue
    [[ $sha != "$bin_sha" && $sha != "$archive_sha" ]] || die "$sha is a withdrawn build ($WITHDRAWN)"
  done < "$WITHDRAWN"
fi

# 3. One version everywhere.
version=$(json "$TEMPLATE" version)
workspace=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml)
for pair in "manifest:$(json "$manifest" version)" "BUILD-INFO.json:$(json "$src/BUILD-INFO.json" version)" "Cargo workspace:$workspace"; do
  [[ ${pair#*:} == "$version" ]] || die "$TEMPLATE is $version, ${pair%%:*} is ${pair#*:}"
done
reported=$("$src/nextrs" --version 2>&1) || die "the binary does not run here: $reported"
[[ $reported == "nextrs $version" ]] || die "the binary reports '$reported', not 'nextrs $version'"

# 4. The platform the package metadata promises.
readelf -h "$src/nextrs" | grep -q 'Machine:.*X86-64' || die "the binary is not x86-64"
readelf -l "$src/nextrs" | grep -q '/lib64/ld-linux-x86-64.so.2' || die "the binary's interpreter is not glibc's ld-linux-x86-64.so.2"
glibc=$(objdump -T "$src/nextrs" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -Vu | tail -1)
[[ $glibc == "$MIN_GLIBC" && $(json "$src/BUILD-INFO.json" min_glibc) == "$MIN_GLIBC" ]] ||
  die "the binary needs glibc $glibc (BUILD-INFO $(json "$src/BUILD-INFO.json" min_glibc)); the package documents $MIN_GLIBC"

# 5. Source state of the packaging inputs (package.json and README).
commit=$(git rev-parse HEAD)
dirty=false
if ! git diff --quiet HEAD -- packages/cli scripts/package-npm.sh; then
  dirty=true
  [[ $ALLOW_DIRTY == 1 ]] || die "packages/cli or this script has uncommitted changes (--allow-dirty for a dry run)"
fi

# 6. Stage: the archive's files, unchanged, and the generated package.json.
pkg="$stage/package"
mkdir -p "$pkg/bin" "$pkg/native"
install -m 0755 "$src/nextrs" "$pkg/native/nextrs"
install -m 0755 packages/cli/bin/nextrs "$pkg/bin/nextrs"
for f in BUILD-INFO.json LICENSE-MIT LICENSE-APACHE THIRD-PARTY.md THIRD-PARTY-LICENSES.txt; do install -m 0644 "$src/$f" "$pkg/$f"; done
install -m 0644 packages/cli/README.md "$pkg/README.md"
node - "$TEMPLATE" "$src/BUILD-INFO.json" "$pkg/package.json" "$archive_sha" <<'EOF'
const fs = require("fs");
const [tpl, info, out, archiveSha] = process.argv.slice(2);
const p = JSON.parse(fs.readFileSync(tpl, "utf8"));
const b = JSON.parse(fs.readFileSync(info, "utf8"));
delete p.private; // the template is private so that it can never be published as is
for (const k of ["scripts", "dependencies", "optionalDependencies", "peerDependencies", "bundleDependencies", "bundledDependencies", "gypfile"])
  if (p[k] !== undefined) throw new Error(`packages/cli/package.json must not have "${k}": the package installs a prebuilt binary and runs nothing`);
p.nextrs = {
  binarySha256: b.binary_sha256,
  binaryCommit: b.binary_commit ?? b.commit,
  releaseCommit: b.commit,
  archiveSha256: archiveSha,
  target: b.target,
  minGlibc: b.min_glibc,
  rustc: b.rustc,
};
fs.writeFileSync(out, JSON.stringify(p, null, 2) + "\n");
EOF

# 7. Pack. npm writes fixed mtimes and modes, so the tarball is a function
#    of the staged bytes. Scripts are off and the user config is ignored, so
#    nothing outside the stage can change it.
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
name="nextrs-sh-cli-$version"
rm -f "$OUT/$name.tgz"
(cd "$pkg" && npm_config_userconfig=/dev/null npm pack --ignore-scripts --pack-destination "$OUT" --json --loglevel=error) > "$stage/pack.json"
[[ -f $OUT/$name.tgz ]] || die "npm pack did not write $name.tgz"
(cd "$OUT" && sha256sum "$name.tgz" > "$name.tgz.sha256")
node - "$stage/pack.json" "$OUT/$name.npm.json" <<EOF
const fs = require("fs");
const j = JSON.parse(fs.readFileSync(process.argv[2], "utf8"))[0];
fs.writeFileSync(process.argv[3], JSON.stringify({
  package: j.name,
  version: j.version,
  tarball: j.filename,
  tarball_sha256: "$(cut -d' ' -f1 "$OUT/$name.tgz.sha256")",
  integrity: j.integrity,
  tarball_bytes: j.size,
  unpacked_bytes: j.unpackedSize,
  files: j.files.map((f) => ({ path: f.path, mode: f.mode.toString(8), bytes: f.size })),
  dist_tag: "$([[ $version == *-* ]] && echo next || echo latest)",
  binary_sha256: "$bin_sha",
  binary_reports: "$reported",
  binary_commit: "$(json "$src/BUILD-INFO.json" binary_commit 2>/dev/null || json "$src/BUILD-INFO.json" commit)",
  archive: "$(basename "$ARCHIVE")",
  archive_sha256: "$archive_sha",
  release_commit: "$(json "$src/BUILD-INFO.json" commit)",
  packaging_commit: "$commit",
  dirty: $dirty,
  min_glibc: "$glibc",
}, null, 2) + "\n");
EOF
echo "$OUT/$name.tgz"
cat "$OUT/$name.tgz.sha256"
"$ROOT/scripts/verify-npm-package.sh" --tarball "$OUT/$name.tgz" --manifest "$manifest" --archive "$ARCHIVE" --source "$ROOT/packages/cli/bin/nextrs" --json "$OUT/$name.verify.json"
