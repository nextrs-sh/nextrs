// Summary of a run.mjs matrix (next start and NextRS at several isolate
// counts): per workload, medians over valid rounds and NextRS/next start
// ratios with 95% bootstrap intervals, closed loop (throughput, CPU and
// instructions per request, memory) and open loop (latency at an equal
// offered load); isolate scaling. Writes summary.json, summary.csv and
// summary.md next to rows.jsonl.
//
//   node benchmarks/phase3a/summarize.mjs DIR [--baseline next-start]
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { median, ratioCI } from "./lib.mjs";

const dir = process.argv[2];
const baseline = process.argv.includes("--baseline") ? process.argv[process.argv.indexOf("--baseline") + 1] : "next-start";
const rows = readFileSync(join(dir, "rows.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l)).filter((r) => !r.failed);
const env = JSON.parse(readFileSync(join(dir, "environment.json"), "utf8"));
const configs = env.parameters.configs;
const keys = [...new Set(rows.map((r) => `${r.group}/${r.workload}`))];

const metric = {
  rps: (r) => r.rps,
  cpu_ms_per_req: (r) => r.cpu_ms_per_req,
  instr_per_req: (r) => r.instructions_per_req,
  p50_ms: (r) => r.latency.p50_ms,
  p95_ms: (r) => r.latency.p95_ms,
  p99_ms: (r) => r.latency.p99_ms,
  ttfb_p50_ms: (r) => r.ttfb.p50_ms,
  rss_peak_mib: (r) => r.mem_peak.rss_mib,
  anon_end_mib: (r) => r.mem_end.anon_mib,
  rss_steady_mib: (r) => r.mem_steady.rss_mib,
  cold_p50_ms: (r) => r.cold?.p50_ms,
};
const CLOSED = ["rps", "cpu_ms_per_req", "instr_per_req", "p50_ms", "p95_ms", "p99_ms", "rss_peak_mib", "anon_end_mib", "rss_steady_mib", "cold_p50_ms"];
const OPEN = ["p50_ms", "p95_ms", "p99_ms", "ttfb_p50_ms", "cpu_ms_per_req"];

const summary = [];
for (const key of keys) {
  const [group, workload] = key.split("/");
  const entry = { group, workload, label: rows.find((r) => r.group === group && r.workload === workload)?.label, configs: {}, ratios: {} };
  for (const mode of ["closed", "open"]) {
    const names = mode === "closed" ? CLOSED : OPEN;
    for (const c of configs) {
      const rs = rows.filter((r) => r.group === group && r.workload === workload && r.config === c && r.mode === mode);
      const valid = rs.filter((r) => r.valid);
      const e = (entry.configs[c] ??= {});
      e[mode] = { rounds: valid.length, invalid: rs.length - valid.length, load_limited: valid.some((r) => r.load_limited), offered_rate: valid[0]?.offered_rate ?? null };
      for (const n of names) {
        const xs = valid.map(metric[n]).filter((x) => x != null);
        e[mode][n] = xs.length ? median(xs) : null;
        e[mode][`${n}_values`] = xs;
      }
    }
    for (const c of configs) {
      if (c === baseline) continue;
      const r = (entry.ratios[c] ??= {});
      r[mode] = {};
      for (const n of names) {
        const a = entry.configs[baseline]?.[mode]?.[`${n}_values`] ?? [];
        const b = entry.configs[c]?.[mode]?.[`${n}_values`] ?? [];
        if (a.length >= 2 && b.length >= 2) r[mode][n] = ratioCI(a, b);
      }
    }
  }
  // Isolate scaling: closed-loop throughput relative to 1 isolate.
  const iso = configs.filter((c) => /-i\d+$/.test(c));
  const one = iso.find((c) => c.endsWith("-i1"));
  if (one) {
    entry.scaling = Object.fromEntries(iso.map((c) => {
      const n = Number(c.split("-i").pop());
      const speedup = entry.configs[c]?.closed?.rps / entry.configs[one]?.closed?.rps;
      return [c, { isolates: n, speedup: +speedup.toFixed(3), efficiency: +(speedup / n).toFixed(3) }];
    }));
  }
  summary.push(entry);
}

writeFileSync(join(dir, "summary.json"), JSON.stringify({ environment: env, baseline, configs, workloads: summary }, null, 1));

// CSV: one line per workload x config x mode with the medians.
const csv = ["group,workload,config,mode,rounds,invalid,load_limited,offered_rate," + CLOSED.join(",")];
for (const e of summary) for (const c of configs) for (const mode of ["closed", "open"]) {
  const x = e.configs[c]?.[mode];
  if (!x) continue;
  csv.push([e.group, e.workload, c, mode, x.rounds, x.invalid, x.load_limited, x.offered_rate ?? "", ...CLOSED.map((n) => (x[n] == null ? "" : +x[n].toFixed(4)))].join(","));
}
writeFileSync(join(dir, "summary.csv"), csv.join("\n") + "\n");

// Markdown.
const f = (x, d = 1) => (x == null ? "–" : x >= 1000 ? Math.round(x).toLocaleString("en-US") : x.toFixed(d));
const rel = (r, better) => {
  if (!r) return "";
  const pct = (r.ratio - 1) * 100;
  const sig = r.significant ? (better === "higher" ? (r.ratio > 1 ? "✔" : "✘") : r.ratio < 1 ? "✔" : "✘") : "·";
  return ` (${r.ratio >= 1 ? "×" + r.ratio.toFixed(2) : "×" + r.ratio.toFixed(2)} ${sig})`;
};
const md = [`# ${dir}`, "", `Medians over valid rounds. Ratios NextRS / ${baseline} with ✔ better, ✘ worse, · within noise (95 % bootstrap interval). Throughput, CPU and memory: closed loop; latency: open loop at an equal offered rate. ⚑ load generator limited (req/s is a lower bound).`, ""];
const ns = configs.filter((c) => c !== baseline);
md.push(`| Workload | ${baseline} req/s | ${ns.map((c) => `${c} req/s`).join(" | ")} | CPU ms/req ${baseline} → ${ns[0]} | open-loop p95 ms ${baseline} → ${ns[0]} | anon RSS MiB ${baseline} → ${ns[0]} |`);
md.push(`|---|---:|${ns.map(() => "---:").join("|")}|---|---|---|`);
for (const e of summary) {
  const b = e.configs[baseline];
  const lim = (c) => (e.configs[c]?.closed?.load_limited ? " ⚑" : "");
  const bad = configs.reduce((a, c) => a + (e.configs[c]?.closed?.invalid ?? 0) + (e.configs[c]?.open?.invalid ?? 0), 0);
  md.push(`| ${e.group}/${e.workload}${bad ? ` ⚠ ${bad} invalid` : ""} | ${f(b?.closed?.rps, 0)}${lim(baseline)} | ${ns.map((c) => `${f(e.configs[c]?.closed?.rps, 0)}${lim(c)}${rel(e.ratios[c]?.closed?.rps, "higher")}`).join(" | ")} | ${f(b?.closed?.cpu_ms_per_req, 3)} → ${f(e.configs[ns[0]]?.closed?.cpu_ms_per_req, 3)}${rel(e.ratios[ns[0]]?.closed?.cpu_ms_per_req, "lower")} | ${f(b?.open?.p95_ms, 2)} → ${f(e.configs[ns[0]]?.open?.p95_ms, 2)}${rel(e.ratios[ns[0]]?.open?.p95_ms, "lower")} | ${f(b?.closed?.anon_end_mib, 0)} → ${f(e.configs[ns[0]]?.closed?.anon_end_mib, 0)}${rel(e.ratios[ns[0]]?.closed?.anon_end_mib, "lower")} |`);
}
if (summary[0]?.scaling) {
  md.push("", "## Isolate scaling (closed-loop throughput relative to 1 isolate; efficiency = speed-up / isolates)", "");
  const iso = Object.keys(summary[0].scaling);
  md.push(`| Workload | ${iso.map((c) => c).join(" | ")} |`, `|---|${iso.map(() => "---:").join("|")}|`);
  for (const e of summary) md.push(`| ${e.group}/${e.workload} | ${iso.map((c) => (e.scaling?.[c] ? `${e.scaling[c].speedup.toFixed(2)}× (${Math.round(e.scaling[c].efficiency * 100)} %)${e.configs[c]?.closed?.load_limited ? " ⚑" : ""}` : "–")).join(" | ")} |`);
}
writeFileSync(join(dir, "summary.md"), md.join("\n") + "\n");
console.log(md.join("\n"));
