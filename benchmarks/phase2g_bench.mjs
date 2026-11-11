// Phase 2G benchmarks: next start, the Phase 2F NextRS binary and this tree,
// on the Pages Router (regular and deployment-adapter builds), native i18n,
// a hybrid build and images under concurrent SSR. Stdlib only: a keep-alive
// HTTP load generator in this process (ab is not assumed).
//
//   node benchmarks/phase2g_bench.mjs --nextrs-2f PATH [--out benchmarks/results]
//        [--requests 3000] [--concurrency 32] [--repeats 3] [--warmup 300] [--only group,...]
//
// With --k6 PATH, each measurement is run by k6 (benchmarks/phase2g_k6.js:
// shared iterations, statuses checked, no redirects followed); warm-ups and
// the background SSR load stay on the built-in generator.
//
// One server runs at a time. Each path's status is checked before it is
// measured. Per measurement: throughput, latency p50/p95/p99, errors, server
// CPU per request (/proc, the server and its children), RSS, and for NextRS
// executor requests per HTTP request, queue time and post-GC heap
// (/metrics.json). Load average is recorded before and after, and the
// spread of the repeats is reported: compare only rows measured in the same
// run.
//
// Fixtures must be built: next-pages (.next and .next-adapter),
// next-pages-i18n (.next), next-hybrid (.next), next-soak-mixed (.next),
// with NEXTRS_UPSTREAM at request time (this script starts the upstream).
import http from "node:http";
import { spawn } from "node:child_process";
import { readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { join, resolve } from "node:path";
import os from "node:os";

const args = Object.fromEntries(process.argv.slice(2).reduce((p, a, i, all) => (a.startsWith("--") && p.push([a.slice(2), all[i + 1]]), p), []));
const ROOT = resolve(new URL("..", import.meta.url).pathname);
const N = Number(args.requests ?? 3000);
const C = Number(args.concurrency ?? 32);
const REPEATS = Number(args.repeats ?? 3);
const WARMUP = Number(args.warmup ?? 300);
const only = args.only ? new Set(args.only.split(",")) : null;
const nextrs = join(ROOT, "target/release/nextrs");
const nextrs2f = args["nextrs-2f"] ? resolve(args["nextrs-2f"]) : null;
const k6 = args.k6 ? resolve(args.k6) : null;
let nextPort = 5980;
const port = () => nextPort++;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function request(p, { method = "GET", path, headers = {}, body }, agent) {
  return new Promise((resolveReq) => {
    const t0 = process.hrtime.bigint();
    const req = http.request({ host: "127.0.0.1", port: p, method, path, headers: { "accept-encoding": "identity", ...headers }, agent }, (res) => {
      res.on("data", () => {});
      res.on("end", () => resolveReq({ status: res.statusCode, ms: Number(process.hrtime.bigint() - t0) / 1e6 }));
    });
    req.on("error", () => resolveReq({ status: 0, ms: Number(process.hrtime.bigint() - t0) / 1e6 }));
    req.setTimeout(60000, () => req.destroy());
    req.end(body);
  });
}

// One measurement by k6: throughput, latency and status errors from its
// summary.
async function k6Load(p, spec, n, c) {
  const summary = join(os.tmpdir(), `phase2g-k6-${process.pid}-${p}.json`);
  const env = {
    ...process.env,
    URL: `http://127.0.0.1:${p}${spec.path}`,
    METHOD: spec.method ?? "GET",
    HEADERS: JSON.stringify(spec.headers ?? {}),
    BODY: spec.body ?? "",
    EXPECT: String(spec.expect),
    VUS: String(c),
    ITERATIONS: String(n),
    SUMMARY: summary,
  };
  await new Promise((done, fail) => {
    const child = spawn(k6, ["run", "--quiet", "--no-usage-report", join(ROOT, "benchmarks/phase2g_k6.js")], { env, stdio: ["ignore", "ignore", "inherit"] });
    child.on("exit", (code) => (code === 0 || code === 99 ? done() : fail(new Error(`k6 exited ${code}`))));
  });
  const m = JSON.parse(readFileSync(summary, "utf8")).metrics;
  const d = m.http_req_duration.values;
  const status = m.checks?.values ?? {};
  return { rps: +m.http_reqs.values.rate.toFixed(1), p50: +d.med.toFixed(2), p95: +d["p(95)"].toFixed(2), p99: +d["p(99)"].toFixed(2), errors: status.fails ?? 0 };
}

async function load(p, spec, n, c) {
  const agent = new http.Agent({ keepAlive: true, maxSockets: c });
  const lat = [];
  let errors = 0;
  let next = 0;
  const t0 = Date.now();
  await Promise.all(
    Array.from({ length: c }, async () => {
      while (next < n) {
        next++;
        const r = await request(p, spec, agent);
        lat.push(r.ms);
        if (r.status !== spec.expect) errors++;
      }
    }),
  );
  const seconds = (Date.now() - t0) / 1000;
  agent.destroy();
  lat.sort((a, b) => a - b);
  const q = (x) => +lat[Math.min(lat.length - 1, Math.floor(x * lat.length))].toFixed(2);
  return { rps: +(n / seconds).toFixed(1), p50: q(0.5), p95: q(0.95), p99: q(0.99), errors };
}

// CPU seconds of a process and its descendants.
function treeCpu(pid) {
  const children = new Map();
  for (const d of readdirSync("/proc")) {
    if (!/^\d+$/.test(d)) continue;
    try {
      const f = readFileSync(`/proc/${d}/stat`, "utf8").split(") ")[1].split(" ");
      const ppid = Number(f[1]);
      if (!children.has(ppid)) children.set(ppid, []);
      children.get(ppid).push(Number(d));
    } catch {}
  }
  let total = 0;
  let rss = 0;
  const walk = (p) => {
    try {
      const f = readFileSync(`/proc/${p}/stat`, "utf8").split(") ")[1].split(" ");
      total += (Number(f[11]) + Number(f[12])) / 100;
      rss += Number(/VmRSS:\s+(\d+)/.exec(readFileSync(`/proc/${p}/status`, "utf8"))?.[1] ?? 0) / 1024;
    } catch {}
    for (const c of children.get(p) ?? []) walk(c);
  };
  walk(pid);
  return { cpu: total, rssMiB: +rss.toFixed(1) };
}

const metrics = (mport) =>
  new Promise((r) => {
    if (!mport) return r(null);
    http.get({ host: "127.0.0.1", port: mport, path: "/metrics.json" }, (res) => {
      let b = "";
      res.on("data", (c) => (b += c));
      res.on("end", () => { try { r(JSON.parse(b)); } catch { r(null); } });
    }).on("error", () => r(null));
  });
const executorTotals = (m) => {
  if (!m?.executors) return null;
  return { requests: m.requests_completed, queueUs: m.queue_wait_seconds_total * 1e6, heapMiB: +(m.executors.reduce((a, e) => a + e.heap_used_after_gc_bytes, 0) / 1048576).toFixed(1) };
};

async function startServer(kind, project, dist, env, isolates) {
  const p = port();
  const mport = kind === "next-start" ? null : port();
  let child;
  if (kind === "next-start") {
    child = spawn(process.execPath, [join(project, "node_modules/next/dist/bin/next"), "start", "-H", "0.0.0.0", "-p", String(p)], { cwd: project, env, stdio: "ignore", detached: true });
  } else {
    const binary = kind.startsWith("2f") ? nextrs2f : nextrs;
    child = spawn(binary, ["start", join(project, dist), "--port", String(p), "--js-runtime", "embedded-v8", "--isolates", String(isolates), "--metrics-addr", `127.0.0.1:${mport}`,
      "--js-env", "NEXTRS_UPSTREAM", "--net-allow", env.NEXTRS_UPSTREAM, "--net-allow", "http://127.0.0.1:4317", "--net-allow-addr", "127.0.0.1"], { env, stdio: "ignore", detached: true });
  }
  for (let i = 0; i < 240; i++) {
    const r = await request(p, { path: "/" });
    if (r.status) return { child, port: p, mport };
    await sleep(250);
  }
  throw new Error(`${kind} did not start`);
}
async function stopServer(s) {
  try { process.kill(-s.child.pid, "SIGTERM"); } catch { try { s.child.kill("SIGTERM"); } catch {} }
  for (let i = 0; i < 80; i++) {
    if (!(await request(s.port, { path: "/" })).status) return;
    await sleep(250);
  }
}

const loadavg = () => Number(readFileSync("/proc/loadavg", "utf8").split(" ")[0]);

async function group(name, project, dist, servers, paths, env) {
  if (only && !only.has(name)) return [];
  const rows = [];
  for (const [kind, isolates] of servers) {
    if (kind.startsWith("2f") && !nextrs2f) continue;
    const variantEnv = { ...env, ...(dist === ".next-adapter" ? { NEXTRS_VARIANT: "adapter" } : {}) };
    const s = await startServer(kind, project, dist, variantEnv, isolates);
    try {
      for (const spec of paths) {
        const check = await request(s.port, spec);
        if (check.status !== spec.expect) {
          rows.push({ group: name, path: spec.name, server: kind, error: `status ${check.status}, expected ${spec.expect}` });
          continue;
        }
        await load(s.port, spec, WARMUP, Math.min(C, 8));
        const runs = [];
        for (let r = 0; r < REPEATS; r++) {
          const before = treeCpu(s.child.pid);
          const m0 = executorTotals(await metrics(s.mport));
          const l0 = loadavg();
          const res = spec.background ? await withBackground(s.port, spec) : await (k6 ? k6Load : load)(s.port, spec, N, C);
          const after = treeCpu(s.child.pid);
          const m1 = executorTotals(await metrics(s.mport));
          runs.push({
            ...res,
            cpuMsPerReq: +(((after.cpu - before.cpu) * 1000) / N).toFixed(3),
            rssMiB: after.rssMiB,
            loadBefore: l0,
            loadAfter: loadavg(),
            jsPerReq: m0 && m1 ? +((m1.requests - m0.requests) / N).toFixed(2) : null,
            queueMsPerReq: m0 && m1 ? +((m1.queueUs - m0.queueUs) / 1000 / Math.max(1, m1.requests - m0.requests)).toFixed(3) : null,
            heapMiB: m1?.heapMiB ?? null,
          });
        }
        const rps = runs.map((r) => r.rps);
        const median = (a) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];
        const best = runs.find((r) => r.rps === median(rps));
        rows.push({ group: name, path: spec.name, server: kind, ...best, rpsMin: Math.min(...rps), rpsMax: Math.max(...rps), errors: runs.reduce((a, r) => a + r.errors, 0) });
        console.log(`${name.padEnd(10)} ${spec.name.padEnd(22)} ${kind.padEnd(10)} ${String(best.rps).padStart(8)} rps  p50 ${best.p50}  p99 ${best.p99}  cpu ${best.cpuMsPerReq} ms/req  [${Math.min(...rps)}-${Math.max(...rps)}]`);
      }
    } finally {
      await stopServer(s);
    }
  }
  return rows;
}

// Images under concurrent SSR: the image path measured while another
// generator keeps SSR busy.
async function withBackground(p, spec) {
  let stop = false;
  const agent = new http.Agent({ keepAlive: true, maxSockets: 8 });
  const bg = Promise.all(Array.from({ length: 8 }, async () => { while (!stop) await request(p, spec.background, agent); }));
  const res = await (k6 ? k6Load : load)(p, spec, N, C);
  stop = true;
  await bg;
  agent.destroy();
  return res;
}

const upstreamPort = port();
const upstream = spawn(process.execPath, [join(ROOT, "tests/compat/upstream.mjs"), "--port", String(upstreamPort)], { stdio: "ignore" });
const { startUpstream } = await import(join(ROOT, "tests/compat/image-upstream.mjs"));
const images = await startUpstream(join(ROOT, "fixtures/next-soak-mixed/public/photos"), 4317);
await sleep(500);
const env = { ...process.env, NEXT_TELEMETRY_DISABLED: "1", NEXTRS_UPSTREAM: `http://127.0.0.1:${upstreamPort}`, NEXTRS_IMAGE_UPSTREAM_PORT: "4317" };
const pagesId = readFileSync(join(ROOT, "fixtures/next-pages/.next/BUILD_ID"), "utf8").trim();
const adapterId = readFileSync(join(ROOT, "fixtures/next-pages/.next-adapter/BUILD_ID"), "utf8").trim();
const i18nId = readFileSync(join(ROOT, "fixtures/next-pages-i18n/.next/BUILD_ID"), "utf8").trim();
const asset = /src="(\/_next\/static\/chunks\/[^"]+\.js)"/.exec(readFileSync(join(ROOT, "fixtures/next-pages/.next/server/pages/about.html"), "utf8"))?.[1];
const body = JSON.stringify({ bench: true, items: Array.from({ length: 32 }, (_, i) => i) });
const P = (name, path, expect = 200, extra = {}) => ({ name, path, expect, ...extra });
const all = { "next-start": 1, "2f": 1, "2g": 1, "2g-i4": 4 };
const pick = (...names) => names.map((n) => [n, all[n]]);
const rows = [];
try {
  rows.push(...(await group("pages", join(ROOT, "fixtures/next-pages"), ".next", pick("next-start", "2f", "2g", "2g-i4"), [
    P("static page", "/about"),
    P("static asset", asset),
    P("ssr page", "/books/42?ref=bench"),
    P("ssr data", `/_next/data/${pagesId}/books/42.json`),
    P("ssg page (hit)", "/ssg/static"),
    P("ssg data (hit)", `/_next/data/${pagesId}/ssg/static.json`),
    P("isr page", "/isr"),
    P("api route", "/api/health"),
    P("api post json", "/api/echo", 200, { method: "POST", body, headers: { "content-type": "application/json" } }),
    P("proxy + ssr", "/protected/secret", 200, { headers: { cookie: "session=synthetic-valid-session; user=u-bench" } }),
  ], env)));
  rows.push(...(await group("adapter", join(ROOT, "fixtures/next-pages"), ".next-adapter", pick("next-start", "2g", "2g-i4"), [
    P("ssr page", "/books/42?ref=bench"),
    P("ssg page (hit)", "/ssg/static"),
    P("ssg data (hit)", `/_next/data/${adapterId}/ssg/static.json`),
  ], env)));
  rows.push(...(await group("i18n", join(ROOT, "fixtures/next-pages-i18n"), ".next", pick("next-start", "2g", "2g-i4"), [
    P("locale redirect", "/", 307, { headers: { "accept-language": "fr" } }),
    P("locale static", "/fr/about"),
    P("locale ssr", "/fr/ssr?x=1"),
    P("locale ssg (hit)", "/fr/posts/hello"),
    P("locale data", `/_next/data/${i18nId}/fr/posts/hello.json`),
    P("locale isr", "/fr/isr"),
    P("default locale", "/about"),
  ], env)));
  rows.push(...(await group("hybrid", join(ROOT, "fixtures/next-hybrid"), ".next", pick("next-start", "2f", "2g"), [
    P("app page", "/app-items/1"),
    P("pages page", "/pages-items/1"),
  ], env)));
  rows.push(...(await group("images", join(ROOT, "fixtures/next-soak-mixed"), ".next", pick("next-start", "2f", "2g", "2g-i4"), [
    P("image hit + ssr", "/_next/image?url=%2Fphotos%2Fphoto.jpg&w=640&q=75", 200, { headers: { accept: "image/webp" }, background: { path: "/p/ssr/1" } }),
    P("ssr alone", "/p/ssr/1"),
  ], env)));
} finally {
  upstream.kill();
  await images.close();
}
const out = resolve(args.out ?? join(ROOT, "benchmarks/results"));
mkdirSync(out, { recursive: true });
const file = join(out, `phase2g-${new Date().toISOString().replace(/[:.]/g, "").slice(0, 15)}.json`);
writeFileSync(file, JSON.stringify({ generator: k6 ? "k6 v2.3.0" : "built-in", environment: { node: process.version, cpus: os.cpus().length, cpu: os.cpus()[0]?.model, memGiB: +(os.totalmem() / 2 ** 30).toFixed(1), kernel: os.release() }, requests: N, concurrency: C, repeats: REPEATS, rows }, null, 2));
console.log(`wrote ${file}`);
