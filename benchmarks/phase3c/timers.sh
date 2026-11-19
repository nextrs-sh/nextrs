#!/usr/bin/env bash
# Phase 3C timer latency: the same probe
# (crates/nextrs-runtime/tests/fixtures/handlers/dist/timer-probe.js) under
# Node.js and inside a NextRS isolate, alternated, N runs each.
#
#   benchmarks/phase3c/timers.sh [--runs 5] [--out benchmarks/results/phase3c/timers] [--cpus 0-2]
#
# Writes <out>/node-<i>.json and <out>/nextrs-<i>.json (one probe report
# each) and <out>/summary.txt. Both run pinned to the same CPUs.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
runs=5 out="$root/benchmarks/results/phase3c/timers" cpus="0-2"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --runs) runs="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --cpus) cpus="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
mkdir -p "$out"
probe="$root/crates/nextrs-runtime/tests/fixtures/handlers/dist/timer-probe.js"
# Build the measurement test once (release: the same optimisation as the
# server binary).
(cd "$root" && cargo test -p nextrs-runtime --release --test timers --no-run 2>&1 | tail -1)
for i in $(seq 1 "$runs"); do
  taskset -c "$cpus" node -e "require('$probe').run().then(r => console.log(JSON.stringify(r)))" > "$out/node-$i.json"
  (cd "$root" && taskset -c "$cpus" cargo test -q -p nextrs-runtime --release --test timers -- --ignored --nocapture timer_latency_report 2>/dev/null) \
    | sed -n 's/^TIMER-REPORT //p' > "$out/nextrs-$i.json"
done
node -e '
const fs = require("fs"), path = require("path");
const dir = process.argv[1], runs = Number(process.argv[2]);
const load = (k) => Array.from({ length: runs }, (_, i) => JSON.parse(fs.readFileSync(path.join(dir, `${k}-${i + 1}.json`), "utf8")));
const node = load("node"), nextrs = load("nextrs");
const med = (xs) => { const s = [...xs].sort((a, b) => a - b); return s[Math.floor(s.length / 2)]; };
const lines = ["case                     stat   node(median of runs)  nextrs(median of runs)"];
for (const k of Object.keys(node[0])) {
  if (typeof node[0][k].p50 !== "number") continue;
  for (const st of ["p50", "p90", "p99", "max"]) {
    lines.push(`${k.padEnd(24)} ${st.padEnd(5)} ${med(node.map((r) => r[k][st])).toFixed(3).padStart(10)} ms ${med(nextrs.map((r) => r[k][st])).toFixed(3).padStart(14)} ms`);
  }
}
lines.push(`cancellation node ${JSON.stringify(node.map((r) => r.cancellation))}`);
lines.push(`cancellation nextrs ${JSON.stringify(nextrs.map((r) => r.cancellation))}`);
fs.writeFileSync(path.join(dir, "summary.txt"), lines.join("\n") + "\n");
console.log(lines.join("\n"));
' "$out" "$runs"
