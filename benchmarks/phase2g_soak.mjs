// Phase 2G mixed soak: App Router, Pages Router and image traffic against one
// NextRS process on fixtures/next-soak-mixed, optionally locale traffic
// against a second NextRS process on fixtures/next-pages-i18n, with restart
// injection, sampling memory (RSS, anonymous RSS, swap, malloc, post-GC heap),
// CPU, image reservations and the shared cache.
//
//   node benchmarks/phase2g_soak.mjs --target URL --metrics URL --project fixtures/next-soak-mixed
//     --ledger URL [--i18n-target URL --i18n-metrics URL --i18n-dist DIR]
//     [--minutes 360] [--concurrency 24] [--upstream-port 4317] [--inject-every 300]
//     [--inject-ms 3000] [--sample-every 30] [--pid N] [--i18n-pid N] [--json out.json]
//
// --ledger is tests/compat/upstream.mjs (both servers fetch from it and must
// be allowed to): the Pages API mutation records each id there, outside
// every isolate, so a mutation that ran twice (or a lost one reported as
// done) is counted, not inferred.
//
// Every request is classified exactly once: ok; client abort (deliberate);
// overload rejection (503 from the image queue); lost to an injected
// restart (status 0/500/502 for a dynamic request inside a restart window);
// shed during a restart (503); or unexpected. Identity checks (a token or a
// user the request carried must come back, and only it) count leaks.
import http from "node:http";
import { randomUUID } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";
import { join } from "node:path";
import { startUpstream } from "../tests/compat/image-upstream.mjs";

const args = Object.fromEntries(process.argv.slice(2).reduce((p, a, i, all) => (a.startsWith("--") && p.push([a.slice(2), all[i + 1]]), p), []));
const target = new URL(args.target);
const i18nTarget = args["i18n-target"] ? new URL(args["i18n-target"]) : null;
const project = args.project ?? "fixtures/next-soak-mixed";
const ledger = args.ledger;
const minutes = Number(args.minutes ?? 60);
const concurrency = Number(args.concurrency ?? 24);
const upstreamPort = Number(args["upstream-port"] ?? 4317);
const injectEvery = Number(args["inject-every"] ?? 0);
const injectMs = Number(args["inject-ms"] ?? 3000);
const sampleEvery = Number(args["sample-every"] ?? 30);
const pidOf = (port) => Number(/pid=(\d+)/.exec(execSync(`ss -ltnpH 'sport = :${port}'`).toString())[1]);
// --pid / --i18n-pid when given (the wrapper knows them), else the
// listening process (ss).
const pid = Number(args.pid ?? 0) || pidOf(target.port);
const i18nPid = i18nTarget ? Number(args["i18n-pid"] ?? 0) || pidOf(i18nTarget.port) : null;
const agent = new http.Agent({ keepAlive: true, maxSockets: concurrency * 2 });

const dist = join(project, ".next");
const buildId = readFileSync(join(dist, "BUILD_ID"), "utf8").trim();
const i18nBuildId = i18nTarget ? readFileSync(join(args["i18n-dist"], "BUILD_ID"), "utf8").trim() : null;
const manifest = JSON.parse(readFileSync(join(dist, "server/server-reference-manifest.json"), "utf8"));
const echoId = Object.entries(manifest.node).find(([, v]) => v.exportedName === "soakEcho")?.[0];
if (!echoId) throw new Error("soakEcho action not found in the build");

function send({ method = "GET", path, headers = {}, body, abortAfterMs, to = target }) {
  return new Promise((resolve) => {
    const t0 = process.hrtime.bigint();
    const ms = () => Number(process.hrtime.bigint() - t0) / 1e6;
    const req = http.request({ host: to.hostname, port: to.port, method, path, headers, agent: abortAfterMs ? false : agent }, (res) => {
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

// Preview Mode cookies, minted once by the application itself.
const previewRes = await send({ path: "/api/p/preview?who=soak" });
const previewCookie = (previewRes.headers?.["set-cookie"] ?? []).map((c) => c.split(";")[0]).join("; ");
if (!previewCookie.includes("__prerender_bypass")) throw new Error("no preview cookies");

const WIDTHS = [32, 48, 64, 96, 128, 256, 384, 640, 750, 828, 1080, 1200, 1920];
const Q = [50, 75, 90];
const ACCEPT = { webp: "image/webp,*/*", avif: "image/avif,image/webp,*/*", any: "*/*" };
const SOURCES = ["photo.jpg", "odd.jpg", "p3.jpg", "rotated.jpg", "alpha.png", "photo.webp", "alpha.webp", "still.gif", "lzw.tiff", "small.png"];
const pick = (a) => a[Math.floor(Math.random() * a.length)];
const img = (url, w, q) => `/_next/image?url=${encodeURIComponent(url)}&w=${w}&q=${q}`;
const HOT = SOURCES.slice(0, 4).flatMap((s) => [256, 640, 1080].map((w) => [img(`/photos/${s}`, w, 75), ACCEPT.webp]));
const remote = (path) => `http://127.0.0.1:${upstreamPort}${path}`;
const mutations = new Map(); // id -> "done" | "lost"

const kinds = {
  // images (Phase 2E's mix)
  "img-hit": { weight: 22, make: () => { const [path, accept] = pick(HOT); return { path, headers: { accept } }; }, ok: [200] },
  "img-miss": { weight: 12, make: () => ({ path: img(`/photos/${pick(SOURCES)}`, pick(WIDTHS), pick(Q)), headers: { accept: pick([ACCEPT.webp, ACCEPT.any]) } }), ok: [200] },
  "img-large": { weight: 3, make: () => ({ path: img(`/photos/${pick(["large.jpg", "large-progressive.jpg"])}`, pick([256, 640, 1080, 1920, 3840]), pick(Q)), headers: { accept: ACCEPT.webp } }), ok: [200] },
  "img-remote": { weight: 6, make: () => ({ path: img(remote(`/img/photo.jpg?n=${Math.floor(Math.random() * 40)}`), pick(WIDTHS), pick(Q)), headers: { accept: ACCEPT.webp } }), ok: [200] },
  "img-avif": { weight: 3, make: () => ({ path: img(`/photos/${pick(["photo.jpg", "odd.jpg", "alpha.png"])}`, pick([32, 64, 128, 256]), pick(Q)), headers: { accept: ACCEPT.avif } }), ok: [200] },
  "img-abort": { weight: 2, make: () => ({ path: img(`/photos/${pick(SOURCES)}`, pick(WIDTHS), pick(Q)), headers: { accept: ACCEPT.any }, abortAfterMs: 5 + Math.floor(Math.random() * 60) }), ok: [200, 0] },
  // App Router
  "app-ssr": { weight: 8, make: () => { const token = randomUUID(); return { path: "/soak/ssr", headers: { "x-soak-token": token, accept: "text/html" }, token }; }, ok: [200], identity: true },
  "app-ssr-abort": { weight: 2, make: () => ({ path: "/soak/ssr", headers: { "x-soak-token": "abort", accept: "text/html" }, abortAfterMs: 2 + Math.floor(Math.random() * 20) }), ok: [200, 0] },
  "app-action": {
    weight: 4,
    make: () => {
      const token = randomUUID();
      return { method: "POST", path: "/soak/action", headers: { "next-action": echoId, accept: "text/x-component", "content-type": "text/plain;charset=UTF-8", origin: target.origin }, body: JSON.stringify([token]), token };
    },
    ok: [200],
    identity: true,
  },
  "app-isr": { weight: 3, make: () => ({ path: "/soak/isr", headers: { accept: "text/html" } }), ok: [200] },
  "app-proxy": { weight: 3, make: () => { const token = randomUUID(); const slug = `s${Math.floor(Math.random() * 50)}`; return { path: `/soak/proxied/${slug}`, headers: { "x-soak-token": token, accept: "text/html" }, token: `${slug}:${token}` }; }, ok: [200], identity: true },
  // Pages Router
  "pages-ssr": { weight: 7, make: () => { const token = randomUUID(); const id = Math.floor(Math.random() * 1000); return { path: `/p/ssr/${id}`, headers: { "x-soak-token": token }, token: `${id}:${token}` }; }, ok: [200], identity: true },
  "pages-data": { weight: 4, make: () => { const token = randomUUID(); const id = Math.floor(Math.random() * 1000); return { path: `/_next/data/${buildId}/p/ssr/${id}.json`, headers: { "x-soak-token": token }, token }; }, ok: [200], identity: true },
  "pages-ssg": { weight: 4, make: () => ({ path: "/p/ssg" }), ok: [200], expectText: '"when":"build"' },
  "pages-isr": { weight: 3, make: () => ({ path: "/p/isr" }), ok: [200] },
  "pages-api-get": { weight: 4, make: () => { const token = randomUUID(); return { path: "/api/p/echo", headers: { "x-soak-token": token }, token }; }, ok: [200], identity: true },
  "pages-api-post": { weight: 3, make: () => { const id = randomUUID(); return { method: "POST", path: `/api/p/mutate?id=${id}`, mutation: id }; }, ok: [200] },
  "pages-preview": { weight: 2, make: () => ({ path: "/p/preview", headers: { cookie: previewCookie } }), ok: [200], expectText: "true:soak" },
  "pages-preview-off": { weight: 2, make: () => ({ path: "/p/preview" }), ok: [200], expectText: "false:-" },
  "pages-secure-anon": { weight: 2, make: () => ({ path: "/p/secure" }), ok: [307], forbidText: "soak-secret" },
  "pages-secure-session": { weight: 1, make: () => ({ path: "/p/secure", headers: { cookie: "session=synthetic-valid-session" } }), ok: [200], expectText: "soak-secret" },
  "pages-revalidate": { weight: 1, make: () => ({ path: "/api/p/revalidate" }), ok: [200] },
};
if (i18nTarget) {
  const loc = (k) => ({ ...k, to: i18nTarget });
  Object.assign(kinds, {
    "locale-redirect": { weight: 2, make: () => loc({ path: "/", headers: { "accept-language": pick(["fr", "ja", "fr-FR,fr;q=0.9"]) } }), ok: [307] },
    "locale-ssr": { weight: 4, make: () => { const locale = pick(["fr", "ja", "en"]); const user = `u${randomUUID().slice(0, 8)}`; return loc({ path: `/${locale}/ssr`, headers: { cookie: `user=${user}` }, token: `user=${user}`, locale }); }, ok: [200], identity: true },
    "locale-data": { weight: 2, make: () => { const locale = pick(["fr", "ja"]); return loc({ path: `/_next/data/${i18nBuildId}/${locale}/posts/hello.json`, locale }); }, ok: [200] },
    "locale-404": { weight: 1, make: () => loc({ path: "/fr/nope" }), ok: [404], expectText: "Introuvable" },
  });
}
const table = Object.entries(kinds).flatMap(([name, k]) => Array(k.weight).fill(name));

const stats = Object.fromEntries(Object.keys(kinds).map((k) => [k, { count: 0, errors: 0, statuses: {}, ms: [], cache: {} }]));
const totals = { requests: 0, ok: 0, unexpected: 0, rejected503: 0, lostToRestart: 0, rejectedDuringRestart: 0, identityLeaks: 0, wrongContent: 0, aborted: 0, injections: 0 };
const errors = [];
let restartWindowUntil = 0;
const note = (entry) => errors.length < 80 && errors.push({ at: new Date().toISOString(), ...entry });

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
    if (cache) s.cache[cache] = (s.cache[cache] ?? 0) + 1;
    if (r.abortAfterMs && res.status === 0) { totals.aborted++; continue; }
    if (res.status === 503 && name.startsWith("img")) { totals.rejected503++; continue; }
    if (!k.ok.includes(res.status)) {
      // A dynamic request whose isolate was replaced by an injected stall.
      if (!name.startsWith("img") && Date.now() < restartWindowUntil) {
        if (res.status === 502 || res.status === 0 || res.status === 500) {
          totals.lostToRestart++;
          if (r.mutation) mutations.set(r.mutation, "lost");
          continue;
        }
        if (res.status === 503) {
          totals.rejectedDuringRestart++;
          if (r.mutation) mutations.set(r.mutation, "lost");
          continue;
        }
      }
      s.errors++;
      totals.unexpected++;
      if (r.mutation) mutations.set(r.mutation, "lost");
      note({ kind: name, path: r.path, status: res.status, error: res.error, body: res.body?.toString().slice(0, 200) });
      continue;
    }
    const body = res.body?.toString() ?? "";
    if (k.identity && res.status === 200 && !body.includes(r.token)) {
      totals.identityLeaks++;
      note({ kind: name, leak: true, body: body.slice(0, 300) });
      continue;
    }
    if (r.locale && res.status === 200 && !body.includes(`"locale":"${r.locale}"`) && !body.includes(`id="locale">${r.locale}<`)) {
      totals.identityLeaks++;
      note({ kind: name, wrongLocale: r.locale, body: body.slice(0, 300) });
      continue;
    }
    if ((k.expectText && !body.includes(k.expectText)) || (k.forbidText && body.includes(k.forbidText))) {
      totals.wrongContent++;
      note({ kind: name, wrongContent: true, status: res.status, body: body.slice(0, 300) });
      continue;
    }
    if (r.mutation) mutations.set(r.mutation, "done");
    totals.ok++;
  }
}

const json = (url) => new Promise((r) => http.get(url, (res) => { let b = ""; res.on("data", (c) => (b += c)); res.on("end", () => { try { r(JSON.parse(b)); } catch { r(null); } }); }).on("error", () => r(null)));
const proc = (p) => {
  try {
    const st = readFileSync(`/proc/${p}/status`, "utf8");
    const kb = (k) => Number(new RegExp(`${k}:\\s+(\\d+)`).exec(st)?.[1] ?? 0) / 1024;
    const f = readFileSync(`/proc/${p}/stat`, "utf8").split(") ")[1].split(" ");
    return { rssMiB: kb("VmRSS"), anonMiB: kb("RssAnon"), swapMiB: kb("VmSwap"), hwmMiB: kb("VmHWM"), cpuSeconds: (Number(f[11]) + Number(f[12])) / 100 };
  } catch {
    return null;
  }
};
const metricsOf = async (url) => {
  const m = url ? await json(`${url}/metrics.json`) : null;
  if (!m) return null;
  return {
    malloc: m.memory?.malloc,
    heapAfterGcMiB: m.executors?.map((e) => +(e.heap_used_after_gc_bytes / 1048576).toFixed(1)),
    restarts: m.executors?.reduce((a, e) => a + e.restarts, 0),
    cache: m.cache,
    images: m.images && Object.fromEntries(["transforms", "fallbacks", "overloaded", "cancelled", "memory_waits", "memory_rejected", "memory_reserved_bytes", "source_held_bytes", "queued", "in_flight", "cache_memory_bytes", "cache_disk_bytes", "cache_corrupt", "cache_write_errors"].map((k) => [k, m.images[k]])),
  };
};
const samples = [];
async function sample(t0) {
  samples.push({
    t: Math.round((Date.now() - t0) / 1000),
    load: Number(readFileSync("/proc/loadavg", "utf8").split(" ")[0]),
    requests: totals.requests,
    main: { ...proc(pid), ...(await metricsOf(args.metrics)) },
    i18n: i18nPid ? { ...proc(i18nPid), ...(await metricsOf(args["i18n-metrics"])) } : undefined,
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
// Explicit invalidation besides background regeneration.
const invalidator = setInterval(() => send({ path: "/api/p/revalidate" }), 30_000);
await Promise.all(Array.from({ length: concurrency }, () => worker(deadline)));
clearInterval(sampler);
clearInterval(invalidator);
if (injector) clearInterval(injector);
await sample(t0);
await up.close();

// Mutation accounting: every acknowledged mutation exactly once, a lost
// one at most once, none twice.
const recorded = await json(`${ledger}/mutations?key=soak`);
const counts = new Map();
for (const id of recorded?.ids ?? []) counts.set(id, (counts.get(id) ?? 0) + 1);
const mutationReport = { sent: mutations.size, acknowledged: 0, acknowledgedMissing: 0, lost: 0, lostApplied: 0, duplicates: 0 };
for (const [id, state] of mutations) {
  const n = counts.get(id) ?? 0;
  if (n > 1) mutationReport.duplicates++;
  if (state === "done") {
    mutationReport.acknowledged++;
    if (n === 0) mutationReport.acknowledgedMissing++;
  } else {
    mutationReport.lost++;
    if (n === 1) mutationReport.lostApplied++;
  }
}

const pct = (a, p) => { const s = [...a].sort((x, y) => x - y); return s.length ? +s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))].toFixed(1) : null; };
const kindsOut = Object.fromEntries(Object.entries(stats).map(([k, s]) => [k, { count: s.count, errors: s.errors, statuses: s.statuses, cache: s.cache, p50: pct(s.ms, 50), p95: pct(s.ms, 95), p99: pct(s.ms, 99) }]));
const all = Object.values(stats).flatMap((s) => s.ms);
const accounted = totals.ok + totals.unexpected + totals.rejected503 + totals.lostToRestart + totals.rejectedDuringRestart + totals.identityLeaks + totals.wrongContent + totals.aborted;
const out = {
  minutes,
  concurrency,
  seconds: (Date.now() - t0) / 1000,
  totals,
  accountedExactlyOnce: accounted === totals.requests,
  mutations: mutationReport,
  throughput: +(totals.requests / ((Date.now() - t0) / 1000)).toFixed(1),
  latency: { p50: pct(all, 50), p95: pct(all, 95), p99: pct(all, 99) },
  kinds: kindsOut,
  errors,
  samples,
};
console.log(JSON.stringify({ ...out, samples: samples.length }, null, 1));
if (args.json) writeFileSync(args.json, JSON.stringify(out, null, 2));
const failed = totals.unexpected || totals.identityLeaks || totals.wrongContent || mutationReport.duplicates || mutationReport.acknowledgedMissing || !out.accountedExactlyOnce;
process.exit(failed ? 1 : 0);
