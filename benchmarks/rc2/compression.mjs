// Compression and deployment-ID regression benchmark (0.1.0-rc.2
// candidate): a NextRS baseline build against the candidate, and next
// start as the reference, each with requests that do not accept
// compression (identity) and with `Accept-Encoding: gzip`, on one build of
// fixtures/next-app (no deployment ID, which the baseline cannot serve).
//
//   node benchmarks/rc2/compression.mjs --out benchmarks/results/rc2/<name> \
//     --project DIR --base PATH --candidate PATH [--node PATH] [--loadgen PATH]
//     [--rounds 3] [--duration 10] [--warmup 3] [--server-cpus 0-3] [--load-cpus 4-5]
//     [--only id,...] [--configs label,...]
//   node benchmarks/rc2/compression.mjs --soak --out ... --project DIR \
//     --candidate PATH [--base PATH] [--minutes 10] [--encoding gzip]
//   node benchmarks/rc2/compression.mjs --cold --out ... --project DIR \
//     --candidate PATH [--base PATH] [--rounds 3]
//
// Final rc.2 additions (the gzip cache of immutable responses, byte
// ranges): `cand-nocache-gzip` runs the candidate with
// `--compression-cache 0` (every response compressed per request, the
// cost of a miss), `cand-tiny-gzip` with a 1 MiB cache that cannot hold
// the static chunks of `static-mix` (eviction churn). Workloads
// `static-mix` (every chunk, round-robin), `range` (206 from the large
// chunk) and `conditional` (304 revalidations) are added; with
// `--project fixtures/next-cache-components` the workloads are its PPR
// page and its prerendered page instead (`--upstream none`: no data
// upstream, the cached functions return at once). `--cold` measures the
// first and second gzip request for every compressible static file and
// prerendered page on a fresh server (the cache's cold path).
//
// Per (configuration, workload, round): a closed-loop run of the load
// generator (keep-alive connections, every response's status checked; with
// identity also its body), server process-tree CPU and memory over exactly
// the measured window. Configurations are interleaved in a shuffled order
// each round, one server process per configuration and round. With gzip the
// bodies cannot be checked by substring (they are compressed), so before
// each gzip run one response is fetched, decoded and checked instead.
//
// The soak runs a mixed workload for --minutes with RSS sampled every 5 s.
import { mkdirSync, writeFileSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { gunzipSync } from "node:zlib";
import * as L from "../phase3a/lib.mjs";

const failures = (lg) => lg.status_mismatch + lg.body_mismatch + lg.io_errors + lg.timeouts;

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) {
  if (!argv[i].startsWith("--")) continue;
  const k = argv[i].slice(2);
  const v = argv[i + 1];
  if (v === undefined || v.startsWith("--")) args[k] = true;
  else (args[k] = v), i++;
}
const out = resolve(args.out ?? "benchmarks/results/rc2/compression");
const project = resolve(args.project ?? "fixtures/next-app");
const node = args.node ?? process.execPath;
const loadgenBin = resolve(args.loadgen ?? "benchmarks/loadgen/target/release/loadgen");
const serverCpus = args["server-cpus"] ?? "0-3";
const loadCpus = args["load-cpus"] ?? "4-5";
const rounds = Number(args.rounds ?? 3);
const duration = Number(args.duration ?? 10);
const warmup = Number(args.warmup ?? 3);
mkdirSync(out, { recursive: true });

// The largest JavaScript chunk: the "large compressible response".
function largestChunk() {
  const dir = join(project, ".next/static/chunks");
  let best = null;
  for (const f of readdirSync(dir)) {
    if (!f.endsWith(".js")) continue;
    const size = statSync(join(dir, f)).size;
    if (!best || size > best.size) best = { f, size };
  }
  return { path: `/_next/static/chunks/${best.f}`, size: best.size };
}
const chunk = largestChunk();

// Every JavaScript and CSS file under .next/static (the cacheable assets).
function staticFiles() {
  const root = join(project, ".next/static");
  const out = [];
  const walk = (dir, rel) => {
    for (const f of readdirSync(dir)) {
      const p = join(dir, f);
      if (statSync(p).isDirectory()) walk(p, `${rel}/${f}`);
      else if (/\.(js|css)$/.test(f)) out.push({ path: `/_next/static${rel}/${f}`, size: statSync(p).size });
    }
  };
  walk(root, "");
  return out.sort((a, b) => a.path.localeCompare(b.path));
}
const assets = staticFiles();
const ppr = project.includes("cache-components");

const appWorkloads = [
  { id: "prerendered", connections: 32, req: { path: "/", headers: [["accept", "text/html"]], expect_body: ['id="title">NextRS static fixture'] } },
  { id: "ssr", connections: 32, req: { path: "/books/42", headers: [["accept", "text/html"]], expect_body: ["Book <!-- -->42", "</html>"] } },
  { id: "rsc", connections: 32, req: { path: "/books/42?_rsc", headers: [["rsc", "1"]], expect_body: ['"42"'] } },
  { id: "json-api", connections: 32, req: { path: "/api/health", expect_body: ['"status":"ok"'] } },
  { id: "large-static", connections: 32, req: { path: chunk.path, expect_body: [] }, size: chunk.size },
  { id: "streaming-64", connections: 64, req: { path: "/dynamic", headers: [["accept", "text/html"]], expect_body: ["Noli Me Tangere", "Streamed after Suspense boundary"] } },
  { id: "static-mix", connections: 32, reqs: assets.map((a) => ({ path: a.path, expect_body: [] })), req: { path: assets[0]?.path } },
  { id: "range", connections: 32, req: { path: chunk.path, headers: [["range", "bytes=1000-17383"]], expect_status: 206, expect_body: [] } },
  { id: "conditional", connections: 32, conditional: ["/", chunk.path], req: { path: "/" } },
];
const pprWorkloads = [
  { id: "ppr-streaming", connections: 32, req: { path: "/profile?key=bench", headers: [["accept", "text/html"]], expect_body: ["</html>"] } },
  { id: "cc-prerendered", connections: 32, req: { path: "/cached", headers: [["accept", "text/html"]], expect_body: ["</html>"] } },
];
const workloads = (ppr ? pprWorkloads : appWorkloads).filter((w) => !args.only || String(args.only).split(",").includes(w.id));

const configs = [
  { label: "base-identity", kind: "nextrs", bin: args.base, gzip: false },
  { label: "base-gzip", kind: "nextrs", bin: args.base, gzip: true },
  { label: "cand-identity", kind: "nextrs", bin: args.candidate, gzip: false },
  { label: "cand-gzip", kind: "nextrs", bin: args.candidate, gzip: true },
  { label: "cand-nocache-gzip", kind: "nextrs", bin: args.candidate, gzip: true, extra: ["--compression-cache", "0"] },
  { label: "cand-tiny-gzip", kind: "nextrs", bin: args.candidate, gzip: true, extra: ["--compression-cache", "1"] },
  { label: "next-identity", kind: "next", gzip: false },
  { label: "next-gzip", kind: "next", gzip: true },
].filter((c) => (c.kind === "next" || c.bin) && (!args.configs || String(args.configs).split(",").includes(c.label)));

const upstreamPort = 5391;
const noUpstream = args.upstream === "none";
const env = noUpstream ? {} : { NEXTRS_UPSTREAM: `http://127.0.0.1:${upstreamPort}` };
const nextrsArgs = noUpstream ? [] : ["--js-env", "NEXTRS_UPSTREAM", "--net-allow", env.NEXTRS_UPSTREAM, "--net-allow-addr", "127.0.0.1"];

async function specFor(w, gzip, port) {
  const enc = ["accept-encoding", gzip ? "gzip" : "identity"];
  let requests;
  if (w.conditional) {
    // Revalidation with the validator this server and encoding hand out.
    requests = [];
    for (const path of w.conditional) {
      const r = await L.request(port, { path, headers: { "accept-encoding": enc[1] } });
      requests.push({ path, headers: [enc, ["if-none-match", r.headers.etag]], expect_status: 304, expect_body: [] });
    }
  } else {
    requests = (w.reqs ?? [w.req]).map((req) => ({ ...req, headers: [...(req.headers ?? []), enc], expect_body: gzip ? [] : req.expect_body }));
  }
  const file = join(out, `spec-${w.id}-${gzip ? "gzip" : "identity"}.json`);
  writeFileSync(file, JSON.stringify({ requests }));
  return file;
}

/** One response, decoded: what the gzip run cannot check per request. */
async function oracle(port, w, gzip) {
  const headers = Object.fromEntries([...(w.req.headers ?? []), ["accept-encoding", gzip ? "gzip" : "identity"]]);
  const r = await L.request(port, { path: w.req.path, headers });
  const encoding = r.headers["content-encoding"] ?? "identity";
  const body = encoding === "gzip" ? gunzipSync(r.body) : r.body;
  const text = body.toString("latin1");
  const missing = (w.req.expect_body ?? []).filter((s) => !text.includes(s));
  return { status: r.status, encoding, wire_bytes: r.body.length, decoded_bytes: body.length, missing, vary: r.headers.vary ?? null };
}

async function start(c, port) {
  return L.startServer({
    kind: c.kind, node, nextrs: c.bin, project, port, metricsPort: port + 100,
    cpus: serverCpus, env, extraArgs: [...nextrsArgs, ...(c.extra ?? [])], readyPath: "/api/health",
  });
}

const results = [];
async function benchmark() {
  const r = L.rng(Number(args.seed ?? 1));
  let port = 5600;
  for (let round = 0; round < rounds; round++) {
    for (const c of L.shuffle([...configs], r)) {
      port += 2;
      const s = await start(c, port);
      try {
        for (const w of workloads) {
          const o = await oracle(port, w, c.gzip);
          const spec = await specFor(w, c.gzip, port);
          await L.loadgen(loadgenBin, { cpus: loadCpus, target: `127.0.0.1:${port}`, spec, connections: w.connections, duration: warmup });
          const a = L.sample(s.pid);
          const stopMem = L.memorySampler(s.pid, 250);
          const t0 = performance.now();
          const lg = await L.loadgen(loadgenBin, { cpus: loadCpus, target: `127.0.0.1:${port}`, spec, connections: w.connections, duration });
          const elapsed = (performance.now() - t0) / 1000;
          const mem = stopMem();
          const b = L.sample(s.pid);
          const cpu = b.cpu_user_s + b.cpu_sys_s - (a.cpu_user_s + a.cpu_sys_s);
          const cache = c.kind === "nextrs" ? (await L.metrics(s.metricsPort))?.compression_cache ?? null : null;
          const row = {
            round, config: c.label, workload: w.id, connections: w.connections, compression_cache: cache,
            oracle: o, rps: lg.achieved_rps, failures: failures(lg), loadgen: lg,
            cpu_s: +cpu.toFixed(3), cpu_util: +(cpu / elapsed).toFixed(3),
            cpu_ms_per_req: lg.completed ? +((cpu * 1000) / lg.completed).toFixed(4) : null,
            peak_rss_mib: mem.peak.rss_mib, end_rss_mib: b.rss_mib,
          };
          results.push(row);
          const lat = lg.latency ?? {};
          console.log(`${c.label.padEnd(14)} ${w.id.padEnd(13)} r${round} rps=${lg.achieved_rps.toFixed(0)} p50=${lat.p50_ms} p99=${lat.p99_ms} ttfb50=${lg.ttfb?.p50_ms} cpu=${row.cpu_util} bytes/resp=${lg.completed ? Math.round(lg.bytes_received / lg.completed) : "-"} fail=${failures(lg)} enc=${o.encoding} missing=${o.missing.length}`);
          writeFileSync(join(out, "results.json"), JSON.stringify({ args, chunk, results }, null, 1));
        }
      } finally {
        await L.stopServer(s);
      }
    }
  }
}

/** First and second gzip request for each cacheable response, fresh server. */
async function cold() {
  const targets = [
    ...assets.filter((a) => a.size >= 1024).map((a) => ({ path: a.path, headers: {} })),
    ...(ppr ? ["/cached"] : ["/", "/no-such-page"]).map((path) => ({ path, headers: {} })),
    ...(ppr ? [] : [{ path: "/?_rsc", headers: { rsc: "1" } }]),
  ];
  const which = [["cand-gzip", args.candidate, []], ["cand-nocache-gzip", args.candidate, ["--compression-cache", "0"]], ["base-gzip", args.base, []]].filter(([, b]) => b);
  const rows = [];
  let port = 5800;
  const r = L.rng(Number(args.seed ?? 3));
  for (let round = 0; round < rounds; round++) {
    for (const [label, bin, extra] of L.shuffle([...which], r)) {
      port += 2;
      const s = await start({ kind: "nextrs", bin, extra }, port);
      try {
        for (const pass of ["first", "second", "third"]) {
          const a = L.sample(s.pid);
          const lat = [];
          let wire = 0;
          for (const t of targets) {
            const t0 = performance.now();
            const res = await L.request(port, { path: t.path, headers: { ...t.headers, "accept-encoding": "gzip" } });
            lat.push(performance.now() - t0);
            wire += res.body.length;
            if (res.headers["content-encoding"] !== "gzip") throw new Error(`${label} ${t.path}: not gzip`);
          }
          const b = L.sample(s.pid);
          const cache = (await L.metrics(s.metricsPort))?.compression_cache ?? null;
          const sorted = [...lat].sort((x, y) => x - y);
          const row = {
            round, config: label, pass, responses: targets.length, wire_bytes: wire,
            total_ms: +lat.reduce((x, y) => x + y, 0).toFixed(2),
            p50_ms: +sorted[Math.floor(sorted.length / 2)].toFixed(3), max_ms: +sorted.at(-1).toFixed(3),
            cpu_ms: +((b.cpu_user_s + b.cpu_sys_s - a.cpu_user_s - a.cpu_sys_s) * 1000).toFixed(1),
            compression_cache: cache,
          };
          rows.push(row);
          console.log(JSON.stringify(row));
        }
      } finally {
        await L.stopServer(s);
      }
    }
  }
  writeFileSync(join(out, "cold.json"), JSON.stringify({ targets, rows }, null, 1));
}

async function soak() {
  const minutes = Number(args.minutes ?? 10);
  const gzip = (args.encoding ?? "gzip") === "gzip";
  // The rc.2 mix (as before), plus with --soak-mix full: every static
  // chunk, ranges and revalidations, so the cache sees its whole key space.
  const base = ["prerendered", "ssr", "rsc", "json-api", "large-static"];
  const extra = args["soak-mix"] === "full" ? ["static-mix", "range"] : [];
  const mixed = workloads.filter((w) => [...base, ...extra].includes(w.id));
  const reqs = [];
  for (const w of [...mixed, workloads.find((w) => w.id === "streaming-64")].filter(Boolean)) {
    for (const req of w.reqs ?? [w.req]) {
      reqs.push({ ...req, kind: w.id, headers: [...(req.headers ?? []), ["accept-encoding", gzip ? "gzip" : "identity"]], expect_body: gzip ? [] : req.expect_body });
    }
  }
  const spec = join(out, `soak-spec-${gzip ? "gzip" : "identity"}.json`);
  writeFileSync(spec, JSON.stringify({ requests: reqs }));
  const which = [["candidate", args.candidate], ["base", args.base]].filter(([, b]) => b);
  let port = 5700;
  for (const [label, bin] of which) {
    port += 2;
    const s = await start({ kind: "nextrs", bin, extra: label === "candidate" && args["soak-cache"] ? ["--compression-cache", String(args["soak-cache"])] : [] }, port);
    const samples = [];
    const t0 = Date.now();
    const timer = setInterval(async () => {
      const m = L.sample(s.pid);
      const row = { t_s: Math.round((Date.now() - t0) / 1000), rss_mib: m.rss_mib, anon_mib: m.anon_mib, threads: m.threads, cpu_s: +(m.cpu_user_s + m.cpu_sys_s).toFixed(2) };
      samples.push(row);
      if (label === "candidate") row.compression_cache = (await L.metrics(s.metricsPort))?.compression_cache ?? null;
    }, 5000);
    let lg;
    try {
      lg = await L.loadgen(loadgenBin, { cpus: loadCpus, target: `127.0.0.1:${port}`, spec, connections: 64, duration: minutes * 60, warmup: 10 });
    } finally {
      clearInterval(timer);
      await L.stopServer(s);
    }
    const third = Math.max(1, Math.floor(samples.length / 3));
    const avg = (xs) => xs.reduce((a, b) => a + b.rss_mib, 0) / xs.length;
    const summary = {
      label, gzip, minutes, completed: lg.completed, failures: failures(lg), rps: lg.achieved_rps,
      latency: lg.latency, rss_first_third_mib: +avg(samples.slice(0, third)).toFixed(1),
      rss_last_third_mib: +avg(samples.slice(-third)).toFixed(1),
      rss_max_mib: Math.max(...samples.map((x) => x.rss_mib)),
    };
    console.log(JSON.stringify(summary));
    writeFileSync(join(out, `soak-${label}-${gzip ? "gzip" : "identity"}.json`), JSON.stringify({ summary, loadgen: lg, samples }, null, 1));
  }
}

const environment = L.environment({ node, nextrs: args.candidate, root: resolve("."), loadgenBin, serverCpus, loadCpus });
writeFileSync(join(out, "environment.json"), JSON.stringify({ environment, chunk, project, configs: configs.map((c) => ({ label: c.label, bin: c.bin ?? "next start" })) }, null, 1));
if (args.soak) await soak();
else if (args.cold) await cold();
else await benchmark();
