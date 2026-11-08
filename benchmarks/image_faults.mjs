// Resource-limit and fault checks for NextRS's image optimizer (Phase 2D):
//   node benchmarks/image_faults.mjs --server URL --metrics URL [--baseline URL] [--out file]
// 1. 30 distinct 24 MP transformations at once: peak RSS, transformations
//    at a time (bounded by --image-concurrency), all answered.
// 2. 40 clients that disconnect after 30 ms: cancellations, then the
//    server still answers, RSS back near its level.
// 3. Public-image transformations never use an isolate: executor request
//    count unchanged; a route handler source does use one.
// 4. Memory admission: reservations and source bytes all return to zero;
//    the burst reserved memory, never above the budget (exit 1 otherwise).
import http from "node:http";
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const args = Object.fromEntries(process.argv.slice(2).reduce((p, a, i, all) => (a.startsWith("--") && p.push([a.slice(2), all[i + 1]]), p), []));
const rss = (port) => {
  const pid = /pid=(\d+)/.exec(execSync(`ss -ltnpH 'sport = :${port}'`).toString())[1];
  return Number(/VmRSS:\s+(\d+)/.exec(readFileSync(`/proc/${pid}/status`, "utf8"))[1]) / 1024;
};
const get = (base, path, { abortMs } = {}) =>
  new Promise((resolve) => {
    const u = new URL(base);
    const req = http.get({ host: u.hostname, port: u.port, path, headers: { accept: "image/webp" } }, (res) => {
      res.resume();
      res.on("end", () => resolve(res.statusCode));
    });
    req.on("error", () => resolve("aborted"));
    if (abortMs) setTimeout(() => req.destroy(), abortMs);
  });
const metrics = async () => JSON.parse(await new Promise((r) => http.get(`${args.metrics}/metrics.json`, (res) => { let b = ""; res.on("data", (c) => (b += c)); res.on("end", () => r(b)); })));
const img = (w, q, extra = "large.jpg") => `/_next/image?url=%2Fphotos%2F${extra}&w=${w}&q=${q}`;
const keys = [640, 750, 828, 1080, 1200, 1920, 2048, 3840, 384, 256].flatMap((w) => [50, 75, 90].map((q) => img(w, q)));
const out = {};
async function burst(base, label) {
  const port = new URL(base).port;
  const before = rss(port);
  let peak = before;
  const t = setInterval(() => (peak = Math.max(peak, rss(port))), 50);
  const t0 = Date.now();
  const statuses = await Promise.all(keys.map((k) => get(base, k)));
  clearInterval(t);
  return { label, requests: keys.length, seconds: (Date.now() - t0) / 1000, statuses: [...new Set(statuses)], rssBeforeMiB: Math.round(before), peakRssMiB: Math.round(peak) };
}
if (args.baseline) out.burstNextStart = await burst(args.baseline, "next start");
// Reservations seen while the burst runs (memory admission, Phase 2E).
let reservedPeak = 0, reservedSampling = true;
const reservedSampler = (async () => {
  while (reservedSampling) {
    const m = (await metrics()).images;
    reservedPeak = Math.max(reservedPeak, m.memory_reserved_bytes ?? 0);
    await new Promise((r) => setTimeout(r, 20));
  }
})();
out.burstNextrs = await burst(args.server, "nextrs");
reservedSampling = false;
await reservedSampler;
{
  const m = (await metrics()).images;
  out.burstNextrs.transforms = m.transforms;
  out.burstNextrs.fallbacks = m.fallbacks;
}
// 2. Aborting clients (bomb-free sources, distinct keys: odd.jpg / photo.webp).
const before = await metrics();
const port = new URL(args.server).port;
const rss0 = rss(port);
const aborts = await Promise.all(
  ["odd.jpg", "photo.webp", "alpha.png", "p3.jpg"].flatMap((f) => [640, 750, 828, 1080, 1200, 1920, 2048, 384, 256, 128].map((w) => get(args.server, img(w, 75, f), { abortMs: 30 }))),
);
await new Promise((r) => setTimeout(r, 3000));
const after = await metrics();
out.aborts = {
  clients: aborts.length,
  aborted: aborts.filter((s) => s === "aborted").length,
  cancelledTransforms: after.images.cancelled - before.images.cancelled,
  transformsStarted: after.images.transforms - before.images.transforms,
  inFlightAfter: after.images.in_flight,
  queuedAfter: after.images.queued,
  rssBeforeMiB: Math.round(rss0),
  rssAfterMiB: Math.round(rss(port)),
  answersAfter: await get(args.server, img(640, 75, "odd.jpg")),
};
// 3. Isolates: executor requests before/after public-image transformations.
const execBefore = (await metrics()).requests_admitted;
const publicStatuses = await Promise.all([32, 48, 64, 96].map((w) => get(args.server, img(w, 50, "photo.jpg"))));
const execAfterPublic = (await metrics()).requests_admitted;
await get(args.server, "/_next/image?url=%2Fapi%2Fimage%2Fdyn.png&w=128&q=50");
const execAfterRoute = (await metrics()).requests_admitted;
out.isolates = { publicStatuses, executorRequestsForPublicImages: execAfterPublic - execBefore, executorRequestsForRouteSource: execAfterRoute - execAfterPublic };
// 4. Admission accounting: everything reserved comes back once the work
// (aborted or finished) has ended; the burst reserved memory, within the budget.
{
  let m;
  for (let i = 0; i < 100; i++) {
    m = (await metrics()).images;
    if (!m.memory_reserved_bytes && !m.source_held_bytes && !m.in_flight) break;
    await new Promise((r) => setTimeout(r, 100));
  }
  const failures = [];
  if (m.memory_reserved_bytes) failures.push(`memory still reserved: ${m.memory_reserved_bytes}`);
  if (m.source_held_bytes) failures.push(`source bytes still held: ${m.source_held_bytes}`);
  if (!(reservedPeak > 1024 * 1024)) failures.push(`the burst's transformations reserved ${reservedPeak} bytes`);
  if (reservedPeak > m.memory_budget_bytes) failures.push(`reserved ${reservedPeak} above the budget ${m.memory_budget_bytes}`);
  out.admission = { ok: failures.length === 0, failures, reservedPeakMiB: Math.round(reservedPeak / 1048576), budgetMiB: Math.round(m.memory_budget_bytes / 1048576), memoryWaits: m.memory_waits, memoryRejected: m.memory_rejected };
}
console.log(JSON.stringify(out, null, 1));
if (args.out) writeFileSync(args.out, JSON.stringify(out, null, 2));
if (!out.admission.ok) process.exit(1);
