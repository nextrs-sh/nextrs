#!/usr/bin/env bash
# Package a NextRS release archive from an already built binary.
#
#   scripts/package-release.sh --binary target/dist/nextrs --version 0.1.0-rc.1 [--out dist]
#
# Produces, in --out:
#   nextrs-v<version>-linux-x86_64.tar.gz          the archive
#   nextrs-v<version>-linux-x86_64.tar.gz.sha256   its checksum (sha256sum format)
#   nextrs-v<version>-linux-x86_64.manifest.json   version, commit, toolchain, checksums
#
# The archive holds one directory, nextrs-v<version>-linux-x86_64/, with the
# binary, BUILD-INFO.json, THIRD-PARTY.md (crate licenses), README.md (the
# install guide, release/README.md) and RELEASE-NOTES.md
# (release/v<version>-release-notes.md). It is deterministic for a given
# binary and commit: entries sorted, owner 0, mtime from the commit date,
# gzip without a timestamp.
#
# Refuses to package when the binary's --version is not exactly
# "nextrs <version>", when the workspace version differs, or when the
# working tree has uncommitted changes to tracked files (--allow-dirty to
# override for a local dry run; the manifest then says dirty: true).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
BIN="" VERSION="" OUT=dist ALLOW_DIRTY=0 PROFILE=dist TOOLCHAIN=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary) BIN="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --profile) PROFILE="$2"; shift 2 ;;
    --toolchain) TOOLCHAIN="$2"; shift 2 ;;
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
[[ -f release/README.md ]] || { echo "missing release/README.md" >&2; exit 1; }

# 2. Source state.
commit=$(git rev-parse HEAD)
dirty=false
if ! git diff --quiet HEAD --; then
  dirty=true
  [[ $ALLOW_DIRTY == 1 ]] || { echo "tracked files have uncommitted changes (--allow-dirty for a dry run)" >&2; exit 1; }
fi
epoch=$(git log -1 --format=%ct HEAD)
rustc_v=$(rustc ${TOOLCHAIN:++$TOOLCHAIN} --version 2>/dev/null || echo unknown)
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
bin_sha=$(sha256sum "$BIN" | cut -d' ' -f1)
cat > "$stage/$name/BUILD-INFO.json" <<EOF
{
  "name": "nextrs",
  "version": "$VERSION",
  "tag": "v$VERSION",
  "commit": "$commit",
  "dirty": $dirty,
  "target": "x86_64-unknown-linux-gnu",
  "cargo_profile": "$PROFILE",
  "rustc": "$rustc_v",
  "min_glibc": "$glibc",
  "binary_sha256": "$bin_sha",
  "source_date_epoch": $epoch
}
EOF
# Third-party crate licenses (names, versions, SPDX expressions) for the
# binary's dependency graph, from cargo metadata.
cargo metadata --format-version 1 --filter-platform x86_64-unknown-linux-gnu --locked 2>/dev/null |
  node -e '
    let s = ""; process.stdin.on("data", (c) => (s += c)).on("end", () => {
      const m = JSON.parse(s);
      const ids = new Set(); const byId = new Map(m.resolve.nodes.map((n) => [n.id, n]));
      const root = m.packages.find((p) => p.name === "nextrs-cli").id;
      const walk = (id) => { if (ids.has(id)) return; ids.add(id); for (const d of byId.get(id).deps) if (d.dep_kinds.some((k) => k.kind === null)) walk(d.pkg); };
      walk(root);
      const rows = m.packages.filter((p) => ids.has(p.id) && !p.name.startsWith("nextrs-")).sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
      console.log("# Third-party components\n");
      console.log("The nextrs binary statically links the Rust crates below (normal dependencies of nextrs-cli for x86_64-unknown-linux-gnu, from Cargo.lock) and, through the `v8` crate, Google V8 (BSD-3-Clause). License texts are in each crate'"'"'s source distribution on crates.io.\n");
      console.log("| Crate | Version | License |\n|---|---|---|");
      for (const p of rows) console.log(`| ${p.name} | ${p.version} | ${p.license ?? (p.license_file ? "see " + p.license_file : "unspecified")} |`);
    });' > "$stage/$name/THIRD-PARTY.md"
[[ -s $stage/$name/THIRD-PARTY.md ]] || { echo "could not list third-party crates" >&2; exit 1; }

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
