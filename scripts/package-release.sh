#!/usr/bin/env bash
# Package a NextRS release archive from an already built binary.
#
#   scripts/package-release.sh --binary target/dist/nextrs --version 0.1.0-rc.1 [--out dist]
#     [--built-from COMMIT]
#
# Produces, in --out:
#   nextrs-v<version>-linux-x86_64.tar.gz          the archive
#   nextrs-v<version>-linux-x86_64.tar.gz.sha256   its checksum (sha256sum format)
#   nextrs-v<version>-linux-x86_64.manifest.json   version, commit, toolchain, checksums
#
# The archive holds one directory, nextrs-v<version>-linux-x86_64/, with the
# binary, BUILD-INFO.json, LICENSE-MIT and LICENSE-APACHE (NextRS's own
# licenses), THIRD-PARTY.md (the components and their licenses),
# THIRD-PARTY-LICENSES.txt (their license and notice texts, from
# scripts/third-party-licenses.mjs), README.md (the install guide,
# release/README.md) and RELEASE-NOTES.md (release/v<version>-release-notes.md).
# It is deterministic for a given binary and commit: entries sorted, owner
# 0, mtime from the commit date, gzip without a timestamp.
#
# --built-from COMMIT packages a binary that was built at an earlier commit
# (for example by the release workflow) with the release documents of
# HEAD. It is accepted only if Cargo.toml, Cargo.lock and crates/ are
# identical at COMMIT and HEAD; BUILD-INFO.json then records both commits.
#
# Refuses to package when the binary's --version is not exactly
# "nextrs <version>", when the workspace version differs, when the
# compiler recorded in the binary is not the --toolchain one, when the
# binary or a release document matches a withdrawn checksum
# (release/WITHDRAWN.txt), or when the working tree has uncommitted changes
# to tracked files (--allow-dirty to override for a local dry run; the
# manifest then says dirty: true).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
BIN="" VERSION="" OUT=dist ALLOW_DIRTY=0 PROFILE=dist TOOLCHAIN="" BUILT_FROM=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) BIN="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --profile) PROFILE="$2"; shift 2 ;;
    --toolchain) TOOLCHAIN="$2"; shift 2 ;;
    --built-from) BUILT_FROM="$2"; shift 2 ;;
    --allow-dirty) ALLOW_DIRTY=1; shift ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -n $BIN && -n $VERSION ]] || { echo "usage: $0 --binary PATH --version X.Y.Z[-pre]" >&2; exit 2; }
[[ -x $BIN ]] || { echo "not an executable: $BIN" >&2; exit 1; }

# 1. Every version source must agree.
workspace=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml)
reported=$("$BIN" --version)
[[ $workspace == "$VERSION" ]] || { echo "Cargo workspace is $workspace, not $VERSION" >&2; exit 1; }
[[ $reported == "nextrs $VERSION" ]] || { echo "binary reports '$reported', not 'nextrs $VERSION'" >&2; exit 1; }
for crate in nextrs-adapter nextrs-cli nextrs-image nextrs-runtime nextrs-server; do
  v=$(awk -v n="$crate" '$0=="name = \""n"\"" {getline; gsub(/version = |"/,""); print; exit}' Cargo.lock)
  [[ $v == "$VERSION" ]] || { echo "Cargo.lock has $crate $v, not $VERSION" >&2; exit 1; }
done
notes="release/v$VERSION-release-notes.md"
[[ -f $notes ]] || { echo "missing $notes" >&2; exit 1; }
for f in release/README.md LICENSE-MIT LICENSE-APACHE; do [[ -f $f ]] || { echo "missing $f" >&2; exit 1; }; done

# 1b. Never a withdrawn build, and no withdrawn checksum in what is shipped.
bin_sha=$(sha256sum "$BIN" | cut -d' ' -f1)
if [[ -f release/WITHDRAWN.txt ]]; then
  while read -r sha _; do
    [[ $sha =~ ^[0-9a-f]{64}$ ]] || continue
    [[ $bin_sha != "$sha" ]] || { echo "the binary is the withdrawn build $sha (release/WITHDRAWN.txt)" >&2; exit 1; }
    if grep -l -e "$sha" -e "${sha:0:8}" "$notes" release/README.md > /dev/null 2>&1; then
      echo "a release document names the withdrawn checksum ${sha:0:12}: $(grep -l -e "$sha" -e "${sha:0:8}" "$notes" release/README.md | tr '\n' ' ')" >&2; exit 1
    fi
  done < release/WITHDRAWN.txt
fi

# 1c. The compiler that built the binary (recorded in its .comment section).
rustc_v=$({ readelf -p .comment "$BIN" 2>/dev/null || true; } | grep -o 'rustc version [^)]*)' | head -1 | sed 's/^rustc version /rustc /' || true)
[[ -n $rustc_v ]] || rustc_v=unknown
if [[ -n $TOOLCHAIN && $rustc_v != "rustc $TOOLCHAIN "* ]]; then
  echo "the binary was built by '$rustc_v', not rustc $TOOLCHAIN" >&2; exit 1
fi

# 2. Source state.
commit=$(git rev-parse HEAD)
binary_commit=$commit
if [[ -n $BUILT_FROM ]]; then
  binary_commit=$(git rev-parse --verify "$BUILT_FROM^{commit}") || exit 1
  git merge-base --is-ancestor "$binary_commit" HEAD || { echo "$BUILT_FROM is not an ancestor of HEAD" >&2; exit 1; }
  if ! git diff --quiet "$binary_commit" HEAD -- Cargo.toml Cargo.lock crates; then
    echo "Cargo.toml, Cargo.lock or crates/ changed since $BUILT_FROM: build the binary at HEAD" >&2
    git diff --stat "$binary_commit" HEAD -- Cargo.toml Cargo.lock crates >&2; exit 1
  fi
fi
dirty=false
if ! git diff --quiet HEAD --; then
  dirty=true
  [[ $ALLOW_DIRTY == 1 ]] || { echo "tracked files have uncommitted changes (--allow-dirty for a dry run)" >&2; exit 1; }
fi
epoch=$(git log -1 --format=%ct HEAD)
glibc=$(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -Vu | tail -1)

# 3. Stage.
name="nextrs-v$VERSION-linux-x86_64"
mkdir -p "$OUT"
OUT=$(cd "$OUT" && pwd)
stage=$(mktemp -d "$OUT/.stage.XXXXXX")
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/$name"
install -m 0755 "$BIN" "$stage/$name/nextrs"
install -m 0644 release/README.md "$stage/$name/README.md"
install -m 0644 "$notes" "$stage/$name/RELEASE-NOTES.md"
install -m 0644 LICENSE-MIT "$stage/$name/LICENSE-MIT"
install -m 0644 LICENSE-APACHE "$stage/$name/LICENSE-APACHE"
cat > "$stage/$name/BUILD-INFO.json" <<EOF
{
  "name": "nextrs",
  "version": "$VERSION",
  "tag": "v$VERSION",
  "commit": "$commit",
  "binary_commit": "$binary_commit",
  "dirty": $dirty,
  "target": "x86_64-unknown-linux-gnu",
  "cargo_profile": "$PROFILE",
  "rustc": "$rustc_v",
  "min_glibc": "$glibc",
  "runtime_libraries": "$(readelf -d "$BIN" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | sort | tr '\n' ' ' | sed 's/ $//')",
  "binary_sha256": "$bin_sha",
  "source_date_epoch": $epoch
}
EOF
# Third-party components: the table and the license and notice texts.
cargo metadata --format-version 1 --filter-platform x86_64-unknown-linux-gnu --locked 2>/dev/null |
  node scripts/third-party-licenses.mjs --table "$stage/$name/THIRD-PARTY.md" --texts "$stage/$name/THIRD-PARTY-LICENSES.txt"
[[ -s $stage/$name/THIRD-PARTY.md && -s $stage/$name/THIRD-PARTY-LICENSES.txt ]] || { echo "could not write the third-party notices" >&2; exit 1; }

# 4. Deterministic archive.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$epoch" \
  --pax-option=exthdr.name=%d/PaxHeaders/%f,delete=atime,delete=ctime \
  -C "$stage" -cf - "$name" | gzip -n -9 > "$OUT/$name.tar.gz"
(cd "$OUT" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
archive_sha=$(cut -d' ' -f1 "$OUT/$name.tar.gz.sha256")
cat > "$OUT/$name.manifest.json" <<EOF
{
  "artifact": "$name.tar.gz",
  "archive_sha256": "$archive_sha",
  "archive_bytes": $(stat -c %s "$OUT/$name.tar.gz"),
  "binary_sha256": "$bin_sha",
  "binary_bytes": $(stat -c %s "$BIN"),
  "version": "$VERSION",
  "binary_reports": "$reported",
  "proposed_tag": "v$VERSION",
  "prerelease": $([[ $VERSION == *-* ]] && echo true || echo false),
  "commit": "$commit",
  "binary_commit": "$binary_commit",
  "dirty": $dirty,
  "target": "x86_64-unknown-linux-gnu",
  "cargo_profile": "$PROFILE",
  "rustc": "$rustc_v",
  "min_glibc": "$glibc",
  "source_date_epoch": $epoch
}
EOF
echo "$OUT/$name.tar.gz"
cat "$OUT/$name.tar.gz.sha256"
