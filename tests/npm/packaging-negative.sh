#!/usr/bin/env bash
# Negative tests of scripts/package-npm.sh and scripts/verify-npm-package.sh:
# each case builds a broken input and expects a refusal that names the
# problem. Runs on the builder (bash, node, npm, tar, readelf).
#
#   tests/npm/packaging-negative.sh --archive dist/nextrs-vVERSION-linux-x86_64.tar.gz \
#     --tarball dist/npm/nextrs-cli-VERSION.tgz [--json OUT]
#
# The archive (with .sha256 and .manifest.json) and the tarball must be a
# good pair: the script first checks that they pass, then breaks copies.
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ARCHIVE="" TARBALL="" JSON=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) ARCHIVE="$2"; shift 2 ;;
    --tarball) TARBALL="$2"; shift 2 ;;
    --json) JSON="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -f $ARCHIVE && -f $TARBALL ]] || { echo "usage: $0 --archive FILE.tar.gz --tarball FILE.tgz [--json OUT]" >&2; exit 2; }
ARCHIVE=$(cd "$(dirname "$ARCHIVE")" && pwd)/$(basename "$ARCHIVE")
TARBALL=$(cd "$(dirname "$TARBALL")" && pwd)/$(basename "$TARBALL")
MANIFEST=${ARCHIVE%.tar.gz}.manifest.json
T=$(mktemp -d "${TMPDIR:-/tmp}/nextrs-npm-neg.XXXXXX")
trap 'rm -rf "$T"' EXIT
declare -a RESULTS=()
FAILED=0
check() {
  local s=pass; [[ $2 == 0 ]] || { s=fail; FAILED=1; }
  RESULTS+=("$(node -e 'console.log(JSON.stringify({check:process.argv[1],result:process.argv[2],detail:process.argv[3].slice(0,500)}))' "$1" "$s" "$3")")
  printf '%-28s %s  %s\n' "$1" "$s" "$(printf '%s' "$3" | head -c 170 | tr '\n' ' ')"
}
PACK=("$ROOT/scripts/package-npm.sh" --allow-dirty)
VERIFY="$ROOT/scripts/verify-npm-package.sh"
name=$(basename "$ARCHIVE" .tar.gz)
# A version no release will ever have: the cases that inject a wrong version
# must not go stale when the real version is bumped.
WRONG=0.0.0-negative-test
[[ $(node -p 'require(process.argv[1]).version' "$MANIFEST") != "$WRONG" ]] || { echo "the manifest version is $WRONG" >&2; exit 2; }

# refused CASE PATTERN CMD...: the command must fail, and its output match PATTERN.
refused() {
  local case=$1 pat=$2; shift 2
  local out rc
  out=$("$@" 2>&1); rc=$?
  check "$case" "$([[ $rc != 0 ]] && echo "$out" | grep -qE "$pat"; echo $?)" "exit $rc: $(echo "$out" | grep -E "$pat|package-npm:| fail " | head -2 | tr '\n' ' ')"
}
# A copy of the release files in DIR, with the archive rebuilt from an edited
# tree (edit is a shell snippet run in the extracted directory). The .sha256
# and manifest are made consistent with the new archive unless KEEP_SUMS=1.
remake() {
  local dir=$1 edit=$2
  mkdir -p "$dir/x"
  tar -xzf "$ARCHIVE" -C "$dir/x"
  (cd "$dir/x/$name" && eval "$edit")
  tar -czf "$dir/$name.tar.gz" -C "$dir/x" "$name"
  local bsha asha
  bsha=$(sha256sum "$dir/x/$name/nextrs" | cut -d' ' -f1)
  asha=$(sha256sum "$dir/$name.tar.gz" | cut -d' ' -f1)
  (cd "$dir" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
  node -e 'const fs=require("fs");const m=JSON.parse(fs.readFileSync(process.argv[1]));m.archive_sha256=process.argv[3];m.binary_sha256=process.argv[4];fs.writeFileSync(process.argv[2],JSON.stringify(m,null,2))' \
    "$MANIFEST" "$dir/$name.manifest.json" "$asha" "$bsha"
}
setinfo() { # key value: edit BUILD-INFO.json in the current directory
  node -e 'const fs=require("fs");const f="BUILD-INFO.json";const j=JSON.parse(fs.readFileSync(f));j[process.argv[1]]=process.argv[2];fs.writeFileSync(f,JSON.stringify(j,null,2))' "$1" "$2"
}
export -f setinfo

# 0. The good pair passes (otherwise the cases below prove nothing).
"$VERIFY" --tarball "$TARBALL" --manifest "$MANIFEST" --archive "$ARCHIVE" > "$T/good.txt" 2>&1
check baseline-verifies $? "the given archive and tarball pass verify-npm-package.sh"

# ------------------------------------------------------- package-npm.sh
mkdir "$T/a1" && cp "$ARCHIVE" "$MANIFEST" "$T/a1/" && echo "0000000000000000000000000000000000000000000000000000000000000000  $name.tar.gz" > "$T/a1/$name.tar.gz.sha256"
refused pack:archive-checksum 'does not match its .sha256' "${PACK[@]}" --archive "$T/a1/$name.tar.gz" --out "$T/o1"

mkdir "$T/a2" && cp "$ARCHIVE" "$ARCHIVE.sha256" "$T/a2/"
node -e 'const fs=require("fs");const m=JSON.parse(fs.readFileSync(process.argv[1]));m.binary_sha256="ab".repeat(32);fs.writeFileSync(process.argv[2],JSON.stringify(m))' "$MANIFEST" "$T/a2/$name.manifest.json"
refused pack:manifest-binary 'the manifest says' "${PACK[@]}" --archive "$T/a2/$name.tar.gz" --out "$T/o2"

refused pack:expected-sha 'not the expected' "${PACK[@]}" --archive "$ARCHIVE" --expect-binary-sha256 "$(printf 'cd%.0s' {1..32})" --out "$T/o3"

remake "$T/a4" 'printf "#!/bin/sh\necho \"nextrs $WRONG\"\n" > nextrs; chmod 755 nextrs; setinfo binary_sha256 "$(sha256sum nextrs | cut -d" " -f1)"'
refused pack:binary-version "reports 'nextrs $WRONG'" "${PACK[@]}" --archive "$T/a4/$name.tar.gz" --out "$T/o4"

node -e 'const fs=require("fs");const p=JSON.parse(fs.readFileSync(process.argv[1]));p.version=process.argv[3];fs.writeFileSync(process.argv[2],JSON.stringify(p))' "$ROOT/packages/cli/package.json" "$T/template-wrong.json" "$WRONG"
refused pack:template-version "template-wrong.json is $WRONG" "${PACK[@]}" --archive "$ARCHIVE" --template "$T/template-wrong.json" --out "$T/o5"

remake "$T/a6" 'setinfo version "$WRONG"'
refused pack:build-info-version "BUILD-INFO.json is $WRONG" "${PACK[@]}" --archive "$T/a6/$name.tar.gz" --out "$T/o6"

echo "$(node -p 'require(process.argv[1]).binary_sha256' "$MANIFEST")  binary  test: this build is withdrawn" > "$T/withdrawn.txt"
refused pack:withdrawn 'withdrawn build' "${PACK[@]}" --archive "$ARCHIVE" --withdrawn "$T/withdrawn.txt" --out "$T/o7"

remake "$T/a8" 'rm THIRD-PARTY-LICENSES.txt'
refused pack:missing-notices 'has no THIRD-PARTY-LICENSES.txt' "${PACK[@]}" --archive "$T/a8/$name.tar.gz" --out "$T/o8"

node -e 'const fs=require("fs");const p=JSON.parse(fs.readFileSync(process.argv[1]));p.scripts={postinstall:"node fetch.js"};fs.writeFileSync(process.argv[2],JSON.stringify(p))' "$ROOT/packages/cli/package.json" "$T/template-scripts.json"
refused pack:lifecycle-script 'must not have "scripts"' "${PACK[@]}" --archive "$ARCHIVE" --template "$T/template-scripts.json" --out "$T/o9"

# --------------------------------------------------- verify-npm-package.sh
# repack DIR EDIT: an edited copy of the npm tarball (edit runs in package/).
repack() {
  local dir=$1 edit=$2
  mkdir -p "$dir"
  tar -xzf "$TARBALL" -C "$dir"
  (cd "$dir/package" && eval "$edit")
  tar -czf "$dir/pkg.tgz" -C "$dir" package
}
setpkg() { node -e 'const fs=require("fs");const p=JSON.parse(fs.readFileSync("package.json"));eval(process.argv[1]);fs.writeFileSync("package.json",JSON.stringify(p,null,2))' "$1"; }
export -f setpkg
repack "$T/v1" 'echo "NPM_TOKEN=npm_abcdefghijklmnopqrstuvwxyz0123456789" > .env'
refused verify:extra-file 'contents +fail.*unexpected: package/.env' "$VERIFY" --tarball "$T/v1/pkg.tgz" --manifest "$MANIFEST"
refused verify:secret 'no-secrets +fail' "$VERIFY" --tarball "$T/v1/pkg.tgz" --manifest "$MANIFEST"
repack "$T/v2" 'chmod 0644 native/nextrs'
refused verify:binary-mode 'binary-mode +fail' "$VERIFY" --tarball "$T/v2/pkg.tgz" --manifest "$MANIFEST"
repack "$T/v3" "setpkg 'p.version=\"$WRONG\"'"
refused verify:version-mismatch 'versions +fail' "$VERIFY" --tarball "$T/v3/pkg.tgz" --manifest "$MANIFEST"
repack "$T/v5" 'setpkg "p.scripts={install:\"node-gyp rebuild\"}"'
refused verify:install-script 'metadata +fail.*has scripts' "$VERIFY" --tarball "$T/v5/pkg.tgz" --manifest "$MANIFEST"
repack "$T/v6" 'printf X | dd of=native/nextrs bs=1 seek=8192 conv=notrunc 2> /dev/null'
refused verify:binary-bytes 'binary-sha256 +fail' "$VERIFY" --tarball "$T/v6/pkg.tgz" --manifest "$MANIFEST" --archive "$ARCHIVE"
refused verify:not-same-as-archive 'same-bytes +fail' "$VERIFY" --tarball "$T/v6/pkg.tgz" --manifest "$MANIFEST" --archive "$ARCHIVE"
repack "$T/v7" 'setpkg "p.cpu=[\"x64\",\"arm64\"]"'
refused verify:platform-widened 'metadata +fail.*cpu' "$VERIFY" --tarball "$T/v7/pkg.tgz" --manifest "$MANIFEST"
repack "$T/v8" 'printf "#!/bin/sh\nnode \"\$(dirname \"\$0\")/../native/nextrs\" \"\$@\"\n" > bin/nextrs'
refused verify:launcher-no-exec 'launcher +fail' "$VERIFY" --tarball "$T/v8/pkg.tgz" --manifest "$MANIFEST"

if [[ -n $JSON ]]; then
  { echo "{\"passed\":$([[ $FAILED == 0 ]] && echo true || echo false),\"checks\":["; IFS=,; echo "${RESULTS[*]}"; echo "]}"; } > "$JSON"
fi
exit $FAILED
