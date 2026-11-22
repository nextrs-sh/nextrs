#!/usr/bin/env bash
# Tests for scripts/publish-release.sh: the prerelease/stable channel, the
# publish commands (GitHub release flags, npm dist-tag, provenance) and the
# artifact checks that run before them. Uses fake artifacts and fake `gh`
# and `npm` that fail if called, so nothing is published. Run by
# release.yml.
set -euo pipefail
cd "$(dirname "$0")/.."
script=scripts/publish-release.sh

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin"
for tool in gh npm; do
  printf '#!/bin/sh\necho "%s $*" >> "%s/called"\nexit 1\n' "$tool" "$tmp" > "$tmp/bin/$tool"
  chmod +x "$tmp/bin/$tool"
done
# macOS has shasum, not sha256sum.
if ! command -v sha256sum >/dev/null; then
  printf '#!/bin/sh\nexec shasum -a 256 "$@"\n' > "$tmp/bin/sha256sum"
  chmod +x "$tmp/bin/sha256sum"
fi
export PATH="$tmp/bin:$PATH"

fails=0 passes=0
ok() { passes=$((passes + 1)); echo "ok    $1"; }
bad() { fails=$((fails + 1)); echo "FAIL  $1"; [[ -z ${2:-} ]] || printf '%s\n' "$2" | sed 's/^/      /'; }

# expect_out NAME EXPECTED CMD...: CMD succeeds and prints EXPECTED exactly.
expect_out() {
  local name=$1 want=$2 got; shift 2
  if got=$("$@" 2>&1) && [[ $got == "$want" ]]; then ok "$name"; else bad "$name" "$got"; fi
}
# expect_fail NAME PATTERN CMD...: CMD fails with a message matching PATTERN.
expect_fail() {
  local name=$1 pat=$2 got; shift 2
  if got=$("$@" 2>&1); then bad "$name (succeeded)" "$got"
  elif [[ $got == *"$pat"* ]]; then ok "$name"
  else bad "$name (wrong error)" "$got"; fi
}
# expect_has NAME OUTPUT WORD...: OUTPUT contains every WORD.
expect_has() {
  local name=$1 out=$2 w; shift 2
  for w in "$@"; do [[ $out == *"$w"* ]] || { bad "$name: missing '$w'" "$out"; return; }; done
  ok "$name"
}
expect_lacks() {
  local name=$1 out=$2 w; shift 2
  for w in "$@"; do [[ $out != *"$w"* ]] || { bad "$name: has '$w'" "$out"; return; }; done
  ok "$name"
}

# --- channel -----------------------------------------------------------
for v in 0.1.0 1.0.0 10.20.30; do
  expect_out "channel $v is stable" $'npm_tag=latest\nprerelease=false' "$script" channel "$v"
done
for v in 0.1.0-rc.3 0.2.0-rc.1 1.0.0-beta.2 1.0.0-alpha.0 0.1.0-rc.10; do
  expect_out "channel $v is a prerelease" $'npm_tag=next\nprerelease=true' "$script" channel "$v"
done
for v in v0.1.0 0.1 0.1.0-rc 0.1.0-RC.1 0.1.0-rc.3+build 0.1.0-pre.1 0.1.0-rc.1.2 "0.1.0 " " 0.1.0" 0.1.0-rc.x; do
  expect_fail "channel '$v' is refused" "not a release version" "$script" channel "$v"
done
expect_fail "empty version is refused" "usage" "$script" channel ""

# --- fake artifacts ------------------------------------------------------
# make_dist DIR VERSION [PRERELEASE] [DIST_TAG]: what the build job uploads.
make_dist() {
  local d=$1 v=$2 pre=${3:-} tag=${4:-} a
  [[ -n $pre ]] || { [[ $v == *-* ]] && pre=true || pre=false; }
  [[ -n $tag ]] || { [[ $v == *-* ]] && tag=next || tag=latest; }
  mkdir -p "$d/npm"
  a="nextrs-v$v-linux-x86_64"
  echo "archive $v" > "$d/$a.tar.gz"
  (cd "$d" && sha256sum "$a.tar.gz" > "$a.tar.gz.sha256")
  local asha; asha=$(cut -d' ' -f1 "$d/$a.tar.gz.sha256")
  printf '{"version":"%s","prerelease":%s,"archive_sha256":"%s"}\n' "$v" "$pre" "$asha" > "$d/$a.manifest.json"
  echo "tarball $v" > "$d/npm/nextrs-cli-$v.tgz"
  (cd "$d/npm" && sha256sum "nextrs-cli-$v.tgz" > "nextrs-cli-$v.tgz.sha256")
  local tsha; tsha=$(cut -d' ' -f1 "$d/npm/nextrs-cli-$v.tgz.sha256")
  printf '{"version":"%s","dist_tag":"%s","tarball_sha256":"%s","archive_sha256":"%s"}\n' "$v" "$tag" "$tsha" "$asha" > "$d/npm/nextrs-cli-$v.npm.json"
}

# --- publish commands (dry run) -------------------------------------------
# 0.1.0-rc.2 has committed release notes; 9.9.9 and 9.9.9-beta.1 do not.
for v in 0.1.0-rc.2 9.9.9-beta.1 9.9.9; do
  d="$tmp/dist-$v"; make_dist "$d" "$v"
  gh_out=$("$script" github "$v" --dist "$d" --dry-run 2>&1) || bad "github $v --dry-run" "$gh_out"
  npm_out=$("$script" npm "$v" --dist "$d" --dry-run 2>&1) || bad "npm $v --dry-run" "$npm_out"
  expect_has "github $v: tag, title, assets" "$gh_out" "dry-run: gh release create v$v --verify-tag" "--title NextRS\\ v$v" \
    "$d/nextrs-v$v-linux-x86_64.tar.gz " "$d/nextrs-v$v-linux-x86_64.tar.gz.sha256 " "$d/nextrs-v$v-linux-x86_64.manifest.json"
  expect_has "npm $v: provenance, public" "$npm_out" "dry-run: npm publish $d/npm/nextrs-cli-$v.tgz " "--access public" "--provenance"
  if [[ $v == *-* ]]; then
    expect_has "github $v is a prerelease, not latest" "$gh_out" "--prerelease --latest=false"
    expect_has "npm $v goes to next" "$npm_out" "--tag next "
    expect_lacks "npm $v never names latest" "$npm_out" "latest"
  else
    expect_has "github $v is the latest release" "$gh_out" "--latest "
    expect_lacks "github $v is not a prerelease" "$gh_out" "--prerelease" "--latest=false"
    expect_has "npm $v goes to latest" "$npm_out" "--tag latest "
  fi
  if [[ -f release/v$v-release-notes.md ]]; then
    expect_has "github $v uses its release notes" "$gh_out" "--notes-file release/v$v-release-notes.md"
  else
    expect_has "github $v generates notes" "$gh_out" "--generate-notes"
  fi
done

# A relative --dist gets "./" so npm reads a file, not a GitHub repository.
rel="dist-rel-test-$$"; make_dist "$rel" 9.9.9-rc.1
trap 'rm -rf "$tmp" "$rel"' EXIT
npm_out=$("$script" npm 9.9.9-rc.1 --dist "$rel" --dry-run 2>&1) || true
expect_has "relative tarball path starts with ./" "$npm_out" "npm publish ./$rel/npm/nextrs-cli-9.9.9-rc.1.tgz"
rm -rf "$rel"

# --- artifact checks -----------------------------------------------------
v=0.1.0-rc.3
fresh() { rm -rf "$tmp/d"; make_dist "$tmp/d" "$@"; }

fresh "$v"; expect_out "verify passes on good artifacts" "verified: v$v (prerelease, npm next)" "$script" verify "$v" --dist "$tmp/d"
fresh 1.0.0; expect_out "verify reports a stable channel" "verified: v1.0.0 (stable, npm latest)" "$script" verify 1.0.0 --dist "$tmp/d"

fresh "$v"; echo tampered >> "$tmp/d/nextrs-v$v-linux-x86_64.tar.gz"
expect_fail "changed archive is refused" "does not match its .sha256" "$script" npm "$v" --dist "$tmp/d" --dry-run
fresh "$v"; echo tampered >> "$tmp/d/npm/nextrs-cli-$v.tgz"
expect_fail "changed npm tarball is refused" "does not match its .sha256" "$script" npm "$v" --dist "$tmp/d" --dry-run
fresh "$v"; rm "$tmp/d/npm/nextrs-cli-$v.tgz.sha256"
expect_fail "missing tarball checksum is refused" "missing" "$script" github "$v" --dist "$tmp/d" --dry-run
fresh "$v" true latest
expect_fail "prerelease packed for latest is refused" "dist_tag is latest, $v needs next" "$script" npm "$v" --dist "$tmp/d" --dry-run
fresh 1.0.0 false next
expect_fail "stable packed for next is refused" "dist_tag is next, 1.0.0 needs latest" "$script" npm 1.0.0 --dist "$tmp/d" --dry-run
fresh "$v" false
expect_fail "manifest calling a prerelease stable is refused" "prerelease is false" "$script" github "$v" --dist "$tmp/d" --dry-run
rm -rf "$tmp/d"; make_dist "$tmp/d" 0.1.0-rc.2; mv "$tmp/d/nextrs-v0.1.0-rc.2-linux-x86_64.tar.gz" "$tmp/d/nextrs-v$v-linux-x86_64.tar.gz"
expect_fail "artifacts of another version are refused" "missing" "$script" verify "$v" --dist "$tmp/d"
fresh "$v"; other=$(printf '%064d' 0)
node -e 'const f=process.argv[1], j=JSON.parse(require("fs").readFileSync(f,"utf8")); j.archive_sha256=process.argv[2]; require("fs").writeFileSync(f, JSON.stringify(j))' "$tmp/d/npm/nextrs-cli-$v.npm.json" "$other"
expect_fail "tarball packed from another archive is refused" "not packed from this archive" "$script" npm "$v" --dist "$tmp/d" --dry-run
expect_fail "unknown command is refused" "unknown command" "$script" release "$v"
expect_fail "unknown option is refused" "unknown option" "$script" npm "$v" --tag latest

# Nothing above may have run gh or npm.
if [[ -s $tmp/called ]]; then bad "dry runs never call gh or npm" "$(cat "$tmp/called")"; else ok "dry runs never call gh or npm"; fi

echo "$passes passed, $fails failed"
[[ $fails == 0 ]]
