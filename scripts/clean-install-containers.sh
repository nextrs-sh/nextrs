#!/usr/bin/env bash
# Runs the clean-install test in fresh distribution containers. Each
# container's only mounts are the bundle (read-only) and an empty results
# directory: the builder's filesystem, this checkout, the Rust toolchain and
# the Cargo registry are not visible inside.
#
#   scripts/clean-install-containers.sh --bundle DIR --results DIR \
#     [--engine docker|podman] [--image ubuntu:22.04 ...]
#
# DIR comes from scripts/clean-install-bundle.sh. Default images: Ubuntu
# 22.04, Ubuntu 24.04, Debian 12 slim. The container runs
# scripts/clean-install-entry.sh as root (to install curl and create the
# unprivileged user that runs the server). Exits non-zero if any image
# fails.
set -uo pipefail
BUNDLE="" RESULTS="" ENGINE="${CONTAINER_ENGINE:-docker}"
IMAGES=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle) BUNDLE="$2"; shift 2 ;;
    --results) RESULTS="$2"; shift 2 ;;
    --engine) ENGINE="$2"; shift 2 ;;
    --image) IMAGES+=("$2"); shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -d $BUNDLE/apps && -n $RESULTS ]] || { echo "usage: $0 --bundle DIR --results DIR [--engine docker|podman] [--image IMG ...]" >&2; exit 2; }
[[ ${#IMAGES[@]} -gt 0 ]] || IMAGES=(ubuntu:22.04 ubuntu:24.04 debian:12-slim)
BUNDLE=$(cd "$BUNDLE" && pwd)
mkdir -p "$RESULTS"
RESULTS=$(cd "$RESULTS" && pwd)
status=0
for image in "${IMAGES[@]}"; do
  out="$RESULTS/$(echo "$image" | tr ':/' '--')"
  mkdir -p "$out"
  chmod 0777 "$out"
  echo "=== $image"
  # --pull=always: the current published image, never one modified locally.
  # label=disable: on SELinux hosts the bind mounts stay unlabelled (no relabelling of the bundle).
  $ENGINE run --rm --pull=always --security-opt label=disable --tmpfs /tmp:rw,exec,size=1g \
    -v "$BUNDLE:/bundle:ro" -v "$out:/results:rw" \
    "docker.io/library/$image" bash /bundle/scripts/clean-install-entry.sh
  rc=$?
  echo "=== $image: exit $rc"
  [[ $rc == 0 ]] || status=1
done
exit $status
