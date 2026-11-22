#!/usr/bin/env bash
# Verify an @nextrs/cli npm tarball against the release it was made from.
#
#   scripts/verify-npm-package.sh --tarball dist/npm/nextrs-cli-0.1.0-rc.1.tgz \
#     --manifest dist/nextrs-v0.1.0-rc.1-linux-x86_64.manifest.json \
#     [--archive dist/nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz] [--json OUT]
#
# Needs bash, tar, gzip, sha256sum, cmp, readelf and node (for JSON). Every
# check is recorded as pass/fail; any failure makes the exit status 1.
#
#   contents        the tarball holds exactly the allowlisted files, nothing else
#   no-secrets      no private key, npm or GitHub token, .env or .npmrc in it
#   launcher        package/bin/nextrs is the POSIX launcher: executable, a #!/bin/sh
#                   script (so pnpm's shim execs it) that execs ../native/nextrs, and
#                   (with --source) identical to packages/cli/bin/nextrs
#   binary-mode     package/native/nextrs is executable in the tarball
#   binary-sha256   the packed binary is the manifest's binary_sha256
#   same-bytes      (--archive) the packed binary and the archive's binary are
#                   identical byte for byte, as are BUILD-INFO.json, the
#                   licenses and the third-party notices
#   metadata        name, version, license, repository, bin, os/cpu/libc,
#                   public access; no scripts, dependencies or gypfile
#   identity        package.json "nextrs" block and BUILD-INFO.json name the
#                   same binary, commit and archive as the manifest
#   versions        package.json, BUILD-INFO.json, the manifest and
#                   `nextrs --version` agree (the last only on Linux x86_64)
#   elf             x86-64 ELF, glibc interpreter, glibc floor 2.35
set -uo pipefail
export LC_ALL=C
TARBALL="" MANIFEST="" ARCHIVE="" JSON="" SOURCE=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --tarball) TARBALL="$2"; shift 2 ;;
    --manifest) MANIFEST="$2"; shift 2 ;;
    --archive) ARCHIVE="$2"; shift 2 ;;
    --json) JSON="$2"; shift 2 ;;
    --source) SOURCE="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -f $TARBALL && -f $MANIFEST ]] || { echo "usage: $0 --tarball FILE.tgz --manifest RELEASE.manifest.json [--archive FILE.tar.gz] [--json OUT]" >&2; exit 2; }
WORK=$(mktemp -d "${TMPDIR:-/tmp}/nextrs-npm-verify.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
declare -a RESULTS=()
FAILED=0
check() { # name, status (0 = pass), detail
  local s=pass; [[ $2 == 0 ]] || { s=fail; FAILED=1; }
  RESULTS+=("{\"check\":\"$1\",\"result\":\"$s\",\"detail\":$(printf '%s' "$3" | head -c 600 | node -e 'let s="";process.stdin.on("data",c=>s+=c).on("end",()=>process.stdout.write(JSON.stringify(s)))')}")
  printf '%-14s %s  %s\n' "$1" "$s" "$(printf '%s' "$3" | head -c 200 | tr '\n' ' ')"
}
jget() { node -e 'let v=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));for(const k of process.argv[2].split("."))v=v?.[k];console.log(v===undefined?"":typeof v==="object"?JSON.stringify(v):v)' "$1" "$2"; }

# contents: exactly the allowlist (npm adds package.json and README.md itself).
EXPECTED="package/BUILD-INFO.json
package/LICENSE-APACHE
package/LICENSE-MIT
package/README.md
package/THIRD-PARTY-LICENSES.txt
package/THIRD-PARTY.md
package/bin/nextrs
package/native/nextrs
package/package.json"
listing=$(tar -tzf "$TARBALL" | grep -v '/$' | sort) # directory entries carry no content
extra=$(comm -13 <(echo "$EXPECTED") <(echo "$listing") | tr '\n' ' ')
missing=$(comm -23 <(echo "$EXPECTED") <(echo "$listing") | tr '\n' ' ')
check contents "$([[ -z $extra && -z $missing ]]; echo $?)" "$(echo "$listing" | wc -l) entries; unexpected: ${extra:-none}; missing: ${missing:-none}"

tar -xzf "$TARBALL" -C "$WORK"
P=$WORK/package
secrets=$(grep -rlE -e '-----BEGIN ([A-Z]+ )?PRIVATE KEY-----' -e 'npm_[A-Za-z0-9]{36}' -e 'gh[pousr]_[A-Za-z0-9]{36}' -e '_authToken' \
  "$P" --exclude-dir=native 2>/dev/null | sed "s#$WORK/##" | tr '\n' ' ')
dotfiles=$(find "$P" -name '.env*' -o -name '.npmrc' -o -name '*.pem' -o -name '*.key' | sed "s#$WORK/##" | tr '\n' ' ')
check no-secrets "$([[ -z $secrets && -z $dotfiles ]]; echo $?)" "key/token patterns: ${secrets:-none}; credential files: ${dotfiles:-none}"

lmode=$(tar -tvzf "$TARBALL" package/bin/nextrs | awk '{print $1}')
lp=""
[[ $lmode == -rwxr-xr-x ]] || lp+=" mode $lmode;"
[[ $(head -c 9 "$P/bin/nextrs") == '#!/bin/sh' ]] || lp+=" no #!/bin/sh line;"
grep -q '^exec "$native" "$@"$' "$P/bin/nextrs" || lp+=" does not exec the native binary;"
[[ $(stat -c %s "$P/bin/nextrs") -lt 8192 ]] || lp+=" larger than 8 KiB;"
[[ -z $SOURCE ]] || cmp -s "$SOURCE" "$P/bin/nextrs" || lp+=" differs from $SOURCE;"
check launcher "$([[ -z $lp ]]; echo $?)" "${lp:-package/bin/nextrs $lmode, $(stat -c %s "$P/bin/nextrs") bytes, #!/bin/sh, execs ../native/nextrs${SOURCE:+, same as the source}}"

mode=$(tar -tvzf "$TARBALL" package/native/nextrs | awk '{print $1}')
check binary-mode "$([[ $mode == -rwxr-xr-x ]]; echo $?)" "package/native/nextrs $mode"

want_sha=$(jget "$MANIFEST" binary_sha256)
got_sha=$(sha256sum "$P/native/nextrs" | cut -d' ' -f1)
check binary-sha256 "$([[ -n $want_sha && $got_sha == "$want_sha" ]]; echo $?)" "packed $got_sha; manifest $want_sha"

if [[ -n $ARCHIVE ]]; then
  mkdir "$WORK/archive"
  tar -xzf "$ARCHIVE" -C "$WORK/archive"
  A=$(find "$WORK/archive" -mindepth 1 -maxdepth 1 -type d -name 'nextrs-v*')
  diffs=""
  cmp -s "$A/nextrs" "$P/native/nextrs" || diffs+=" nextrs"
  for f in BUILD-INFO.json LICENSE-MIT LICENSE-APACHE THIRD-PARTY.md THIRD-PARTY-LICENSES.txt; do cmp -s "$A/$f" "$P/$f" || diffs+=" $f"; done
  archive_sha=$(sha256sum "$ARCHIVE" | cut -d' ' -f1)
  [[ $archive_sha == "$(jget "$MANIFEST" archive_sha256)" ]] || diffs+=" (archive is not the manifest's)"
  check same-bytes "$([[ -z $diffs ]]; echo $?)" "archive $(basename "$ARCHIVE") ${archive_sha:0:12}…: ${diffs:+differs:}${diffs:-binary $(stat -c %s "$P/native/nextrs") bytes, BUILD-INFO.json, licenses and notices identical}"
fi

pj=$P/package.json
problems=$(node - "$pj" <<'EOF'
const p = JSON.parse(require("fs").readFileSync(process.argv[2], "utf8"));
const bad = [];
const eq = (k, v) => JSON.stringify(p[k]) === JSON.stringify(v) || bad.push(`${k}=${JSON.stringify(p[k])}`);
eq("name", "@nextrs/cli");
eq("license", "MIT OR Apache-2.0");
eq("bin", { nextrs: "bin/nextrs" });
eq("os", ["linux"]);
eq("cpu", ["x64"]);
eq("libc", ["glibc"]);
if (p.private) bad.push("private=true");
if (p.publishConfig?.access !== "public") bad.push("publishConfig.access is not public");
if (!/github\.com\/nextrs-sh\/nextrs/.test(p.repository?.url ?? "")) bad.push("repository");
for (const k of ["scripts", "dependencies", "optionalDependencies", "peerDependencies", "bundleDependencies", "bundledDependencies", "gypfile", "binding"])
  if (p[k] !== undefined) bad.push(`has ${k}`);
console.log(bad.join("; "));
EOF
)
check metadata "$([[ -z $problems ]]; echo $?)" "${problems:-name, license, repository, bin, os linux, cpu x64, libc glibc, public; no scripts, dependencies or gypfile}"

version=$(jget "$pj" version)

idp=""
[[ $(jget "$pj" nextrs.binarySha256) == "$want_sha" ]] || idp+=" package.json nextrs.binarySha256"
[[ $(jget "$P/BUILD-INFO.json" binary_sha256) == "$want_sha" ]] || idp+=" BUILD-INFO.json binary_sha256"
[[ $(jget "$pj" nextrs.archiveSha256) == "$(jget "$MANIFEST" archive_sha256)" ]] || idp+=" package.json nextrs.archiveSha256"
mc=$(jget "$MANIFEST" binary_commit); [[ -n $mc ]] || mc=$(jget "$MANIFEST" commit)
[[ $(jget "$pj" nextrs.binaryCommit) == "$mc" ]] || idp+=" binaryCommit"
[[ $(jget "$pj" nextrs.releaseCommit) == "$(jget "$MANIFEST" commit)" ]] || idp+=" releaseCommit"
check identity "$([[ -z $idp ]]; echo $?)" "${idp:+mismatch:}${idp:-binary ${want_sha:0:12}…, binary commit ${mc:0:7}, release commit $(jget "$MANIFEST" commit | cut -c1-7), archive $(jget "$MANIFEST" archive_sha256 | cut -c1-12)…}"

vp="package.json $version, BUILD-INFO.json $(jget "$P/BUILD-INFO.json" version), manifest $(jget "$MANIFEST" version)"
vok=0
[[ $(jget "$P/BUILD-INFO.json" version) == "$version" && $(jget "$MANIFEST" version) == "$version" && $(jget "$MANIFEST" binary_reports) == "nextrs $version" ]] || vok=1
if [[ $(uname -sm) == "Linux x86_64" ]]; then
  reported=$("$P/native/nextrs" --version 2>&1)
  via=$("$P/bin/nextrs" --version 2>&1)
  [[ $reported == "nextrs $version" && $via == "nextrs $version" ]] || vok=1
  vp+=", binary reports '$reported', launcher '$via'"
fi
check versions $vok "$vp"

elf=""
readelf -h "$P/native/nextrs" 2>/dev/null | grep -q 'Machine:.*X86-64' || elf+=" not x86-64;"
readelf -l "$P/native/nextrs" 2>/dev/null | grep -q '/lib64/ld-linux-x86-64.so.2' || elf+=" interpreter is not glibc's;"
glibc=$(objdump -T "$P/native/nextrs" 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -Vu | tail -1)
[[ $glibc == 2.35 && $(jget "$pj" nextrs.minGlibc) == 2.35 ]] || elf+=" glibc floor $glibc (package.json $(jget "$pj" nextrs.minGlibc));"
needed=$(readelf -d "$P/native/nextrs" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | sort | tr '\n' ' ')
check elf "$([[ -z $elf ]]; echo $?)" "${elf:-x86-64 ELF, /lib64/ld-linux-x86-64.so.2, needs GLIBC_$glibc; shared libraries: $needed}"

echo "tarball $(basename "$TARBALL"): $(stat -c %s "$TARBALL") bytes, sha256 $(sha256sum "$TARBALL" | cut -d' ' -f1)"
if [[ -n $JSON ]]; then
  { echo "{\"tarball\":\"$(basename "$TARBALL")\",\"tarball_sha256\":\"$(sha256sum "$TARBALL" | cut -d' ' -f1)\",\"passed\":$([[ $FAILED == 0 ]] && echo true || echo false),\"checks\":["; IFS=,; echo "${RESULTS[*]}"; echo "]}"; } > "$JSON"
fi
exit $FAILED
