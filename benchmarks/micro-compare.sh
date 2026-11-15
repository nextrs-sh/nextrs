#!/usr/bin/env bash
# Runs the Criterion microbenchmarks on a base commit and on the working
# tree, one after the other on this machine, and prints Criterion's
# comparison (change and p-value per benchmark):
#
#   benchmarks/micro-compare.sh [BASE]     (default: merge-base with origin/main)
#
# The base is checked out in a temporary worktree sharing this target
# directory, so both builds use the same toolchain and the comparison uses
# Criterion's saved baseline. If the base predates a benchmark, that
# benchmark is only measured on the working tree.
#
# Wall-clock comparisons are only meaningful on one machine in one
# session; on shared CI runners they indicate, they do not gate.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
base="${1:-$(git -C "$root" merge-base HEAD origin/main)}"
target="${CARGO_TARGET_DIR:-$root/target}"
benches=("nextrs-server:request_path" "nextrs-runtime:dispatch")
args=(-- --warm-up-time 2 --measurement-time 5 --noplot)

worktree="$(mktemp -d)"
trap 'git -C "$root" worktree remove --force "$worktree" >/dev/null 2>&1 || true' EXIT
git -C "$root" worktree add --detach "$worktree" "$base" >/dev/null

echo "== base $(git -C "$root" rev-parse --short "$base")"
for b in "${benches[@]}"; do
  crate="${b%%:*}" bench="${b#*:}"
  if [[ -f "$worktree/crates/$crate/benches/$bench.rs" ]]; then
    (cd "$worktree" && CARGO_TARGET_DIR="$target" cargo bench -q -p "$crate" --bench "$bench" "${args[@]}" --save-baseline base >/dev/null)
  else
    echo "   $crate/$bench: not in the base commit"
  fi
done

echo "== working tree $(git -C "$root" rev-parse --short HEAD)$(git -C "$root" diff --quiet || echo '+dirty')"
for b in "${benches[@]}"; do
  crate="${b%%:*}" bench="${b#*:}"
  # --baseline-lenient: a benchmark without a saved base is just measured.
  (cd "$root" && CARGO_TARGET_DIR="$target" cargo bench -q -p "$crate" --bench "$bench" "${args[@]}" --baseline-lenient base)
done
