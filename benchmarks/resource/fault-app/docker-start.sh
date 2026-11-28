#!/bin/sh
# The fault app on NextRS (benchmarks/resource/docker/Dockerfile.fault-runtime,
# target nextrs). The same environment contract as Velasia's start script
# for what the harness sets: PORT, NEXTRS_ISOLATES, NEXTRS_METRICS_ADDR,
# NEXTRS_EXTRA_ARGS (word-split, never quoted or expanded).
set -eu
set -- start /app \
  --host 0.0.0.0 \
  --port "${PORT:-8080}" \
  --js-runtime embedded-v8 \
  --isolates "${NEXTRS_ISOLATES:-1}"
metrics="${NEXTRS_METRICS_ADDR:-127.0.0.1:9100}"
[ "$metrics" = off ] || set -- "$@" --metrics-addr "$metrics"
# shellcheck disable=SC2086 # deliberate word splitting of extra flags
[ -z "${NEXTRS_EXTRA_ARGS:-}" ] || set -- "$@" $NEXTRS_EXTRA_ARGS
printf '{"level":"info","service":"nextrs-fault-app","msg":"exec nextrs","args":"%s"}\n' "$*" >&2
exec /usr/local/bin/nextrs "$@"
