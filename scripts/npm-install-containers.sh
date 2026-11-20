#!/usr/bin/env bash
# npm and pnpm installation of the @nextrs/cli tarball in fresh
# distribution containers, as a developer would do it, with no Rust anywhere.
#
#   scripts/npm-install-containers.sh --bundle DIR --results DIR \
#     [--engine docker|podman] [--image ubuntu:22.04 ...] [--no-unsupported | --only-unsupported]
#
# DIR comes from scripts/npm-install-bundle.sh. For each supported image
# (default Ubuntu 22.04, Ubuntu 24.04, Debian 12 slim):
#   1. `deps`, with the network: npm ci and pnpm install of the application's
#      own dependencies (Next.js, React), into a volume;
#   2. `install`, with --network none: install the NextRS tarball with npm and
#      pnpm, `next build`, serve and probe, signals and exit codes, lifecycle
#      scripts disabled, one-off execution, and failure cases;
#   3. `missing-lib` (Debian 12 only), with --network none: libgcc_s removed.
# Then, unless --no-unsupported: `old-glibc` on Ubuntu 20.04 (glibc 2.31)
# and `musl` on node:22-alpine, both with the package installed offline.
# The volume is removed afterwards. Exits non-zero if any phase fails.
#
# ENGINE may be a command with arguments (for example
# "flatpak-spawn --host podman" inside a Flatpak sandbox).
set -uo pipefail
BUNDLE="" RESULTS="" ENGINE="${CONTAINER_ENGINE:-docker}" UNSUPPORTED=1 SUPPORTED=1
IMAGES=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle) BUNDLE="$2"; shift 2 ;;
    --results) RESULTS="$2"; shift 2 ;;
    --engine) ENGINE="$2"; shift 2 ;;
    --image) IMAGES+=("$2"); shift 2 ;;
    --no-unsupported) UNSUPPORTED=0; shift ;;
    --only-unsupported) SUPPORTED=0; shift ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -d $BUNDLE/pkg && -n $RESULTS ]] || { echo "usage: $0 --bundle DIR --results DIR [--engine docker|podman] [--image IMG ...] [--no-unsupported]" >&2; exit 2; }
[[ ${#IMAGES[@]} -gt 0 ]] || IMAGES=(ubuntu:22.04 ubuntu:24.04 debian:12-slim)
BUNDLE=$(cd "$BUNDLE" && pwd)
mkdir -p "$RESULTS"
RESULTS=$(cd "$RESULTS" && pwd)
chmod 0777 "$RESULTS"
status=0
# run IMAGE VOLUME NETWORK MODE [shell prefix]
run() {
  local image=$1 vol=$2 net=$3 mode=$4 pre=${5:-}
  echo "=== $image: $mode (network: $net)"
  # --pull=missing: published images only (docker.io/library), never built locally.
  # label=disable: on SELinux hosts the bind mounts are not relabelled.
  local netopt=()
  [[ $net == default ]] || netopt=(--network "$net")
  $ENGINE run --rm --pull=missing --security-opt label=disable "${netopt[@]}" --tmpfs /tmp:rw,exec,size=2g \
    -v "$BUNDLE:/bundle:ro" -v "$RESULTS:/results:rw" ${vol:+-v "$vol:/work"} \
    "docker.io/library/$image" sh -c "${pre}bash /bundle/scripts/npm-install-entry.sh $mode"
  local rc=$?
  echo "=== $image: $mode exit $rc"
  [[ $rc == 0 ]] || status=1
}
[[ $SUPPORTED == 1 ]] || IMAGES=()
for image in "${IMAGES[@]}"; do
  vol="nextrs-npm-work-$(echo "$image" | tr ':/.' '---')"
  $ENGINE volume rm -f "$vol" > /dev/null 2>&1
  $ENGINE volume create "$vol" > /dev/null
  run "$image" "$vol" default deps
  run "$image" "$vol" none install
  [[ $image == debian:12-slim ]] && run "$image" "$vol" none missing-lib
  $ENGINE volume rm -f "$vol" > /dev/null
done
if [[ $UNSUPPORTED == 1 ]]; then
  run ubuntu:20.04 "" none old-glibc
  # Alpine has no bash: install it first (the only use of the network); the
  # tarball itself is installed with --offline.
  run node:22-alpine "" default musl "apk add --no-cache bash > /dev/null && "
fi
exit $status
