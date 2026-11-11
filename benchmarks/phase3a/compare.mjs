// Compares two configurations of a run.mjs result directory, workload by
// workload: medians over the rounds, and the ratio B/A of the medians with a
// 95% bootstrap interval. A difference counts only when the interval
// excludes 1 ("significant"); otherwise it is reported as within noise.
//
//   node benchmarks/phase3a/compare.mjs DIR --a CONFIG --b CONFIG [--mode closed|open] [--md OUT.md] [--json OUT.json]
//
// Invalid rows (failed oracles, preflight or ledger checks) are excluded and
// counted; a workload with any invalid row is flagged.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { median, ratioCI } from "./lib.mjs";

const argv = process.argv.slice(2);
const dir = argv[0];
const opt = {};
for (let i = 1; i < argv.length; i++) if (argv[i].startsWith("--")) opt[argv[i].slice(2)] = argv[++i];
const mode = opt.mode ?? "closed";
const rows = readFileSync(join(dir, "rows.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l)).filter((r) => r.mode === mode && !r.failed);

const METRICS = [
  ["rps", (r) => r.rps, "higher"],
  ["cpu_ms_per_req", (r) => r.cpu_ms_per_req, "lower"],
  ["instr_per_req", (r) => r.instructions_per_req, "lower"],
  ["p50_ms", (r) => r.latency.p50_ms, "lower"],
  ["p95_ms", (r) => r.latency.p95_ms, "lower"],
  ["p99_ms", (r) => r.latency.p99_ms, "lower"],
  ["rss_peak_mib", (r) => r.mem_peak.rss_mib, "lower"],
  ["rss_steady_mib", (r) => r.mem_steady.rss_mib, "lower"],
];

const keys = [...new Set(rows.map((r) => `${r.group}/${r.workload}`))];
const out = [];
for (const key of keys) {
  const [group, workload] = key.split("/");
  const of = (c) => rows.filter((r) => r.group === group && r.workload === workload && r.config === c);
  const A = of(opt.a), B = of(opt.b);
  const invalid = [...A, ...B].filter((r) => !r.valid).length;
  const va = A.filter((r) => r.valid), vb = B.filter((r) => r.valid);
  const entry = { group, workload, label: (A[0] ?? B[0])?.label, n: [va.length, vb.length], invalid, loadLimited: [...va, ...vb].some((r) => r.load_limited), metrics: {} };
  for (const [name, get, better] of METRICS) {
    const xa = va.map(get).filter((x) => x != null), xb = vb.map(get).filter((x) => x != null);
    if (xa.length < 2 || xb.length < 2) continue;
    const ci = ratioCI(xa, xb);
    const improved = ci.significant && (better === "higher" ? ci.ratio > 1 : ci.ratio < 1);
    entry.metrics[name] = { a: median(xa), b: median(xb), ...ci, verdict: !ci.significant ? "noise" : improved ? "better" : "worse", spread_a: [Math.min(...xa), Math.max(...xa)], spread_b: [Math.min(...xb), Math.max(...xb)] };
  }
  out.push(entry);
}

const f = (x, d = 2) => (x == null ? "–" : Math.abs(x) >= 1000 ? x.toFixed(0) : x.toFixed(d));
const pctChange = (m) => `${m.ratio >= 1 ? "+" : ""}${((m.ratio - 1) * 100).toFixed(1)}% [${((m.ci95[0] - 1) * 100).toFixed(1)}, ${((m.ci95[1] - 1) * 100).toFixed(1)}]`;
const lines = [`# ${opt.b} vs ${opt.a} (${mode} loop) — ${dir}`, "", "Median of valid rounds; change = B/A − 1 with 95% bootstrap interval; ✔ better, ✘ worse, · within noise.", ""];
lines.push("| Workload | n | req/s A → B | CPU ms/req A → B | instr/req A → B | p95 ms A → B | p99 ms A → B | peak RSS MiB A → B |");
lines.push("|---|---|---|---|---|---|---|---|");
const mark = (m) => (m ? (m.verdict === "better" ? "✔" : m.verdict === "worse" ? "✘" : "·") : "");
const cell = (m, d) => (m ? `${f(m.a, d)} → ${f(m.b, d)} ${mark(m)}<br>${pctChange(m)}` : "–");
for (const e of out) {
  const m = e.metrics;
  lines.push(`| ${e.group}/${e.workload}${e.invalid ? ` ⚠ ${e.invalid} invalid` : ""}${e.loadLimited ? " (loadgen-limited)" : ""} | ${e.n.join("/")} | ${cell(m.rps, 0)} | ${cell(m.cpu_ms_per_req, 3)} | ${m.instr_per_req ? `${f(m.instr_per_req.a / 1e6)}M → ${f(m.instr_per_req.b / 1e6)}M ${mark(m.instr_per_req)}<br>${pctChange(m.instr_per_req)}` : "–"} | ${cell(m.p95_ms)} | ${cell(m.p99_ms)} | ${cell(m.rss_peak_mib, 0)} |`);
}
const md = lines.join("\n") + "\n";
console.log(md);
if (opt.md) writeFileSync(opt.md, md);
if (opt.json) writeFileSync(opt.json, JSON.stringify(out, null, 2));
