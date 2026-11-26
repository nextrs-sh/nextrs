// Summarizes a `run.mjs matrix` (or `memory`) result directory: one row per
// run and phase (summary.csv), and the median with the range across
// rounds per runtime and phase (summary.md, summary.json).
//
//   node benchmarks/series1/summarize.mjs DIR
//
// Definitions (all from the storefront container's own cgroup, so the
// whole container is counted, whatever the runtime):
// * cpu_ms_per_req: cgroup CPU time (user + system) during the measured
//   window / successful requests. Background work in that window (GC,
//   timers) is included, as it is real cost.
// * req_per_cpu_s: the inverse.
// * latency: open loop, from the scheduled send (coordinated-omission
//   aware); closed loop: service time. TTFB from the actual send.
// * memory: cgroup memory.current (anon + file cache + kernel), its anon
//   part, and the main process's RSS (VmRSS) and anonymous RSS.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const dir = process.argv[2];
// --max-host-load N: leave out windows whose host 1-minute load average
// (before or after) was N or more; the left-out windows are counted.
const maxLoadArg = process.argv.indexOf("--max-host-load");
const maxHostLoad = maxLoadArg > 0 ? Number(process.argv[maxLoadArg + 1]) : null;
const runs = readdirSync(dir)
  .filter((f) => f.endsWith(".json") && !["meta.json", "summary.json"].includes(f) && !f.startsWith("capture"))
  .map((f) => ({ file: f, ...JSON.parse(readFileSync(join(dir, f), "utf8")) }));

const MiB = 1024 * 1024;
const r2 = (x, d = 2) => (x == null || Number.isNaN(x) ? null : Number(x.toFixed(d)));
const delta = (a, b, k) => (a?.[k] != null && b?.[k] != null ? b[k] - a[k] : null);

function phaseRow(run, p) {
  const ok = p.result.ok;
  const cpuUs = delta(p.before, p.after, "cpu_usage_usec");
  const window = (p.after.t - p.before.t) / 1000;
  const mb = p.metricsBefore;
  const ma = p.metricsAfter;
  const d = (f) => (mb && ma ? f(ma) - f(mb) : null);
  const stage = (m, via) => m?.http?.stages?.served?.[via] ?? 0;
  const vm = p.contention;
  const vmBusy = vm && vm.after.vmTotalTicks != null
    ? (vm.after.vmBusyTicks - vm.before.vmBusyTicks) / Math.max(vm.after.vmTotalTicks - vm.before.vmTotalTicks, 1)
    : null;
  return {
    runtime: run.runtime, round: run.round, position: run.position, phase: p.name,
    mode: p.loadgen.rate ? "open" : "closed", offered: p.loadgen.rate || null, connections: p.loadgen.connections,
    completed: p.result.completed, ok, failed: p.result.completed - ok + p.result.io_errors + p.result.timeouts,
    rps: r2(p.result.achieved_rps, 1), late_starts: p.result.late_starts,
    p50: p.result.latency.p50_ms, p95: p.result.latency.p95_ms, p99: p.result.latency.p99_ms,
    p999: p.result.latency.p999_ms, max: p.result.latency.max_ms,
    service_p50: p.result.service.p50_ms, ttfb_p50: p.result.ttfb.p50_ms, ttfb_p95: p.result.ttfb.p95_ms,
    bytes_per_req: ok ? Math.round(p.result.bytes_received / ok) : null,
    cpu_ms_per_req: cpuUs != null && ok ? r2(cpuUs / 1000 / ok, 4) : null,
    user_ms_per_req: ok ? r2(delta(p.before, p.after, "cpu_user_usec") / 1000 / ok, 4) : null,
    sys_ms_per_req: ok ? r2(delta(p.before, p.after, "cpu_system_usec") / 1000 / ok, 4) : null,
    req_per_cpu_s: cpuUs ? r2(ok / (cpuUs / 1e6), 0) : null,
    mbytes_per_cpu_s: cpuUs ? r2(p.result.bytes_received / MiB / (cpuUs / 1e6), 1) : null,
    cpu_util: cpuUs != null ? r2(cpuUs / 1e6 / window, 3) : null,
    throttled: delta(p.before, p.after, "cpu_nr_throttled"),
    ctx_switches: (delta(p.before, p.after, "proc_voluntary_ctxt_switches") ?? 0) + (delta(p.before, p.after, "proc_nonvoluntary_ctxt_switches") ?? 0),
    mem_mib: r2(p.after.memory_current / MiB, 1), anon_mib: r2(p.after.mem_anon / MiB, 1),
    file_mib: r2(p.after.mem_file / MiB, 1), rss_mib: r2(p.after.proc_VmRSS / MiB, 1),
    rss_anon_mib: r2(p.after.proc_RssAnon / MiB, 1), threads: p.after.proc_Threads,
    loadgen_cpu_s: r2(p.result.loadgen_cpu_s, 2),
    host_load1: vm?.before.hostLoad1 ?? null, host_load1_after: vm?.after.hostLoad1 ?? null,
    vm_load1: vm?.before.vmLoad1 ?? null, vm_busy: r2(vmBusy, 3),
    // NextRS only: what answered, and the per-request work behind it.
    via_executor: d((m) => stage(m, "executor")),
    via_static: d((m) => stage(m, "static")),
    via_prerendered: d((m) => stage(m, "prerendered")),
    exec_ms_per_req: ok ? r2(d((m) => m.execution_seconds_total ?? 0) * 1000 / ok, 3) : null,
    queue_ms_per_req: ok ? r2(d((m) => m.queue_wait_seconds_total ?? 0) * 1000 / ok, 3) : null,
    cache_value_kib_per_req: ok && ma?.cache?.value_bytes != null ? r2(d((m) => m.cache.value_bytes) / 1024 / ok, 1) : null,
    cache_reused_per_req: ok && ma?.cache?.reused != null ? r2(d((m) => m.cache.reused) / ok, 3) : null,
    stream_frames_per_req: ok && ma?.http?.stream_compression ? r2(d((m) => m.http.stream_compression.input_frames) / ok, 1) : null,
    stream_gzip_ms_per_req: ok && ma?.http?.stream_compression ? r2(d((m) => m.http.stream_compression.encode_seconds) * 1000 / ok, 4) : null,
    cc_hits: d((m) => m.compression_cache?.hits ?? 0), cc_misses: d((m) => m.compression_cache?.misses ?? 0),
    cc_coalesced: d((m) => m.compression_cache?.coalesced ?? 0),
    heap_used_mib: ma?.executors?.length ? r2(ma.executors.reduce((s, e) => s + e.heap_used_bytes, 0) / MiB, 1) : null,
    heap_total_mib: ma?.executors?.length ? r2(ma.executors.reduce((s, e) => s + e.heap_total_bytes, 0) / MiB, 1) : null,
    malloc_in_use_mib: ma?.memory?.malloc ? r2(ma.memory.malloc.in_use_bytes / MiB, 1) : null,
    malloc_free_mib: ma?.memory?.malloc ? r2(ma.memory.malloc.free_bytes / MiB, 1) : null,
  };
}

function runRow(run) {
  const idleCpu = run.idleStart && run.idleEnd
    ? delta(run.idleStart, run.idleEnd, "cpu_usage_usec") / 1e6 / ((run.idleEnd.t - run.idleStart.t) / 1000)
    : null;
  const first = run.idleStart ?? run.idle;
  return {
    runtime: run.runtime, round: run.round, position: run.position, isolates: run.isolates ?? 1, js: run.jsRuntime ?? "default",
    ready_ms: run.readyMs,
    idle_cpu_millicores: idleCpu != null ? r2(idleCpu * 1000, 2) : null,
    idle_mem_mib: r2(first?.memory_current / MiB, 1), idle_anon_mib: r2(first?.mem_anon / MiB, 1),
    idle_rss_mib: r2(first?.proc_VmRSS / MiB, 1), idle_rss_anon_mib: r2(first?.proc_RssAnon / MiB, 1),
    idle_threads: first?.proc_Threads,
    after_load_rss_mib: r2((run.afterLoad ?? run.phases?.at(-1)?.after)?.proc_VmRSS / MiB, 1),
    peak_mem_mib: r2((run.recovered ?? run.end)?.memory_peak / MiB, 1),
    peak_rss_mib: r2((run.recovered ?? run.end)?.proc_VmHWM / MiB, 1),
    recovered_mem_mib: r2(run.recovered?.memory_current / MiB, 1),
    recovered_anon_mib: r2(run.recovered?.mem_anon / MiB, 1),
    recovered_rss_mib: r2(run.recovered?.proc_VmRSS / MiB, 1),
    recovered_rss_anon_mib: r2(run.recovered?.proc_RssAnon / MiB, 1),
    idle_heap_used_mib: run.metricsIdleStart?.executors?.length || run.metricsIdle?.executors?.length
      ? r2((run.metricsIdleStart ?? run.metricsIdle).executors.reduce((s, e) => s + e.heap_used_bytes, 0) / MiB, 1) : null,
    recovered_heap_used_mib: run.metricsRecovered?.executors?.length
      ? r2(run.metricsRecovered.executors.reduce((s, e) => s + e.heap_used_bytes, 0) / MiB, 1) : null,
    recovered_malloc_free_mib: run.metricsRecovered?.memory?.malloc ? r2(run.metricsRecovered.memory.malloc.free_bytes / MiB, 1) : null,
  };
}

const allPhases = runs.flatMap((run) => (run.phases ?? []).map((p) => phaseRow(run, p)));
const contended = (p) => maxHostLoad !== null && (p.host_load1 >= maxHostLoad || p.host_load1_after >= maxHostLoad);
const phases = allPhases.filter((p) => !contended(p));
const excluded = allPhases.filter(contended);
const runRows = runs.map(runRow);

function csv(rows) {
  if (!rows.length) return "";
  const cols = Object.keys(rows[0]);
  return [cols.join(","), ...rows.map((r) => cols.map((c) => r[c] ?? "").join(","))].join("\n") + "\n";
}
writeFileSync(join(dir, "phases.csv"), csv(allPhases));
writeFileSync(join(dir, "runs.csv"), csv(runRows));

const median = (xs) => {
  const v = xs.filter((x) => x != null && !Number.isNaN(x)).sort((a, b) => a - b);
  if (!v.length) return null;
  const m = Math.floor(v.length / 2);
  return v.length % 2 ? v[m] : (v[m - 1] + v[m]) / 2;
};
const spread = (xs) => {
  const v = xs.filter((x) => x != null && !Number.isNaN(x));
  if (!v.length) return "n/a";
  const med = median(v);
  const lo = Math.min(...v);
  const hi = Math.max(...v);
  const f = (x) => (Math.abs(x) >= 100 ? x.toFixed(0) : Math.abs(x) >= 10 ? x.toFixed(1) : x.toFixed(3).replace(/0+$/, "").replace(/\.$/, ""));
  return v.length > 1 ? `${f(med)} (${f(lo)}–${f(hi)})` : f(med);
};
const group = (rows, keyOf) => {
  const m = new Map();
  for (const r of rows) {
    const k = keyOf(r);
    if (!m.has(k)) m.set(k, []);
    m.get(k).push(r);
  }
  return m;
};

const runtimes = [...new Set(runs.map((r) => r.runtime))];
const summary = { phases: {}, runs: {} };
let md = `# Summary of ${dir}\n\nMedian across rounds, with (min–max). n = rounds.\n\n`;
if (maxHostLoad !== null) {
  md += `Windows with a host load average of ${maxHostLoad} or more are left out: ${excluded.length} of ${allPhases.length}` +
    (excluded.length ? ` (${[...new Set(excluded.map((p) => `${p.runtime} r${p.round} ${p.phase}`))].join(", ")})` : "") + ".\n\n";
}
const phaseNames = [...new Set(phases.map((p) => p.phase))].filter((n) => !n.startsWith("warmup"));
const cols = ["rps", "p50", "p95", "p99", "max", "ttfb_p50", "cpu_ms_per_req", "req_per_cpu_s", "rss_mib", "failed", "vm_busy"];
const nextrsCols = ["via_executor", "exec_ms_per_req", "cache_value_kib_per_req", "cache_reused_per_req", "stream_frames_per_req", "stream_gzip_ms_per_req", "heap_used_mib"];
for (const name of phaseNames) {
  md += `## ${name}\n\n| runtime | n | ${cols.join(" | ")} |\n|---|---:|${cols.map(() => "---:").join("|")}|\n`;
  summary.phases[name] = {};
  for (const rt of runtimes) {
    const rows = phases.filter((p) => p.phase === name && p.runtime === rt);
    if (!rows.length) continue;
    summary.phases[name][rt] = Object.fromEntries([...cols, ...nextrsCols].map((c) => [c, median(rows.map((r) => r[c]))]));
    md += `| ${rt} | ${rows.length} | ${cols.map((c) => spread(rows.map((r) => r[c]))).join(" | ")} |\n`;
  }
  const nx = runtimes.filter((rt) => rt !== "node");
  if (nx.some((rt) => phases.some((p) => p.phase === name && p.runtime === rt && p.via_executor != null))) {
    md += `\n| runtime (NextRS metrics) | ${nextrsCols.join(" | ")} |\n|---|${nextrsCols.map(() => "---:").join("|")}|\n`;
    for (const rt of nx) {
      const rows = phases.filter((p) => p.phase === name && p.runtime === rt);
      if (rows.length) md += `| ${rt} | ${nextrsCols.map((c) => spread(rows.map((r) => r[c]))).join(" | ")} |\n`;
    }
  }
  md += "\n";
}
const runCols = ["ready_ms", "idle_cpu_millicores", "idle_mem_mib", "idle_rss_mib", "idle_rss_anon_mib", "idle_threads", "peak_mem_mib", "peak_rss_mib", "recovered_mem_mib", "recovered_rss_mib", "recovered_rss_anon_mib", "idle_heap_used_mib", "recovered_heap_used_mib"];
md += `## Process and memory per run\n\n| runtime | isolates | js | n | ${runCols.join(" | ")} |\n|---|---:|---|---:|${runCols.map(() => "---:").join("|")}|\n`;
for (const [k, rows] of group(runRows, (r) => `${r.runtime}|${r.isolates}|${r.js}`)) {
  const [rt, iso, js] = k.split("|");
  summary.runs[k] = Object.fromEntries(runCols.map((c) => [c, median(rows.map((r) => r[c]))]));
  md += `| ${rt} | ${iso} | ${js} | ${rows.length} | ${runCols.map((c) => spread(rows.map((r) => r[c]))).join(" | ")} |\n`;
}
writeFileSync(join(dir, "summary.md"), md);
writeFileSync(join(dir, "summary.json"), JSON.stringify(summary, null, 1));
console.log(md);
