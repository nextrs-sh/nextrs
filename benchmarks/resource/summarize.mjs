// Summarizes a `run.mjs profile` directory:
//
//   node benchmarks/resource/summarize.mjs DIR [--json OUT.json]
//
// One table per container configuration, one row per variant, medians
// over rounds (min–max when rounds differ by more than 10 %). Every run is
// counted: a run that was OOM-killed, failed readiness or had failed
// requests is listed as such and its measurements up to the failure are
// kept. Memory is the container's (cgroup `memory.current`, includes page
// cache) and its anonymous part (`memory.stat anon`), from the probe's
// 1-second samples; CPU is the container's cgroup CPU time per successful
// request at a fixed offered rate.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [dir, ...rest] = process.argv.slice(2);
const jsonOut = rest[rest.indexOf("--json") + 1];
const MiB = 2 ** 20;

const median = (xs) => {
  const v = xs.filter((x) => Number.isFinite(x)).sort((a, b) => a - b);
  if (!v.length) return null;
  const m = Math.floor(v.length / 2);
  return v.length % 2 ? v[m] : (v[m - 1] + v[m]) / 2;
};
const fmt = (xs, digits = 1) => {
  const v = xs.filter((x) => Number.isFinite(x));
  if (!v.length) return "–";
  const m = median(v);
  const lo = Math.min(...v);
  const hi = Math.max(...v);
  const spread = m ? (hi - lo) / Math.abs(m) : 0;
  return spread > 0.1 ? `${m.toFixed(digits)} (${lo.toFixed(digits)}–${hi.toFixed(digits)})` : m.toFixed(digits);
};

function loadSamples(file) {
  try {
    return readFileSync(file, "utf8").trim().split("\n").filter(Boolean).map((l) => JSON.parse(l));
  } catch {
    return [];
  }
}

function window(samples, from, to) {
  return samples.filter((s) => s.t >= from && s.t <= to);
}

function summarizeRun(run, samples) {
  const out = {
    label: run.variant.label, memory: run.memory, cpus: run.cpus, round: run.round,
    readyMs: run.readyMs ?? null, error: run.error ?? null,
    oom: Boolean(run.final?.oomKilled), exitCode: run.final?.exitCode ?? null, phases: {},
  };
  const at = (t) => samples.filter((s) => s.t <= t).at(-1);
  const idle = run.marks?.idle ? at(run.marks.idle) : null;
  out.idleMiB = idle ? idle.memory_current / MiB : null;
  out.idleAnonMiB = idle ? idle.stat.anon / MiB : null;
  const idleSmaps = run.marks?.idle ? samples.filter((s) => s.smaps && s.t <= run.marks.idle).at(-1) : null;
  out.idlePssMiB = idleSmaps?.smaps?.Pss ? idleSmaps.smaps.Pss / MiB : null;
  out.peakMiB = samples.length ? Math.max(...samples.map((s) => s.memory_current)) / MiB : null;
  out.peakAnonMiB = samples.length ? Math.max(...samples.map((s) => s.stat.anon ?? 0)) / MiB : null;
  out.maxThreads = samples.length ? Math.max(...samples.map((s) => s.proc.Threads ?? 0)) : null;
  for (const p of run.phases ?? []) {
    const w = window(samples, p.started, p.ended);
    const r = p.result;
    out.phases[p.name] = {
      ok: r?.ok ?? 0, completed: r?.completed ?? 0, failed: p.failed ?? (p.error ? 1 : 0), error: p.error ?? null,
      rps: r?.achieved_rps ?? null, offered: p.rate,
      p50: r?.latency?.p50_ms ?? null, p95: r?.latency?.p95_ms ?? null, p99: r?.latency?.p99_ms ?? null,
      p999: r?.latency?.p999_ms ?? null, cpuMsPerOk: p.cpu_ms_per_ok ?? null,
      cpuCores: p.cpu_usec && p.ended > p.started ? p.cpu_usec / 1000 / (p.ended - p.started) : null,
      throttledMs: p.throttled_usec != null ? p.throttled_usec / 1000 : null,
      peakMiB: w.length ? Math.max(...w.map((s) => s.memory_current)) / MiB : null,
      endMiB: p.after?.["memory.current"] ? p.after["memory.current"] / MiB : null,
      heapUsedMiB: p.metricsAfter?.executors?.[0]?.heap_used_bytes / MiB,
      heapTotalMiB: p.metricsAfter?.executors?.reduce((a, e) => a + e.heap_total_bytes, 0) / MiB,
      mallocFreeMiB: p.metricsAfter?.memory?.malloc?.free_bytes / MiB,
    };
  }
  out.recovery = Object.fromEntries((run.recovery ?? []).map((r) => [r.after_s, r.cgroup ? r.cgroup["memory.current"] / MiB : null]));
  const recEnd = run.recovery?.at(-1);
  const recSample = recEnd ? at(recEnd.cgroup?.t ?? 0) : null;
  out.recoveredAnonMiB = recSample ? recSample.stat.anon / MiB : null;
  const m = recEnd?.metrics;
  out.recoveredHeapUsedMiB = m?.executors ? m.executors.reduce((a, e) => a + e.heap_used_bytes, 0) / MiB : null;
  out.recoveredHeapTotalMiB = m?.executors ? m.executors.reduce((a, e) => a + e.heap_total_bytes, 0) / MiB : null;
  out.recoveredMallocFreeMiB = m?.memory?.malloc ? m.memory.malloc.free_bytes / MiB : null;
  out.recoveredMallocInUseMiB = m?.memory?.malloc ? (m.memory.malloc.in_use_bytes + m.memory.malloc.mmap_bytes) / MiB : null;
  out.restarts = m?.executors ? m.executors.reduce((a, e) => a + (e.restarts ?? 0), 0) : null;
  out.failedRequests = Object.values(out.phases).reduce((a, p) => a + (p.failed || 0), 0);
  return out;
}

const files = readdirSync(dir).filter((f) => f.endsWith(".json") && !f.startsWith("meta"));
const runs = files.map((f) => {
  const run = JSON.parse(readFileSync(join(dir, f), "utf8"));
  return summarizeRun(run, loadSamples(join(dir, f.replace(/\.json$/, ".samples.jsonl"))));
});

const configs = [...new Set(runs.map((r) => `${r.memory}:${r.cpus}`))];
const labels = [...new Set(runs.map((r) => r.label))];
const lines = [`# ${dir}`, ""];
const phaseNames = [...new Set(runs.flatMap((r) => Object.keys(r.phases)))];
for (const config of configs) {
  const [memory, cpus] = config.split(":");
  lines.push(`## ${memory} memory, ${cpus} vCPU`, "");
  lines.push("| Variant | Runs | OOM / failed runs | Failed requests | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 60 s | After 120 s (anon) | Heap used/total after 120 s | malloc free after 120 s |");
  lines.push("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
  for (const label of labels) {
    const rs = runs.filter((r) => r.label === label && `${r.memory}:${r.cpus}` === config);
    if (!rs.length) continue;
    const bad = rs.filter((r) => r.oom || r.error);
    lines.push(`| ${label} | ${rs.length} | ${bad.length ? bad.map((r) => `r${r.round}: ${r.oom ? "OOM" : r.error.slice(0, 40)}`).join("; ") : "0"} | ${rs.map((r) => r.failedRequests).join(", ")} | ${fmt(rs.map((r) => r.readyMs), 0)} | ${fmt(rs.map((r) => r.idleMiB))} (${fmt(rs.map((r) => r.idleAnonMiB))}) | ${fmt(rs.map((r) => r.idlePssMiB))} | ${fmt(rs.map((r) => r.peakMiB))} (${fmt(rs.map((r) => r.peakAnonMiB))}) | ${fmt(rs.map((r) => r.recovery[10]))} | ${fmt(rs.map((r) => r.recovery[60]))} | ${fmt(rs.map((r) => r.recovery[120]))} (${fmt(rs.map((r) => r.recoveredAnonMiB))}) | ${fmt(rs.map((r) => r.recoveredHeapUsedMiB))} / ${fmt(rs.map((r) => r.recoveredHeapTotalMiB))} | ${fmt(rs.map((r) => r.recoveredMallocFreeMiB))} |`);
  }
  lines.push("");
  lines.push("| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Peak MiB |");
  lines.push("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
  for (const phase of phaseNames.filter((p) => p.startsWith("steady") || p.startsWith("burst"))) {
    for (const label of labels) {
      const rs = runs.filter((r) => r.label === label && `${r.memory}:${r.cpus}` === config && r.phases[phase]);
      if (!rs.length) continue;
      const ps = rs.map((r) => r.phases[phase]);
      lines.push(`| ${label} | ${phase} | ${ps[0].offered || "closed"} | ${fmt(ps.map((p) => p.rps))} | ${ps.map((p) => p.failed).join(", ")} | ${fmt(ps.map((p) => p.p50))} | ${fmt(ps.map((p) => p.p99))} | ${fmt(ps.map((p) => p.p999))} | ${fmt(ps.map((p) => p.cpuMsPerOk), 2)} | ${fmt(ps.map((p) => p.cpuCores), 2)} | ${fmt(ps.map((p) => p.peakMiB))} |`);
    }
  }
  lines.push("");
}
console.log(lines.join("\n"));
if (jsonOut) writeFileSync(jsonOut, JSON.stringify(runs, null, 1));
