// Compression-cache churn soak (rc.3, PERF-1): the rc.2 churn soak
// (`benchmarks/rc2/compression.mjs --soak --soak-mix full`) run for longer
// and measured as a time series.
//
//   node benchmarks/rc3/churn-soak.mjs --out DIR --project fixtures/next-app \
//     --bin PATH --label NAME [--cache MIB] [--minutes 60] [--segment 60] \
//     [--connections 64] [--server-cpus 0-3] [--load-cpus 4-5] \
//     [--loadgen PATH] [--env KEY=VALUE,...]
//
// The request mix is rc.2's: every JavaScript and CSS file under
// .next/static (static-mix), the prerendered page, SSR, RSC, a JSON route
// handler, the largest chunk, a byte range of it and the streaming page,
// all with `Accept-Encoding: gzip`. With `--cache 1` the gzip copies of the
// static set do not fit (eviction churn).
//
// The load generator runs in consecutive segments of --segment seconds
// (closed loop, keep-alive), so throughput, latency percentiles and errors
// are a time series; the gap between segments is the load generator's
// start-up (tens of milliseconds). Every 5 s: RSS, anonymous memory,
// threads, CPU of the server process tree, the thread count by name, and
// the server's /metrics.json (compression cache counters, in-flight
// requests, open connections). Results are written after every segment.
import { spawn } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import * as L from "../phase3a/lib.mjs";

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) {
  if (!argv[i].startsWith("--")) continue;
  const k = argv[i].slice(2);
  const v = argv[i + 1];
  if (v === undefined || v.startsWith("--")) args[k] = true;
  else (args[k] = v), i++;
}
const out = resolve(args.out ?? "benchmarks/results/rc3/churn");
const project = resolve(args.project ?? "fixtures/next-app");
const bin = resolve(args.bin);
const label = args.label ?? "candidate";
const minutes = Number(args.minutes ?? 60);
const segment = Number(args.segment ?? 60);
const connections = Number(args.connections ?? 64);
const serverCpus = args["server-cpus"] ?? "0-3";
const loadCpus = args["load-cpus"] ?? "4-5";
const loadgenBin = resolve(args.loadgen ?? "benchmarks/loadgen/target/release/nextrs-loadgen");
const extraEnv = Object.fromEntries(
  String(args.env ?? "")
    .split(",")
    .filter(Boolean)
    .map((kv) => kv.split("=")),
);
mkdirSync(out, { recursive: true });

function staticFiles() {
  const root = join(project, ".next/static");
  const found = [];
  const walk = (dir, rel) => {
    for (const f of readdirSync(dir)) {
      const p = join(dir, f);
      if (statSync(p).isDirectory()) walk(p, `${rel}/${f}`);
      else if (/\.(js|css)$/.test(f)) found.push({ path: `/_next/static${rel}/${f}`, size: statSync(p).size });
    }
  };
  walk(root, "");
  return found.sort((a, b) => a.path.localeCompare(b.path));
}
const assets = staticFiles();
const chunk = assets.reduce((a, b) => (b.size > a.size && b.path.endsWith(".js") ? b : a), assets[0]);
const gzip = ["accept-encoding", "gzip"];
const requests = [
  ...assets.map((a) => ({ path: a.path, kind: "static-mix", headers: [gzip], expect_body: [] })),
  { path: "/", kind: "prerendered", headers: [["accept", "text/html"], gzip], expect_body: [] },
  { path: "/books/42", kind: "ssr", headers: [["accept", "text/html"], gzip], expect_body: [] },
  { path: "/books/42?_rsc", kind: "rsc", headers: [["rsc", "1"], gzip], expect_body: [] },
  { path: "/api/health", kind: "json-api", headers: [gzip], expect_body: [] },
  { path: chunk.path, kind: "large-static", headers: [gzip], expect_body: [] },
  { path: chunk.path, kind: "range", headers: [["range", "bytes=1000-17383"], gzip], expect_status: 206, expect_body: [] },
  { path: "/dynamic", kind: "streaming-64", headers: [["accept", "text/html"], gzip], expect_body: [] },
];
const spec = join(out, `spec-${label}.json`);
writeFileSync(spec, JSON.stringify({ requests }));
const workingSet = assets.reduce((n, a) => n + a.size, 0);

const upstreamPort = 5391;
const upstream = spawn(process.execPath, [resolve("tests/compat/upstream.mjs"), "--port", String(upstreamPort)], { stdio: "ignore" });
await L.sleep(500);
const env = { NEXTRS_UPSTREAM: `http://127.0.0.1:${upstreamPort}`, ...extraEnv };
const extraArgs = ["--js-env", "NEXTRS_UPSTREAM", "--net-allow", env.NEXTRS_UPSTREAM, "--net-allow-addr", "127.0.0.1"];
if (args.cache !== undefined) extraArgs.push("--compression-cache", String(args.cache));

function threadNames(pid) {
  const names = {};
  for (const p of L.tree(pid)) {
    let tasks = [];
    try { tasks = readdirSync(`/proc/${p}/task`); } catch {}
    for (const t of tasks) {
      try {
        const n = readFileSync(`/proc/${p}/task/${t}/comm`, "utf8").trim().replace(/[0-9]+$/, "#");
        names[n] = (names[n] ?? 0) + 1;
      } catch {}
    }
  }
  return names;
}

// Page faults of the process tree (/proc/<pid>/stat fields 10 and 12),
// cumulative: allocator churn shows as minor faults.
function faults(pid) {
  let minflt = 0, majflt = 0;
  for (const p of L.tree(pid)) {
    try {
      const f = readFileSync(`/proc/${p}/stat`, "utf8").split(") ")[1].split(" ");
      minflt += Number(f[7]);
      majflt += Number(f[9]);
    } catch {}
  }
  return { minflt, majflt };
}

const port = 5802;
const server = await L.startServer({ kind: "nextrs", nextrs: bin, project, port, metricsPort: port + 100, cpus: serverCpus, env, extraArgs, readyPath: "/api/health" });
const t0 = Date.now();
const samples = [];
const segments = [];
const sampler = setInterval(async () => {
  const m = L.sample(server.pid);
  const row = {
    t_s: +((Date.now() - t0) / 1000).toFixed(1), rss_mib: m.rss_mib, anon_mib: m.anon_mib, threads: m.threads,
    cpu_s: +(m.cpu_user_s + m.cpu_sys_s).toFixed(2), cpu_user_s: +m.cpu_user_s.toFixed(2), cpu_sys_s: +m.cpu_sys_s.toFixed(2),
    // Context switches of the live threads (a thread's count leaves with it).
    vcsw: m.voluntary_ctxt_switches, ivcsw: m.nonvoluntary_ctxt_switches,
    ...faults(server.pid), thread_names: threadNames(server.pid),
  };
  const metrics = await L.metrics(server.metricsPort);
  if (metrics) {
    row.compression_cache = metrics.compression_cache ?? null;
    row.in_flight = metrics.http?.in_flight ?? null;
    row.connections_open = metrics.http?.connections_open ?? null;
    // glibc's view of the heap (arena bytes, in use, free, mmapped): what
    // separates allocator retention from live memory.
    row.malloc = metrics.memory?.malloc ?? null;
    row.process = metrics.process ?? null;
  }
  samples.push(row);
}, 5000);

const write = (done) => {
  const lat = (k) => segments.map((s) => s[k]);
  const summary = {
    label, done, minutes, segment_s: segment, connections, cache_mib: args.cache ?? "default (32)", env: extraEnv,
    working_set_bytes: workingSet, static_files: assets.length, segments: segments.length,
    completed: segments.reduce((n, s) => n + s.completed, 0), failures: segments.reduce((n, s) => n + s.failures, 0),
    rps: lat("rps"), p50_ms: lat("p50_ms"), p99_ms: lat("p99_ms"),
  };
  writeFileSync(join(out, `soak-${label}.json`), JSON.stringify({ summary, segments, samples, binary: bin, project, startup_ms: server.startup_ms }, null, 1));
};

try {
  const total = Math.ceil((minutes * 60) / segment);
  for (let i = 0; i < total; i++) {
    const lg = await L.loadgen(loadgenBin, { cpus: loadCpus, target: `127.0.0.1:${port}`, spec, connections, duration: segment, warmup: i === 0 ? 10 : 0 });
    segments.push({
      i, t_end_s: +((Date.now() - t0) / 1000).toFixed(1), completed: lg.completed, rps: +lg.achieved_rps.toFixed(1),
      failures: lg.status_mismatch + lg.body_mismatch + lg.io_errors + lg.timeouts, timeouts: lg.timeouts, io_errors: lg.io_errors,
      p50_ms: lg.latency.p50_ms, p95_ms: lg.latency.p95_ms, p99_ms: lg.latency.p99_ms, p999_ms: lg.latency.p999_ms, max_ms: lg.latency.max_ms,
    });
    write(false);
    const last = samples.at(-1) ?? {};
    console.log(`${label} segment ${i + 1}/${total}: ${segments.at(-1).rps} req/s p99 ${segments.at(-1).p99_ms} ms rss ${last.rss_mib} MiB threads ${last.threads} evictions ${last.compression_cache?.evictions ?? "-"}`);
  }
} finally {
  clearInterval(sampler);
  write(true);
  await L.stopServer(server);
  upstream.kill();
}
