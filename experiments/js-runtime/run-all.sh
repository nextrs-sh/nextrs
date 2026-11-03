#!/usr/bin/env bash
# Runs every engine experiment and stores one JSON line per run in results/.
# Prerequisites: fixtures/next-app built (`npx next build`), js inputs built
# (`cd js && npm ci && npm run build`).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --quiet
BIN=./target/release/js-experiment
PROJECT=../../fixtures/next-app
ENTRY=.next/server/app/dynamic/page.js
OUT=results/runs.jsonl
: > "$OUT"
for engine in v8 qjs; do
  for test in hello async module react rsc; do
    "$BIN" "$engine" "$test" 2>/dev/null >> "$OUT"
  done
  for level in 0 1 2 3 4; do
    "$BIN" "$engine" next "$PROJECT" "$ENTRY" "$level" 2>/dev/null >> "$OUT"
  done
done
node -e '
const lines = require("fs").readFileSync(process.argv[1], "utf8").trim().split("\n").map(JSON.parse);
for (const j of lines) {
  const r = j.result || {};
  const what = j.test === "next" ? `next L${r.level}` : j.test;
  const outcome = r.error ? `${r.error.name || "Error"}: ${(r.error.message || r.error).toString().split("\n")[0].slice(0, 90)}` : (r.ok ? "ok" : "FAILED");
  console.log(`${j.engine.padEnd(4)} ${what.padEnd(8)} init ${(j.initMs ?? 0).toFixed(1).padStart(6)} ms  total ${j.totalMs.toFixed(1).padStart(7)} ms  heap ${((j.engineHeapBytes || 0) / 1048576).toFixed(2).padStart(6)} MiB  rss ${(j.rssKiBAfter / 1024).toFixed(1).padStart(6)} MiB  ${j.test === "next" ? `modules ${String(r.modulesLoaded).padStart(3)}  ` : ""}${outcome}`);
}' "$OUT"
