// Sustained mixed traffic against a running server, with optional executor
// stall injection, sampling server RSS and executor metrics.
//
//   node benchmarks/soak.mjs --target http://127.0.0.1:3001 --pid <server pid> \
//        [--metrics http://127.0.0.1:9101] [--minutes 10] [--concurrency 48] \
//        [--inject-every 60 --inject-ms 3000] [--sample-every 10] [--json out.json]
//
// Traffic, per worker, in rotation:
//   * /request-info and /books/<id> with unique tokens; any foreign token in
//     a body is a request-state leak (as in tests/compat/isolation.mjs);
//   * /dynamic (streaming Suspense), some aborted after the first chunk;
//   * /api/stream, 4 MiB, read slowly; some aborted mid-body;
//   * POST /api/echo with a 256 KiB body, hash verified;
//   * /api/health.
// With --inject-every, GET /api/spin?ms=<inject-ms> is sent on that period;
// run the server with --js-stall-timeout below inject-ms so the stall
// terminates and replaces an isolate.
//
// Requests that fail because their isolate was terminated (502, aborted
// body) are counted as "lost to restart", not as errors; only leaks,
// corrupted bodies and unexpected statuses fail the run.
import http from "node:http";
import { createHash, randomBytes } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const target = new URL(args.target ?? "http://127.0.0.1:3001");
const pid = args.pid ? Number(args.pid) : null;
const metricsUrl = args.metrics ? new URL("/metrics.json", args.metrics) : null;
const minutes = Number(args.minutes ?? 10);
const concurrency = Number(args.concurrency ?? 48);
const injectEvery = args["inject-every"] ? Number(args["inject-every"]) : null;
const injectMs = Number(args["inject-ms"] ?? 3000);
const sampleEvery = Number(args["sample-every"] ?? 10);
const agent = new http.Agent({ keepAlive: true, maxSockets: concurrency + 8 });

const counts = {};
const bump = (key) => (counts[key] = (counts[key] ?? 0) + 1);
const failures = [];
const fail = (kind, detail) => {
  bump(`${kind}-FAILED`);
  if (failures.length < 50) failures.push({ kind, ...detail });
};

function request(path, { method = "GET", headers = {}, body, abortAfter, slow } = {}) {
  return new Promise((resolve) => {
    const req = http.request(
      { host: target.hostname, port: target.port, method, path, headers, agent },
      (res) => {
        const chunks = [];
        let received = 0;
        res.on("data", (chunk) => {
          chunks.push(chunk);
          received += chunk.length;
          if (abortAfter != null && received >= abortAfter) {
            req.destroy();
            resolve({ status: res.statusCode, aborted: true });
            return;
          }
          if (slow) {
            res.pause();
            setTimeout(() => res.resume(), slow);
          }
        });
        res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(chunks) }));
        res.on("error", () => resolve({ status: res.statusCode, truncated: true }));
        res.on("aborted", () => resolve({ status: res.statusCode, truncated: true }));
      },
    );
    req.on("error", (error) => resolve({ error: error.code ?? String(error) }));
    req.end(body);
  });
}

// A response cut short by an isolate restart: 502 before the head, or a
// body that ended without its terminating chunk.
function lostToRestart(r) {
  return r.status === 502 || r.truncated || r.error === "ECONNRESET" || r.error === "UND_ERR_SOCKET";
}

const TOKEN = /nx[pctb]-\d+-[0-9a-f]{8}/g;
let seq = 0;

async function identityCase() {
  const i = seq++;
  const salt = randomBytes(4).toString("hex");
  let path, headers, expected;
  if (i % 2) {
    const probe = `nxp-${i}-${salt}`;
    const theme = `nxc-${i}-${salt}`;
    path = "/request-info";
    headers = { "x-nextrs-probe": probe, cookie: `theme=${theme}` };
    expected = [probe, theme];
  } else {
    const book = `nxb-${i}-${salt}`;
    path = `/books/${book}`;
    headers = {};
    expected = [book];
  }
  const r = await request(path, { headers });
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("identity-status", { path, status: r.status, error: r.error });
  const found = new Set(r.body.toString().match(TOKEN) ?? []);
  const foreign = [...found].filter((t) => !expected.includes(t));
  const missing = expected.filter((t) => !found.has(t));
  if (foreign.length) return fail("LEAK", { path, foreign });
  if (missing.length) return fail("identity-missing", { path, missing });
  bump("identity-ok");
}

async function dynamicCase(abort) {
  const r = await request("/dynamic", abort ? { abortAfter: 1 } : {});
  if (abort) return bump("dynamic-aborted");
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200 || !r.body.includes("Streamed after Suspense boundary")) {
    return fail("dynamic", { status: r.status });
  }
  bump("dynamic-ok");
}

function checkStream(body, size) {
  if (body.length !== size) return false;
  for (let at = 0; at < body.length; at += 4093) {
    const index = Math.floor(at / 16384);
    const pos = at % 16384;
    const expected = pos < 4 ? (index >>> (8 * (3 - pos))) & 0xff : index % 251;
    if (body[at] !== expected) return false;
  }
  return true;
}

async function streamCase(abort) {
  const size = 4 * 1024 * 1024;
  const r = await request(`/api/stream?size=${size}`, abort ? { abortAfter: 512 * 1024 } : { slow: 2 });
  if (abort) return bump("stream-aborted");
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200 || !checkStream(r.body, size)) return fail("stream", { status: r.status, length: r.body?.length });
  bump("stream-ok");
}

async function uploadCase() {
  const body = randomBytes(256 * 1024);
  const r = await request("/api/echo", { method: "POST", body, headers: { "content-type": "application/octet-stream" } });
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("upload-status", { status: r.status });
  const json = JSON.parse(r.body.toString());
  if (json.sha256 !== createHash("sha256").update(body).digest("hex")) return fail("upload-hash", {});
  bump("upload-ok");
}

async function healthCase() {
  const r = await request("/api/health");
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("health", { status: r.status });
  bump("health-ok");
}

const rotation = [
  identityCase, identityCase, identityCase, identityCase,
  () => dynamicCase(false), () => dynamicCase(true),
  () => streamCase(false), () => streamCase(true),
  uploadCase, healthCase, identityCase, healthCase,
];

function rssMiB() {
  if (!pid) return null;
  const status = readFileSync(`/proc/${pid}/status`, "utf8");
  return Number(/VmRSS:\s+(\d+)/.exec(status)[1]) / 1024;
}

async function metrics() {
  if (!metricsUrl) return null;
  return new Promise((resolve) => {
    http.get(metricsUrl, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve(JSON.parse(Buffer.concat(chunks).toString())));
    }).on("error", () => resolve(null));
  });
}

const started = Date.now();
const deadline = started + minutes * 60_000;
const samples = [];
let completed = 0;

async function sample() {
  const m = await metrics();
  const row = {
    t: Math.round((Date.now() - started) / 1000),
    rssMiB: rssMiB(),
    completed,
    restarts: m ? m.executors.reduce((n, e) => n + e.restarts, 0) : null,
    ready: m ? m.executors.filter((e) => e.state === "ready").length : null,
    cancelled: m?.responses_cancelled ?? null,
    backpressureWaits: m?.backpressure_waits ?? null,
    rejected: m ? m.rejected_overloaded + m.rejected_queue_timeout + m.rejected_unavailable : null,
  };
  samples.push(row);
  console.log(JSON.stringify(row));
}

const sampler = setInterval(sample, sampleEvery * 1000);
const injections = [];
const injector = injectEvery
  ? setInterval(async () => {
      const at = Math.round((Date.now() - started) / 1000);
      const r = await request(`/api/spin?ms=${injectMs}`);
      injections.push({ at, status: r.status ?? r.error });
    }, injectEvery * 1000)
  : null;

await sample();
await Promise.all(
  Array.from({ length: concurrency }, async (_, w) => {
    let k = w;
    while (Date.now() < deadline) {
      await rotation[k++ % rotation.length]();
      completed++;
    }
  }),
);
clearInterval(sampler);
if (injector) clearInterval(injector);
await new Promise((r) => setTimeout(r, 3000));
await sample();

const result = {
  target: target.href,
  minutes,
  concurrency,
  injectEvery,
  injectMs,
  completed,
  counts,
  injections,
  failures,
  samples,
};
if (args.json) writeFileSync(args.json, JSON.stringify(result, null, 2));
console.log(JSON.stringify({ completed, counts, injections: injections.length, failures: failures.length }, null, 2));
agent.destroy();
process.exitCode = Object.keys(counts).some((k) => k.endsWith("-FAILED")) ? 1 : 0;
