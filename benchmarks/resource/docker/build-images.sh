#!/usr/bin/env bash
# Builds the resource benchmarks' images with ONE `next build` per application, then checks
# that every runtime image carries that build (run.mjs verify-build). Changes the host only by
# adding Docker images (and BuildKit cache); downloads base images and npm packages.
#
#   OUT=/abs/results/linux-YYYYMMDD VELASIA_SRC=/abs/velasia-d04127a \
#     benchmarks/resource/docker/build-images.sh [--dry-run] velasia|fault|all
#
# velasia  velasia-web-build:$TAG  Velasia's own `build` stage (Dockerfile.node --target build)
#          velasia-node:$TAG       Dockerfile.velasia-node over it
#          velasia-nextrs:$TAG     Dockerfile.velasia-nextrs over it (NextRS 0.1.0-rc.2 from npm)
#          velasia-api:$TAG        Velasia's docker/Dockerfile.api, unchanged
# fault    nextrs-fault-build:$TAG, nextrs-fault:node, nextrs-fault-base:$TAG (fault-app)
#
# Environment: OUT (absolute, required), VELASIA_SRC (absolute; a clean checkout of
# VELASIA_COMMIT, default d04127a), TAG (default s1), VELASIA_TODAY and
# VELASIA_DEPLOYMENT_ID (optional build arguments, recorded). --dry-run prints every command
# and runs only the read-only checks (git rev-parse/status of VELASIA_SRC, docker context).
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
RESOURCE=$(cd "$HERE/.." && pwd)
NODE=${NODE:-node}
DRY=0
if [ "${1:-}" = "--dry-run" ]; then DRY=1; shift; fi
WHAT=${1:-}
case "$WHAT" in velasia | fault | all) ;; *) echo "usage: $0 [--dry-run] velasia|fault|all" >&2; exit 2 ;; esac

: "${OUT:?set OUT to an absolute results directory}"
case "$OUT" in /*) ;; *) echo "OUT must be absolute (got $OUT)" >&2; exit 2 ;; esac
TAG=${TAG:-s1}
VELASIA_COMMIT=${VELASIA_COMMIT:-d04127a}

run() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
  if [ "$DRY" = 0 ]; then "$@"; fi
}
# A real build stops at the first failure; a dry run reports every one and exits 1 at the end.
fails=0
fail() { echo "FAIL: $*" >&2; fails=$((fails + 1)); [ "$DRY" = 1 ] || exit 1; }

ctx=$(docker context show 2>/dev/null || echo unavailable)
case "$ctx" in
  desktop-linux) fail "docker context is desktop-linux (Docker Desktop's VM): use the native engine" ;;
  unavailable) fail "docker is not available" ;;
esac

run mkdir -p "$OUT/build"

build_args=()
[ -z "${VELASIA_TODAY:-}" ] || build_args+=(--build-arg "VELASIA_TODAY=$VELASIA_TODAY")
[ -z "${VELASIA_DEPLOYMENT_ID:-}" ] || build_args+=(--build-arg "VELASIA_DEPLOYMENT_ID=$VELASIA_DEPLOYMENT_ID")

velasia() {
  : "${VELASIA_SRC:?set VELASIA_SRC to an absolute path of a Velasia checkout at $VELASIA_COMMIT}"
  case "$VELASIA_SRC" in /*) ;; *) fail "VELASIA_SRC must be absolute (got $VELASIA_SRC)" ;; esac
  if [ -d "$VELASIA_SRC" ]; then
    head=$(git -C "$VELASIA_SRC" rev-parse HEAD)
    case "$head" in "$VELASIA_COMMIT"*) ;; *) fail "VELASIA_SRC is at $head, not $VELASIA_COMMIT" ;; esac
    [ -z "$(git --no-optional-locks -C "$VELASIA_SRC" status --porcelain)" ] || fail "VELASIA_SRC has local changes"
    # Our runtime stages must still be Velasia's (byte for byte from the runtime FROM line on).
    diff <(sed -n '/^FROM ${NODE_IMAGE} AS runtime/,$p' "$VELASIA_SRC/Dockerfile.node") \
         <(sed -n '/^FROM ${NODE_IMAGE} AS runtime/,$p' "$HERE/Dockerfile.velasia-node") >/dev/null \
      || fail "Dockerfile.velasia-node runtime stage differs from $VELASIA_SRC/Dockerfile.node"
    diff <(sed -n '/^FROM ${NODE_IMAGE} AS nextrs-install/,$p' "$VELASIA_SRC/Dockerfile.nextrs") \
         <(sed -n '/^FROM ${NODE_IMAGE} AS nextrs-install/,$p' "$HERE/Dockerfile.velasia-nextrs") >/dev/null \
      || fail "Dockerfile.velasia-nextrs stages differ from $VELASIA_SRC/Dockerfile.nextrs"
  else
    fail "VELASIA_SRC $VELASIA_SRC does not exist"
  fi
  run docker build --target build -f "$VELASIA_SRC/Dockerfile.node" "${build_args[@]}" \
    -t "velasia-web-build:$TAG" "$VELASIA_SRC"
  run docker build -f "$HERE/Dockerfile.velasia-node" --build-arg "BUILD_IMAGE=velasia-web-build:$TAG" \
    -t "velasia-node:$TAG" "$VELASIA_SRC"
  run docker build -f "$HERE/Dockerfile.velasia-nextrs" --build-arg "BUILD_IMAGE=velasia-web-build:$TAG" \
    -t "velasia-nextrs:$TAG" "$VELASIA_SRC"
  run docker build -f "$VELASIA_SRC/docker/Dockerfile.api" -t "velasia-api:$TAG" "$VELASIA_SRC"
  run "$NODE" "$RESOURCE/run.mjs" verify-build --out "$OUT/build" --name velasia \
    --images "velasia-web-build:$TAG=/repo/apps/web/.next/standalone,velasia-node:$TAG=/app,velasia-nextrs:$TAG=/app" \
    --app-dir apps/web
}

fault() {
  run docker build -f "$HERE/Dockerfile.fault-build" -t "nextrs-fault-build:$TAG" "$RESOURCE/fault-app"
  run docker build -f "$HERE/Dockerfile.fault-runtime" --target node --build-arg "BUILD_IMAGE=nextrs-fault-build:$TAG" \
    -t nextrs-fault:node "$RESOURCE/fault-app"
  run docker build -f "$HERE/Dockerfile.fault-runtime" --target nextrs --build-arg "BUILD_IMAGE=nextrs-fault-build:$TAG" \
    -t "nextrs-fault-base:$TAG" "$RESOURCE/fault-app"
  run "$NODE" "$RESOURCE/run.mjs" verify-build --out "$OUT/build" --name fault \
    --images "nextrs-fault-build:$TAG=/src/.next/standalone,nextrs-fault:node=/app,nextrs-fault-base:$TAG=/app" \
    --app-dir .
}

record() {
  local f="$OUT/images.txt"
  [ "$DRY" = 1 ] && { echo "+ (record image IDs and pinned base images in $f)"; return; }
  {
    date -Is
    echo "VELASIA_COMMIT=$VELASIA_COMMIT TAG=$TAG VELASIA_TODAY=${VELASIA_TODAY:-} VELASIA_DEPLOYMENT_ID=${VELASIA_DEPLOYMENT_ID:-}"
    grep -h '^ARG \(NODE\|RUNTIME\)_IMAGE=' "$HERE"/Dockerfile.* | sort -u
    for i in velasia-web-build:$TAG velasia-node:$TAG velasia-nextrs:$TAG velasia-api:$TAG \
             nextrs-fault-build:$TAG nextrs-fault:node nextrs-fault-base:$TAG; do
      id=$(docker image inspect --format '{{.Id}}' "$i" 2>/dev/null || echo absent)
      echo "$i $id"
    done
  } > "$f"
  cat "$f"
}

case "$WHAT" in
  velasia) velasia ;;
  fault) fault ;;
  all) velasia; fault ;;
esac
record
if [ "$fails" != 0 ]; then
  echo "$fails check(s) failed" >&2
  exit 1
fi
