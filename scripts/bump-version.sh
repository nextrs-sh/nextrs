#!/usr/bin/env bash
# Set the release version everywhere the release workflow checks it:
# Cargo.toml ([workspace.package]), Cargo.lock (the workspace crates) and
# packages/cli/package.json.
#
#   scripts/bump-version.sh 0.1.0
#
# Then merge the change into main, and tag it:
#   git tag v0.1.0 origin/main && git push origin v0.1.0
set -euo pipefail
cd "$(dirname "$0")/.."

v=${1:-}
[[ $v =~ ^[0-9]+\.[0-9]+\.[0-9]+(-(alpha|beta|rc)\.[0-9]+)?$ ]] || { echo "usage: $0 <version>, e.g. 0.1.0 or 0.2.0-rc.1" >&2; exit 1; }

# Cargo.toml: the version line inside [workspace.package] only.
awk -v v="$v" '
  /^\[/ { in_ws = ($0 == "[workspace.package]") }
  in_ws && /^version = "/ { $0 = "version = \"" v "\"" }
  { print }
' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml

# Cargo.lock: the version line right after each workspace crate's name.
awk -v v="$v" '
  /^name = "nextrs-(adapter|cli|image|runtime|server)"$/ { print; getline; $0 = "version = \"" v "\"" }
  { print }
' Cargo.lock > Cargo.lock.tmp && mv Cargo.lock.tmp Cargo.lock

# package.json, keeping its formatting (two-space JSON, trailing newline).
node -e '
  const fs = require("fs"), f = "packages/cli/package.json";
  const p = JSON.parse(fs.readFileSync(f, "utf8"));
  p.version = process.argv[1];
  fs.writeFileSync(f, JSON.stringify(p, null, 2) + "\n");
' "$v"

git --no-pager diff --stat -- Cargo.toml Cargo.lock packages/cli/package.json
echo "version $v: open a pull request, merge it, then: git tag v$v origin/main && git push origin v$v"
