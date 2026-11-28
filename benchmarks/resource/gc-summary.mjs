// Summarizes V8 `--trace-gc` output saved by `run.mjs profile --save-logs`
// (variants with `--v8-flags --trace-gc`): per run and collection type,
// count, total and longest pause, p99 pause; and pause time as a share of
// the isolate's lifetime in the log.
//
//   node benchmarks/resource/gc-summary.mjs DIR
//
// A trace-gc line looks like
//   [1:0x5f..]    52141 ms: Mark-Compact 61.2 (70.9) -> 50.3 (69.1) MB, pooled: 0 MB, 21.80 / 0.00 ms (...) ...
// The pause is the first figure after the sizes (main-thread time).
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const [dir] = process.argv.slice(2);
const LINE = /(\d+) ms: ((?:Incremental |Minor )?[A-Za-z-]+(?: \([a-z ]+\))?) ([\d.]+) \(([\d.]+)\) -> ([\d.]+) \(([\d.]+)\) MB,(?: pooled: [\d.]+ MB,)? ([\d.]+) \/ ([\d.]+) ms(?: \(\+ ([\d.]+) ms in \d+ steps)?/;
const pct = (v, p) => {
  const s = [...v].sort((a, b) => a - b);
  return s.length ? s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))] : null;
};
console.log("| Run | Type | Count | Total pause ms | Incremental steps ms | Mean pause ms | p99 ms | Max ms | Main-thread GC share of log span |");
console.log("|---|---|---:|---:|---:|---:|---:|---:|---:|");
for (const f of readdirSync(dir).filter((f) => f.endsWith(".log")).sort()) {
  const events = [];
  for (const line of readFileSync(join(dir, f), "utf8").split("\n")) {
    const m = line.match(LINE);
    // Incremental marking steps also run on the main thread: counted in
    // the collection's main-thread time, but not in its final pause.
    if (m) events.push({ t: Number(m[1]), type: m[2].includes("Mark-Compact") ? "Mark-Compact" : m[2].replace(/ \(.*\)$/, ""), pause: Number(m[7]), steps: Number(m[9] ?? 0) });
  }
  if (!events.length) continue;
  const span = Math.max(...events.map((e) => e.t)) - Math.min(...events.map((e) => e.t));
  const types = [...new Set(events.map((e) => e.type))];
  for (const type of [...types, "all"]) {
    const es = events.filter((e) => type === "all" || e.type === type);
    const ps = es.map((e) => e.pause);
    const total = ps.reduce((a, b) => a + b, 0);
    const steps = es.reduce((a, e) => a + e.steps, 0);
    console.log(`| ${f.replace(/\.log$/, "")} | ${type} | ${ps.length} | ${total.toFixed(0)} | ${steps.toFixed(0)} | ${(total / ps.length).toFixed(2)} | ${pct(ps, 99)?.toFixed(1)} | ${Math.max(...ps).toFixed(1)} | ${type === "all" && span ? `${((100 * (total + steps)) / span).toFixed(2)} %` : ""} |`);
  }
}
