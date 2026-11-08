// Memory during image bursts against NextRS (or next start): RSS, and for
// NextRS glibc's malloc statistics and the optimizer's queue, sampled every
// 50 ms while a workload runs all at once.
//
//   node benchmarks/image_memory.mjs --server URL [--metrics URL] --workload NAME [--out file]
//
// Workloads (fixtures/next-image sources):
//   burst24   30 distinct transformations of large.jpg (24 MP), widths 256-3840 (Phase 2D's burst)
//   thumbs24  21 distinct thumbnails (32-384 px) of large.jpg
//   full24    large.jpg and large-progressive.jpg at 3840 px, 3 qualities, webp + jpeg
//   mixed     60 distinct keys: 24 MP JPEGs small and large, photo.jpg, alpha.png,
//             photo.webp, AVIF and JPEG outputs, all at once
import http from "node:http";
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const args = Object.fromEntries(process.argv.slice(2).reduce((p, a, i, all) => (a.startsWith("--") && p.push([a.slice(2), all[i + 1]]), p), []));
const server = new URL(args.server);
const pid = Number(/pid=(\d+)/.exec(execSync(`ss -ltnpH 'sport = :${server.port}'`).toString())[1]);
const status = () => readFileSync(`/proc/${pid}/status`, "utf8");
const kib = (s, k) => Number(new RegExp(`${k}:\\s+(\\d+)`).exec(s)?.[1] ?? 0) / 1024;
const json = (url) => new Promise((r) => http.get(url, (res) => { let b = ""; res.on("data", (c) => (b += c)); res.on("end", () => r(JSON.parse(b))); }).on("error", () => r(null)));
const img = (file, w, q) => `/_next/image?url=%2Fphotos%2F${file}&w=${w}&q=${q}`;
const Q = [50, 75, 90];
const webp = "image/webp,*/*", avif = "image/avif,image/webp,*/*", any = "*/*";
const workloads = {
  burst24: () => [640, 750, 828, 1080, 1200, 1920, 2048, 3840, 384, 256].flatMap((w) => Q.map((q) => [img("large.jpg", w, q), webp])),
  thumbs24: () => [32, 48, 64, 96, 128, 256, 384].flatMap((w) => Q.map((q) => [img("large.jpg", w, q), webp])),
  full24: () => ["large.jpg", "large-progressive.jpg"].flatMap((f) => Q.flatMap((q) => [[img(f, 3840, q), webp], [img(f, 3840, q), any]])),
  mixed: () => [
    ...[64, 256, 640, 1920, 3840].flatMap((w) => [[img("large.jpg", w, 75), webp], [img("large-progressive.jpg", w, 75), webp]]),
    ...[640, 1080, 3840].flatMap((w) => [[img("large.jpg", w, 50), any], [img("large.jpg", w, 90), avif]]),
    ...[32, 64, 128, 256, 384, 640, 750, 828, 1080, 1200].flatMap((w) => [[img("photo.jpg", w, 75), webp], [img("alpha.png", w, 75), webp]]),
    ...[64, 384, 1080].flatMap((w) => [[img("photo.webp", w, 75), webp], [img("photo.jpg", w, 50), any], [img("alpha.png", w, 50), any], [img("odd.jpg", w, 90), avif]]),
    ...[48, 96].flatMap((w) => Q.map((q) => [img("photo.jpg", w, q), avif])),
  ],
};
const jobs = workloads[args.workload]();
const get = ([path, accept]) => new Promise((resolve) => {
  const t0 = Date.now();
  http.get({ host: server.hostname, port: server.port, path, headers: { accept } }, (res) => {
    let bytes = 0;
    res.on("data", (c) => (bytes += c.length));
    res.on("end", () => resolve({ status: res.statusCode, ms: Date.now() - t0, bytes, type: res.headers["content-type"] }));
  }).on("error", (e) => resolve({ status: 0, ms: Date.now() - t0, error: e.code }));
});

const s0 = status();
const before = { rssMiB: kib(s0, "VmRSS"), swapMiB: kib(s0, "VmSwap") };
const peak = { rssMiB: before.rssMiB, rssSwapMiB: before.rssMiB + before.swapMiB, mallocInUseMiB: 0, mallocTotalMiB: 0, queued: 0, inFlight: 0, imageReservedMiB: 0 };
let sampling = true;
const sampler = (async () => {
  while (sampling) {
    const s = status();
    peak.rssMiB = Math.max(peak.rssMiB, kib(s, "VmRSS"));
    peak.rssSwapMiB = Math.max(peak.rssSwapMiB, kib(s, "VmRSS") + kib(s, "VmSwap"));
    if (args.metrics) {
      const m = await json(`${args.metrics}/metrics.json`);
      const mal = m?.memory?.malloc;
      if (mal) {
        peak.mallocInUseMiB = Math.max(peak.mallocInUseMiB, (mal.in_use_bytes + mal.mmap_bytes) / 1048576);
        peak.mallocTotalMiB = Math.max(peak.mallocTotalMiB, (mal.arena_bytes + mal.mmap_bytes) / 1048576);
      }
      if (m?.images) {
        peak.queued = Math.max(peak.queued, m.images.queued ?? 0);
        peak.inFlight = Math.max(peak.inFlight, m.images.in_flight ?? 0);
        peak.imageReservedMiB = Math.max(peak.imageReservedMiB, (m.images.memory_reserved_bytes ?? 0) / 1048576);
      }
    }
    await new Promise((r) => setTimeout(r, 50));
  }
})();
const t0 = Date.now();
const results = await Promise.all(jobs.map(get));
const seconds = (Date.now() - t0) / 1000;
sampling = false;
await sampler;
await new Promise((r) => setTimeout(r, 2000));
const s1 = status();
const after = { rssMiB: kib(s1, "VmRSS"), swapMiB: kib(s1, "VmSwap") };
const m = args.metrics ? await json(`${args.metrics}/metrics.json`) : null;
const statuses = {};
for (const r of results) statuses[r.status] = (statuses[r.status] ?? 0) + 1;
const ms = results.map((r) => r.ms).sort((a, b) => a - b);
const out = {
  workload: args.workload, requests: jobs.length, seconds, statuses,
  p50ms: ms[Math.floor(ms.length / 2)], maxMs: ms.at(-1),
  before, peak: Object.fromEntries(Object.entries(peak).map(([k, v]) => [k, Math.round(v * 10) / 10])), after,
  malloc: m?.memory?.malloc, images: m?.images,
};
console.log(JSON.stringify(out, null, 1));
if (args.out) writeFileSync(args.out, JSON.stringify(out, null, 2));
