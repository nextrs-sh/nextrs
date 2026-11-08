// /_next/image benchmark: next start (sharp) against NextRS on
// fixtures/next-image, identical transformations, with a local image
// upstream for remote sources (no public hosts).
//
//   node benchmarks/image_bench.mjs --server URL --label NAME --project fixtures/next-image
//     [--upstream-port 4317] [--out results.json]
//
// The server must have just started with an empty image cache. Phases, in
// order (each key requested once in a cold phase, so a cold phase measures
// transformations, never cache hits):
//   cold-webp   3 sources x 3 widths x 3 qualities, Accept webp, 4 at a time
//   cold-jpeg   the same keys without webp in Accept (JPEG/PNG output)
//   cold-avif   2 sources x 3 widths, Accept avif, 2 at a time
//   thumbnails  photo.jpg at 32..128 px, cold
//   warm        the cold-webp keys again, 32 at a time for 8 s (cache hits)
//   remote-cold 27 distinct remote keys (query nonce), 4 at a time
//   remote-warm the same keys, 32 at a time for 6 s
//   dynamic     a route handler alone, 8 at a time for 6 s, then with
//               cold remote transformations running beside it
// CPU time and RSS of the listening process come from /proc.
import http from "node:http";
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { startUpstream } from "../tests/compat/image-upstream.mjs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const server = new URL(args.server);
const upstreamPort = Number(args["upstream-port"] ?? 4317);
const agent = new http.Agent({ keepAlive: true, maxSockets: 256 });

function pidOf(port) {
  const out = execSync(`ss -ltnpH 'sport = :${port}'`).toString();
  return Number(/pid=(\d+)/.exec(out)?.[1]);
}
const pid = pidOf(server.port);
const ticks = 100;
function cpuSeconds() {
  const f = readFileSync(`/proc/${pid}/stat`, "utf8").split(") ")[1].split(" ");
  return (Number(f[11]) + Number(f[12])) / ticks;
}
function rssMiB() {
  const s = readFileSync(`/proc/${pid}/status`, "utf8");
  return Number(/VmRSS:\s+(\d+)/.exec(s)[1]) / 1024;
}

function request(path, accept) {
  return new Promise((resolve) => {
    const started = process.hrtime.bigint();
    const req = http.request({ host: server.hostname, port: server.port, path, agent, headers: { accept } }, (res) => {
      let bytes = 0;
      res.on("data", (c) => (bytes += c.length));
      res.on("end", () =>
        resolve({ status: res.statusCode, ms: Number(process.hrtime.bigint() - started) / 1e6, bytes, cache: res.headers["x-nextjs-cache"] }),
      );
    });
    req.on("error", () => resolve({ status: 0, ms: Number(process.hrtime.bigint() - started) / 1e6, bytes: 0 }));
    req.end();
  });
}

const pct = (sorted, p) => sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))];

// Runs `jobs` (a list, or a generator for timed phases) `concurrency` at a time.
async function phase(name, { jobs, concurrency, seconds }) {
  let peak = rssMiB();
  const sampler = setInterval(() => (peak = Math.max(peak, rssMiB())), 100);
  const cpu0 = cpuSeconds();
  const t0 = Date.now();
  const results = [];
  let i = 0;
  const deadline = seconds ? t0 + seconds * 1000 : Infinity;
  const next = () => (seconds ? (Date.now() < deadline ? jobs(i++) : null) : jobs[i++] ?? null);
  await Promise.all(
    Array.from({ length: concurrency }, async () => {
      for (let job = next(); job; job = next()) results.push(await request(job.path, job.accept));
    }),
  );
  clearInterval(sampler);
  const wall = (Date.now() - t0) / 1000;
  const ms = results.map((r) => r.ms).sort((a, b) => a - b);
  const out = {
    phase: name,
    requests: results.length,
    concurrency,
    wallSeconds: +wall.toFixed(2),
    rps: +(results.length / wall).toFixed(1),
    p50: +pct(ms, 50).toFixed(1),
    p95: +pct(ms, 95).toFixed(1),
    p99: +pct(ms, 99).toFixed(1),
    errors: results.filter((r) => r.status !== 200).length,
    statuses: [...new Set(results.map((r) => r.status))],
    cache: Object.fromEntries([...new Set(results.map((r) => r.cache))].map((c) => [c ?? "-", results.filter((r) => r.cache === c).length])),
    outputBytes: results.reduce((a, r) => a + r.bytes, 0),
    cpuSeconds: +(cpuSeconds() - cpu0).toFixed(2),
    peakRssMiB: +peak.toFixed(1),
  };
  console.log(JSON.stringify(out));
  return out;
}

const img = (url, w, q) => `/_next/image?url=${encodeURIComponent(url)}&w=${w}&q=${q}`;
const WEBP = "image/webp,*/*";
const AVIF = "image/avif,image/webp,*/*";
const ANY = "*/*";
const sources = ["/photos/photo.jpg", "/photos/large.jpg", "/photos/alpha.png"];
const keys = (accept) =>
  sources.flatMap((s) => [640, 1080, 1920].flatMap((w) => [50, 75, 90].map((q) => ({ path: img(s, w, q), accept }))));
const remote = (n) => `http://127.0.0.1:${upstreamPort}/img/photo.jpg?bench=${n}`;

const up = await startUpstream(join(args.project ?? "fixtures/next-image", "public/photos"), upstreamPort);
const startRss = rssMiB();
const phases = [];
phases.push(await phase("cold-webp", { jobs: keys(WEBP), concurrency: 4 }));
phases.push(await phase("cold-jpeg", { jobs: keys(ANY), concurrency: 4 }));
phases.push(
  await phase("cold-avif", {
    jobs: ["/photos/photo.jpg", "/photos/odd.jpg"].flatMap((s) => [384, 640, 1080].map((w) => ({ path: img(s, w, 75), accept: AVIF }))),
    concurrency: 2,
  }),
);
phases.push(await phase("thumbnails", { jobs: [32, 48, 64, 96, 128].flatMap((w) => [50, 75, 90].map((q) => ({ path: img("/photos/odd.jpg", w, q), accept: WEBP }))), concurrency: 4 }));
const warmKeys = keys(WEBP);
phases.push(await phase("warm", { jobs: (i) => warmKeys[i % warmKeys.length], concurrency: 32, seconds: 8 }));
const remoteKeys = Array.from({ length: 27 }, (_, n) => ({ path: img(remote(`c${n}`), [640, 1080, 1920][n % 3], [50, 75, 90][n % 3]), accept: WEBP }));
phases.push(await phase("remote-cold", { jobs: remoteKeys, concurrency: 4 }));
phases.push(await phase("remote-warm", { jobs: (i) => remoteKeys[i % remoteKeys.length], concurrency: 32, seconds: 6 }));
const dynamicJob = () => ({ path: "/api/image/dyn.png", accept: "*/*" });
phases.push(await phase("dynamic-alone", { jobs: dynamicJob, concurrency: 8, seconds: 6 }));
// The same dynamic load with cold transformations beside it.
let n = 0;
const background = phase("background-transforms", {
  jobs: () => ({ path: img(remote(`m${n++}`), 1920, 75), accept: WEBP }),
  concurrency: 6,
  seconds: 6,
});
const mixed = await phase("dynamic-with-transforms", { jobs: dynamicJob, concurrency: 8, seconds: 6 });
phases.push(mixed, await background);
await up.close();
const report = { label: args.label, server: args.server, pid, startRssMiB: +startRss.toFixed(1), endRssMiB: +rssMiB().toFixed(1), phases };
if (args.out) writeFileSync(args.out, JSON.stringify(report, null, 2));
agent.destroy();
