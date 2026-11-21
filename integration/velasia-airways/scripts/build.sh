#!/usr/bin/env bash
# Build both images (Velasia API + NextRS storefront) without starting them.
#   integration/velasia-airways/scripts/build.sh [--no-cache]
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
src=${VELASIA_SRC:-$here/../../../velasia-airways}
src=$(cd "$here" && cd "$src" && pwd)
[ -f "$src/apps/web/next.config.ts" ] || { echo "Velasia Airways not found at $src (set VELASIA_SRC)" >&2; exit 1; }
[ -f "$here/package-lock.json" ] || { echo "missing package-lock.json" >&2; exit 1; }
grep -q '"@nextrs/cli": "0.1.0-rc.1"' "$here/package.json" || { echo "@nextrs/cli must be pinned to exactly 0.1.0-rc.1" >&2; exit 1; }
echo "Velasia source: $src ($(git -C "$src" rev-parse --short HEAD 2>/dev/null || echo 'not a git checkout'))"
docker compose -f "$here/compose.yaml" build --progress=plain "$@"
docker image ls --format '{{.Repository}}:{{.Tag}}\t{{.ID}}\t{{.Size}}' | grep '^nextrs-velasia-'
