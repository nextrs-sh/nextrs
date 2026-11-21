// Markdown tables from benchmarks/rc2/compression.mjs results: the median
// over rounds of each metric, per workload and configuration.
//
//   node benchmarks/rc2/summarize.mjs benchmarks/results/rc2/matrix > summary.md
import { readFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
const { results, chunk } = JSON.parse(readFileSync(join(dir, "results.json"), "utf8"));
const median = (xs) => {
  const s = xs.filter((x) => x != null && !Number.isNaN(x)).sort((a, b) => a - b);
  if (!s.length) return null;
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};
const fmt = (x, d = 0) => (x == null ? "–" : Number(x).toFixed(d));
const order = ["base-identity", "cand-identity", "next-identity", "base-gzip", "cand-gzip", "cand-nocache-gzip", "cand-tiny-gzip", "next-gzip"];
const workloads = [...new Set(results.map((r) => r.workload))];

console.log(`Large static response: \`${chunk.path}\` (${chunk.size} bytes).\n`);
console.log("Medians over rounds. `base`: NextRS before this branch; `cand`: this branch; `next`: next start. `-gzip` requests send `Accept-Encoding: gzip`, `-identity` ones `identity`. CPU: server process tree, over the measured window (4 CPUs available). Bytes: body bytes per response as received.\n");
for (const w of workloads) {
  console.log(`### ${w}\n`);
  console.log("| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |");
  console.log("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|");
  for (const c of order) {
    const rows = results.filter((r) => r.workload === w && r.config === c);
    if (!rows.length) continue;
    const m = (f) => median(rows.map(f));
    const bytes = m((r) => (r.loadgen.completed ? r.loadgen.bytes_received / r.loadgen.completed : null));
    console.log(
      `| ${c} | ${fmt(m((r) => r.rps))} | ${fmt(m((r) => r.loadgen.latency.p50_ms), 2)} | ${fmt(m((r) => r.loadgen.latency.p95_ms), 2)} | ${fmt(m((r) => r.loadgen.latency.p99_ms), 2)} | ${fmt(m((r) => r.loadgen.ttfb.p50_ms), 2)} | ${fmt(m((r) => r.loadgen.ttfb.p99_ms), 2)} | ${fmt(m((r) => r.cpu_util), 2)} | ${fmt(m((r) => r.cpu_ms_per_req * 1000), 1)} | ${fmt(bytes)} | ${[...new Set(rows.map((r) => r.oracle.encoding))].join(",")} | ${fmt(m((r) => r.peak_rss_mib), 1)} | ${rows.reduce((a, r) => a + r.failures, 0)} |`,
    );
  }
  console.log();
}
