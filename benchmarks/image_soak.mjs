// Sustained image-heavy mixed traffic against NextRS on fixtures/next-image
// (its soak routes: /soak/ssr, /soak/isr, /soak/action, /soak/proxied/*,
// /soak/spin), with a local image upstream, sampling memory, CPU and the
// optimizer's counters.
//
//   node benchmarks/image_soak.mjs --target URL --metrics URL --project fixtures/next-image
//     [--minutes 60] [--concurrency 24] [--upstream-port 4317] [--inject-every 300]
//     [--inject-ms 3000] [--cache-dir DIR --readonly-at 20 --readonly-for 5]
//     [--sample-every 10] [--json out.json]
//
// Rotation (weighted): image cache hits (a small hot set) and misses (a
// large key space over sources, widths, qualities and formats), 24 MP
// resizes, remote JPEG, AVIF output, clients that leave mid-transformation,
// invalid images, a slow upstream, an unavailable upstream; SSR whose answer
// must carry the request's own token, a Server Action echoing its token,
// ISR, and requests through proxy.ts. Every --inject-every seconds a request
// spins an isolate past the server's --js-stall-timeout (a restart); with
// --cache-dir the directory is made read-only for a while (a full or
// failing disk: images must still be served).
import http from "node:http";
import { randomUUID } from "node:crypto";
import { chmodSync, readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";
import { join } from "node:path";
import { startUpstream } from "../tests/compat/image-upstream.mjs";

const args = Object.fromEntries(process.argv.slice(2).reduce((p, a, i, all) => (a.startsWith("--") && p.push([a.slice(2), all[i + 1]]), p), []));
const target = new URL(args.target);
const metricsUrl = args.metrics;
const project = args.project ?? "fixtures/next-image";
const minutes = Number(args.minutes ?? 60);
const concurrency = Number(args.concurrency ?? 24);
const upstreamPort = Number(args["upstream-port"] ?? 4317);
const injectEvery = Number(args["inject-every"] ?? 0);
const injectMs = Number(args["inject-ms"] ?? 3000);
const sampleEvery = Number(args["sample-every"] ?? 10);
const pid = Number(/pid=(\d+)/.exec(execSync(`ss -ltnpH 'sport = :${target.port}'`).toString())[1]);
const agent = new http.Agent({ keepAlive: true, maxSockets: concurrency * 2 });

const manifest = JSON.parse(readFileSync(join(project, ".next/server/server-reference-manifest.json"), "utf8"));
const echoId = Object.entries(manifest.node).find(([, v]) => v.exportedName === "soakEcho")?.[0];
if (!echoId) throw new Error("soakEcho action not found in the build");

function send({ method = "GET", path, headers = {}, body, abortAfterMs }) {
  return new Promise((resolve) => {
    const t0 = process.hrtime.bigint();
    const ms = () => Number(process.hrtime.bigint() - t0) / 1e6;
    const req = http.request({ host: target.hostname, port: target.port, method, path, headers, agent: abortAfterMs ? false : agent }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks), ms: ms() }));
      res.on("error", () => resolve({ status: 0, error: "body", ms: ms() }));
    });
    req.setTimeout(30000, () => req.destroy(new Error("timeout")));
    req.on("error", (e) => resolve({ status: 0, error: e.code ?? e.message, ms: ms() }));
    if (abortAfterMs) setTimeout(() => req.destroy(new Error("aborted by client")), abortAfterMs);
    req.end(body);
  });
}

const WIDTHS = [32, 48, 64, 96, 128, 256, 384, 640, 750, 828, 1080, 1200, 1920];
const Q = [50, 75, 90];
const ACCEPT = { webp: "image/webp,*/*", avif: "image/avif,image/webp,*/*", any: "*/*" };
const SOURCES = ["photo.jpg", "odd.jpg", "p3.jpg", "rotated.jpg", "alpha.png", "photo.webp", "alpha.webp", "still.gif", "lzw.tiff", "small.png"];
const pick = (a) => a[Math.floor(Math.random() * a.length)];
const img = (url, w, q) => `/_next/image?url=${encodeURIComponent(url)}&w=${w}&q=${q}`;
const HOT = SOURCES.slice(0, 4).flatMap((s) => [256, 640, 1080].map((w) => [img(`/photos/${s}`, w, 75), ACCEPT.webp]));
const remote = (path) => `http://127.0.0.1:${upstreamPort}${path}`;

const kinds = {
  "img-hit": { weight: 30, make: () => { const [path, accept] = pick(HOT); return { path, headers: { accept } }; }, ok: [200] },
  "img-miss": { weight: 16, make: () => ({ path: img(`/photos/${pick(SOURCES)}`, pick(WIDTHS), pick(Q)), headers: { accept: pick([ACCEPT.webp, ACCEPT.webp, ACCEPT.any]) } }), ok: [200] },
  "img-large": { weight: 3, make: () => ({ path: img(`/photos/${pick(["large.jpg", "large-progressive.jpg"])}`, pick([256, 640, 1080, 1920, 3840]), pick(Q)), headers: { accept: ACCEPT.webp } }), ok: [200] },
  "img-remote": { weight: 8, make: () => ({ path: img(remote(`/img/photo.jpg?n=${Math.floor(Math.random() * 40)}`), pick(WIDTHS), pick(Q)), headers: { accept: ACCEPT.webp } }), ok: [200] },
  "img-avif": { weight: 4, make: () => ({ path: img(`/photos/${pick(["photo.jpg", "odd.jpg", "alpha.png"])}`, pick([32, 64, 128, 256, 384]), pick(Q)), headers: { accept: ACCEPT.avif } }), ok: [200] },
  "img-abort": { weight: 3, make: () => ({ path: img(`/photos/${pick(SOURCES)}`, pick(WIDTHS), pick(Q)), headers: { accept: ACCEPT.any }, abortAfterMs: 5 + Math.floor(Math.random() * 60) }), ok: [200, 0] },
  "img-invalid": { weight: 2, make: () => pick([{ path: img("/photos/corrupt.jpg", 256, 75), headers: { accept: ACCEPT.webp }, expect: 200 }, { path: img("/photos/not-an-image.png", 256, 75), headers: { accept: ACCEPT.webp }, expect: 400 }]), ok: [200, 400] },
  "img-slow-upstream": { weight: 1, make: () => ({ path: img(remote(`/img/trickle.jpg?n=${Math.floor(Math.random() * 20)}`), pick([256, 640]), 75), headers: { accept: ACCEPT.webp } }), ok: [200] },
  "img-unavailable": { weight: 1, make: () => ({ path: img(remote(`/img/server-error.jpg?n=${Math.floor(Math.random() * 5)}`), 256, 75), headers: { accept: ACCEPT.webp } }), ok: [503] },
  ssr: { weight: 14, make: () => { const token = randomUUID(); return { path: "/soak/ssr", headers: { "x-soak-token": token, accept: "text/html" }, token }; }, ok: [200], identity: true },
  action: {
    weight: 6,
    make: () => {
      const token = randomUUID();
      return { method: "POST", path: "/soak/action", headers: { "next-action": echoId, accept: "text/x-component", "content-type": "text/plain;charset=UTF-8", origin: target.origin }, body: JSON.stringify([token]), token };
    },
    ok: [200],
    identity: true,
  },
  isr: { weight: 6, make: () => ({ path: "/soak/isr", headers: { accept: "text/html" } }), ok: [200] },
  proxy: { weight: 6, make: () => { const token = randomUUID(); const slug = `s${Math.floor(Math.random() * 50)}`; return { path: `/soak/proxied/${slug}`, headers: { "x-soak-token": token, accept: "text/html" }, token: `${slug}:${token}` }; }, ok: [200], identity: true },
};
const table = Object.entries(kinds).flatMap(([name, k]) => Array(k.weight).fill(name));

const stats = Object.fromEntries(Object.keys(kinds).map((k) => [k, { count: 0, errors: 0, statuses: {}, ms: [], cache: {} }]));
const totals = { requests: 0, unexpected: 0, rejected503: 0, lostToRestart: 0, rejectedDuringRestart: 0, identityLeaks: 0, aborted: 0, injections: 0 };
const errors = [];
let restartWindowUntil = 0;

async function worker(deadline) {
  while (Date.now() < deadline) {
    const name = pick(table);
    const k = kinds[name];
    const r = k.make();
    const res = await send(r);
    const s = stats[name];
    s.count++;
    totals.requests++;
    s.statuses[res.status] = (s.statuses[res.status] ?? 0) + 1;
    if (s.ms.length < 200000) s.ms.push(res.ms);
    const cache = res.headers?.["x-nextjs-cache"];
    if (name.startsWith("img") && cache) s.cache[cache] = (s.cache[cache] ?? 0) + 1;
    if (r.abortAfterMs && res.status === 0) { totals.aborted++; continue; }
    const expected = r.expect !== undefined ? [r.expect] : k.ok;
    if (res.status === 503 && name.startsWith("img") && !expected.includes(503)) { totals.rejected503++; continue; }
    if (!expected.includes(res.status)) {
      // A dynamic request whose isolate was replaced by an injected stall.
      if (!name.startsWith("img") && Date.now() < restartWindowUntil) {
        if (res.status === 502 || res.status === 0 || res.status === 500) { totals.lostToRestart++; continue; }
        // The executor's bounded queue sheds load while its isolate is
        // stalled or being replaced.
        if (res.status === 503) { totals.rejectedDuringRestart++; continue; }
      }
      s.errors++;
      totals.unexpected++;
      if (errors.length < 50) errors.push({ at: new Date().toISOString(), kind: name, path: r.path, status: res.status, error: res.error, body: res.body?.toString().slice(0, 200) });
      continue;
    }
    if (k.identity && res.status === 200) {
      const body = res.body.toString();
      if (!body.includes(r.token)) {
        totals.identityLeaks++;
        if (errors.length < 50) errors.push({ at: new Date().toISOString(), kind: name, leak: true, body: body.slice(0, 300) });
      }
    }
  }
}

const json = (url) => new Promise((r) => http.get(url, (res) => { let b = ""; res.on("data", (c) => (b += c)); res.on("end", () => { try { r(JSON.parse(b)); } catch { r(null); } }); }).on("error", () => r(null)));
const proc = () => {
  const st = readFileSync(`/proc/${pid}/status`, "utf8");
  const kb = (k) => Number(new RegExp(`${k}:\\s+(\\d+)`).exec(st)?.[1] ?? 0) / 1024;
  const f = readFileSync(`/proc/${pid}/stat`, "utf8").split(") ")[1].split(" ");
  return { rssMiB: kb("VmRSS"), swapMiB: kb("VmSwap"), hwmMiB: kb("VmHWM"), cpuSeconds: (Number(f[11]) + Number(f[12])) / 100 };
};
const samples = [];
async function sample(t0) {
  const m = await json(`${metricsUrl}/metrics.json`);
  const p = proc();
  const loadavg = readFileSync("/proc/loadavg", "utf8").split(" ")[0];
  samples.push({
    t: Math.round((Date.now() - t0) / 1000),
    ...p,
    load: Number(loadavg),
    requests: totals.requests,
    malloc: m?.memory?.malloc,
    heapAfterGcMiB: m?.executors?.map((e) => +(e.heap_used_after_gc_bytes / 1048576).toFixed(1)),
    restarts: m?.executors?.reduce((a, e) => a + e.restarts, 0),
    images: m?.images && Object.fromEntries(["transforms", "transform_failures", "fallbacks", "overloaded", "cancelled", "coalesced", "memory_waits", "memory_rejected", "memory_reserved_bytes", "source_held_bytes", "source_rejected", "queued", "in_flight", "cache_memory_hits", "cache_disk_hits", "cache_misses", "cache_memory_bytes", "cache_disk_bytes", "cache_write_errors", "cache_corrupt", "decode_us", "resize_us", "encode_us"].map((k) => [k, m.images[k]])),
  });
}

const up = await startUpstream(join(project, "public/photos"), upstreamPort);
const t0 = Date.now();
const deadline = t0 + minutes * 60_000;
const sampler = setInterval(() => sample(t0), sampleEvery * 1000);
await sample(t0);
const injector = injectEvery
  ? setInterval(async () => {
      totals.injections++;
      restartWindowUntil = Date.now() + injectMs + 15000;
      await send({ path: `/soak/spin?ms=${injectMs}` });
    }, injectEvery * 1000)
  : null;
const readonlyAt = Number(args["readonly-at"] ?? 0), readonlyFor = Number(args["readonly-for"] ?? 5);
const disk = args["cache-dir"] && readonlyAt
  ? setTimeout(() => {
      // Entries are written to tmp/ and renamed into place.
      const dirs = [args["cache-dir"], join(args["cache-dir"], "tmp")];
      for (const d of dirs) chmodSync(d, 0o500);
      setTimeout(() => { for (const d of dirs) chmodSync(d, 0o700); }, readonlyFor * 60_000);
    }, readonlyAt * 60_000)
  : null;
await Promise.all(Array.from({ length: concurrency }, () => worker(deadline)));
clearInterval(sampler);
if (injector) clearInterval(injector);
if (disk) clearTimeout(disk);
await sample(t0);
await up.close();

const pct = (a, p) => { const s = [...a].sort((x, y) => x - y); return s.length ? +s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))].toFixed(1) : null; };
const kindsOut = Object.fromEntries(Object.entries(stats).map(([k, s]) => [k, { count: s.count, errors: s.errors, statuses: s.statuses, cache: s.cache, p50: pct(s.ms, 50), p95: pct(s.ms, 95), p99: pct(s.ms, 99) }]));
const all = Object.values(stats).flatMap((s) => s.ms);
const out = { minutes, concurrency, seconds: (Date.now() - t0) / 1000, totals, throughput: +(totals.requests / ((Date.now() - t0) / 1000)).toFixed(1), latency: { p50: pct(all, 50), p95: pct(all, 95), p99: pct(all, 99) }, kinds: kindsOut, errors, samples };
console.log(JSON.stringify({ ...out, samples: samples.length }, null, 1));
if (args.json) writeFileSync(args.json, JSON.stringify(out, null, 2));
process.exit(totals.unexpected || totals.identityLeaks ? 1 : 0);
