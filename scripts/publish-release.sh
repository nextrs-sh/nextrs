#!/usr/bin/env bash
# Publish a release built by release.yml: the GitHub release (archive,
# checksum, manifest) and @nextrs/cli on npm.
#
#   scripts/publish-release.sh channel <version>
#       prints npm_tag=... and prerelease=... (for $GITHUB_OUTPUT)
#   scripts/publish-release.sh verify <version> [--dist DIR]
#   scripts/publish-release.sh github <version> [--dist DIR] [--dry-run]
#   scripts/publish-release.sh npm    <version> [--dist DIR] [--dry-run]
#
# The channel follows the version:
#   x.y.z                       GitHub latest release, npm dist-tag `latest`
#   x.y.z-alpha.N/-beta.N/-rc.N GitHub prerelease (never latest), npm `next`
# Anything else is refused.
#
# `github` and `npm` verify the artifacts first (`verify`): the archive and
# the npm tarball match their .sha256 files, and both manifests name this
# version, this channel and the same archive. --dry-run verifies, then
# prints the publish command instead of running it.
#
# DIR (default dist) holds what release.yml's build job uploads:
#   nextrs-v<v>-linux-x86_64.tar.gz{,.sha256}, nextrs-v<v>-linux-x86_64.manifest.json
#   npm/nextrs-cli-<v>.tgz{,.sha256}, npm/nextrs-cli-<v>.npm.json
set -euo pipefail
cd "$(dirname "$0")/.."

die() { echo "publish-release: $*" >&2; exit 1; }

# Sets NPM_TAG and PRERELEASE for version $1, or refuses it.
classify() {
  if [[ $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    NPM_TAG=latest PRERELEASE=false
  elif [[ $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+-(alpha|beta|rc)\.[0-9]+$ ]]; then
    NPM_TAG=next PRERELEASE=true
  else
    die "not a release version: '$1' (x.y.z, or x.y.z-alpha.N, -beta.N, -rc.N)"
  fi
}

json() { node -e 'const v = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"))[process.argv[2]]; console.log(v === undefined ? "" : String(v))' "$1" "$2"; }

checksum_ok() { (cd "$(dirname "$1")" && sha256sum -c --status "$(basename "$1").sha256"); }

verify() {
  local f
  for f in "$ARCHIVE" "$ARCHIVE.sha256" "$MANIFEST" "$TARBALL" "$TARBALL.sha256" "$NPM_JSON"; do
    [[ -f $f ]] || die "missing $f"
  done
  checksum_ok "$ARCHIVE" || die "$(basename "$ARCHIVE") does not match its .sha256"
  checksum_ok "$TARBALL" || die "$(basename "$TARBALL") does not match its .sha256"

  [[ $(json "$MANIFEST" version) == "$V" ]] || die "$(basename "$MANIFEST"): version is $(json "$MANIFEST" version), not $V"
  [[ $(json "$MANIFEST" prerelease) == "$PRERELEASE" ]] || die "$(basename "$MANIFEST"): prerelease is $(json "$MANIFEST" prerelease), $V needs $PRERELEASE"
  [[ $(json "$MANIFEST" archive_sha256) == "$(cut -d' ' -f1 "$ARCHIVE.sha256")" ]] || die "$(basename "$MANIFEST"): archive_sha256 is not the archive's"

  [[ $(json "$NPM_JSON" version) == "$V" ]] || die "$(basename "$NPM_JSON"): version is $(json "$NPM_JSON" version), not $V"
  [[ $(json "$NPM_JSON" dist_tag) == "$NPM_TAG" ]] || die "$(basename "$NPM_JSON"): dist_tag is $(json "$NPM_JSON" dist_tag), $V needs $NPM_TAG"
  [[ $(json "$NPM_JSON" tarball_sha256) == "$(cut -d' ' -f1 "$TARBALL.sha256")" ]] || die "$(basename "$NPM_JSON"): tarball_sha256 is not the tarball's"
  [[ $(json "$NPM_JSON" archive_sha256) == "$(json "$MANIFEST" archive_sha256)" ]] || die "the npm tarball was not packed from this archive"
  echo "verified: v$V ($([[ $PRERELEASE == true ]] && echo prerelease || echo stable), npm $NPM_TAG)"
}

# Runs a command, or prints it with --dry-run.
run() {
  if [[ $DRY_RUN == 1 ]]; then printf 'dry-run:'; printf ' %q' "$@"; echo; else "$@"; fi
}

github() {
  local notes="release/v$V-release-notes.md" args
  args=(release create "v$V" --verify-tag --title "NextRS v$V")
  if [[ $PRERELEASE == true ]]; then args+=(--prerelease --latest=false); else args+=(--latest); fi
  if [[ -f $notes ]]; then args+=(--notes-file "$notes"); else args+=(--generate-notes); fi
  run gh "${args[@]}" "$ARCHIVE" "$ARCHIVE.sha256" "$MANIFEST"
  [[ $DRY_RUN == 1 ]] && return
  local got
  got=$(gh release view "v$V" --json isPrerelease --jq .isPrerelease)
  [[ $got == "$PRERELEASE" ]] || die "GitHub release v$V: isPrerelease is $got, expected $PRERELEASE"
  echo "GitHub release v$V: prerelease=$got"
}

npm_publish() {
  local path=$TARBALL
  # "./": without it npm reads "dist/npm/<file>" as a GitHub repository.
  [[ $path == /* || $path == ./* ]] || path="./$path"
  [[ $PRERELEASE == false || $NPM_TAG != latest ]] || die "refusing to publish prerelease $V as latest"
  run npm publish "$path" --tag "$NPM_TAG" --access public --provenance
  [[ $DRY_RUN == 1 ]] && return
  # The registry can take a few seconds to show the new tag.
  local got=""
  for _ in 1 2 3 4 5 6; do
    got=$(npm view @nextrs/cli "dist-tags.$NPM_TAG" 2>/dev/null || true)
    [[ $got == "$V" ]] && break
    sleep 10
  done
  npm view @nextrs/cli dist-tags
  [[ $got == "$V" ]] || die "npm dist-tag $NPM_TAG is '$got', expected $V"
}

cmd=${1:-}; V=${2:-}
[[ -n $cmd && -n $V ]] || die "usage: $0 channel|verify|github|npm <version> [--dist DIR] [--dry-run]"
shift 2
classify "$V"

DIST=dist DRY_RUN=0
while (($#)); do
  case $1 in
    --dist) [[ $# -ge 2 ]] || die "--dist needs a directory"; DIST=${2%/}; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    *) die "unknown option: $1" ;;
  esac
done
ARCHIVE="$DIST/nextrs-v$V-linux-x86_64.tar.gz"
MANIFEST="$DIST/nextrs-v$V-linux-x86_64.manifest.json"
TARBALL="$DIST/npm/nextrs-cli-$V.tgz"
NPM_JSON="$DIST/npm/nextrs-cli-$V.npm.json"

case $cmd in
  channel) echo "npm_tag=$NPM_TAG"; echo "prerelease=$PRERELEASE" ;;
  verify) verify ;;
  github) verify; github ;;
  npm) verify; npm_publish ;;
  *) die "unknown command: $cmd" ;;
esac
