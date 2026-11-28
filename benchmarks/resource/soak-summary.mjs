// Summarizes `run.mjs profile --workload soak` runs: per 6-minute block,
// container and anonymous memory at the end of the idle gap and at their
// peak, V8 heap and allocator figures, failures; then the trend over the
// warmed-up blocks (least-squares slope, MiB per hour), excluding the
// first `--warmup` blocks.
//
//   node benchmarks/resource/soak-summary.mjs DIR [--warmup 2]
//
// A slope near zero over a short soak is not proof of no leak: a leak
// slower than the measurement's noise needs a longer soak (RE-8: 24 h).
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const [dir, ...rest] = process.argv.slice(2);
const warmup = Number(rest[rest.indexOf("--warmup") + 1] ?? 2) || 2;
const MiB = 2 ** 20;
const slope = (pts) => {
  const n = pts.length;
  if (n < 3) return null;
  const mx = pts.reduce((a, p) => a + p[0], 0) / n;
  const my = pts.reduce((a, p) => a + p[1], 0) / n;
  const sxx = pts.reduce((a, p) => a + (p[0] - mx) ** 2, 0);
  return sxx ? pts.reduce((a, p) => a + (p[0] - mx) * (p[1] - my), 0) / sxx : null;
};

for (const f of readdirSync(dir).filter((f) => f.endsWith(".json") && !f.startsWith("meta") && !f.startsWith("summary"))) {
  const run = JSON.parse(readFileSync(join(dir, f), "utf8"));
  if (run.workload !== "soak") continue;
  const samples = readFileSync(join(dir, f.replace(/\.json$/, ".samples.jsonl")), "utf8").trim().split("\n").filter(Boolean).map((l) => JSON.parse(l));
  const t0 = run.marks.idle;
  const blocks = new Map();
  for (const p of run.phases) {
    const m = p.name.match(/^soak(\d+)-(\w+)$/);
    if (!m) continue;
    const b = blocks.get(m[1]) ?? { n: Number(m[1]), failed: 0, ok: 0 };
    if (m[2] === "mixed") b.start = p.started;
    if (m[2] === "idle") {
      b.end = p.ended;
      b.idleCurrent = p.after?.["memory.current"] / MiB;
      const ex = p.metricsAfter?.executors ?? [];
      b.heapTotal = ex.reduce((a, e) => a + e.heap_total_bytes, 0) / MiB;
      b.heapAfterGc = ex.reduce((a, e) => a + (e.heap_used_after_gc_bytes ?? 0), 0) / MiB;
      b.mallocInUse = p.metricsAfter?.memory?.malloc ? (p.metricsAfter.memory.malloc.in_use_bytes + p.metricsAfter.memory.malloc.mmap_bytes) / MiB : null;
      b.restarts = ex.reduce((a, e) => a + (e.restarts ?? 0), 0);
    }
    b.failed += p.failed ?? 0;
    b.ok += p.result?.ok ?? 0;
    blocks.set(m[1], b);
  }
  const rows = [...blocks.values()].sort((a, b) => a.n - b.n);
  for (const b of rows) {
    const w = samples.filter((s) => s.t >= b.start && s.t <= b.end);
    b.peak = w.length ? Math.max(...w.map((s) => s.memory_current)) / MiB : null;
    b.peakAnon = w.length ? Math.max(...w.map((s) => s.stat.anon)) / MiB : null;
    const last = w.at(-1);
    b.idleAnon = last ? last.stat.anon / MiB : null;
    b.hour = (b.end - t0) / 3.6e6;
  }
  const warm = rows.filter((b) => b.n > warmup);
  const trend = (k) => slope(warm.filter((b) => Number.isFinite(b[k])).map((b) => [b.hour, b[k]]));
  const fmt = (x, d = 1) => (Number.isFinite(x) ? x.toFixed(d) : "–");
  console.log(`## ${run.variant.label} ${run.memory} / ${run.cpus} vCPU, ${rows.length} blocks (${fmt((rows.at(-1)?.end - t0) / 6e4, 0)} min)` +
    `${run.final?.oomKilled ? " — OOM-KILLED" : ""}\n`);
  console.log("| Block | Hour | OK | Failed | Peak MiB (anon) | End of idle MiB (anon) | V8 heap total | V8 live after GC | malloc in use | Isolate restarts |");
  console.log("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
  for (const b of rows) {
    console.log(`| ${b.n} | ${fmt(b.hour, 2)} | ${b.ok} | ${b.failed} | ${fmt(b.peak)} (${fmt(b.peakAnon)}) | ${fmt(b.idleCurrent)} (${fmt(b.idleAnon)}) | ${fmt(b.heapTotal)} | ${fmt(b.heapAfterGc)} | ${fmt(b.mallocInUse)} | ${b.restarts ?? "–"} |`);
  }
  const range = (k) => {
    const v = warm.map((b) => b[k]).filter(Number.isFinite);
    return v.length ? `${fmt(Math.min(...v))}–${fmt(Math.max(...v))}` : "–";
  };
  console.log(`\nAfter block ${warmup}: end-of-idle anon ${range("idleAnon")} MiB (slope ${fmt(trend("idleAnon"), 2)} MiB/h), ` +
    `peak ${range("peak")} MiB (slope ${fmt(trend("peak"), 2)} MiB/h), V8 live ${range("heapAfterGc")} MiB (slope ${fmt(trend("heapAfterGc"), 2)} MiB/h), ` +
    `malloc in use ${range("mallocInUse")} MiB (slope ${fmt(trend("mallocInUse"), 2)} MiB/h).\n`);
}
