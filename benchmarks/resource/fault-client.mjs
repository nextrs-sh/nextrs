// Runs in the tools container (Node.js 24; copied to /tmp by faults.mjs).
// The client side of the fault workloads: what `nextrs-loadgen` cannot do
// (per-status counts, uploads, slow readers, single large requests) and
// what the classification needs (every status, every connection error,
// health while the fault runs, /metrics.json over time). One JSON object
// on stdout; the harness keeps it raw (`*.client.jsonl`).
//
//   node fault-client.mjs MODE --base http://s1-sf:8080 [options]
//
// Modes:
//   requests   --path P [--method GET] [--header k:v ...] --count N --concurrency C [--stop-after-fails K]
//   closed     --spec FILE --connections N --duration S
//   open       --spec FILE --rate R --duration S [--max-inflight M]
//   upload     --path P --size-mib M --concurrency C --count N
//   slow-read  --path P --streams N --rate-bps B --duration S
//   health     --path P --duration S [--interval S]
// Every mode:  [--timeout S] [--probe PATH --probe-interval S] [--poll-metrics URL --poll-interval S]
import http from "node:http";
import { readFileSync } from "node:fs";

const [mode, ...rest] = process.argv.slice(2);
const o = { header: [] };
for (let i = 0; i < rest.length; i++) {
  const k = rest[i].replace(/^--/, "");
  const v = rest[i + 1];
  if (k === "header") o.header.push(v), i++;
  else if (v === undefined || v.startsWith("--")) o[k] = true;
  else (o[k] = v), i++;
}
const base = new URL(o.base ?? "http://s1-sf:8080");
const timeoutMs = Number(o.timeout ?? 60) * 1000;
const now = () => Number(process.hrtime.bigint() / 1000n) / 1000; // ms, monotonic
const t0 = now();
const started = Date.now();

const pct = (xs, p) => {
  if (!xs.length) return null;
  const s = [...xs].sort((a, b) => a - b);
  return Math.round(s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))] * 10) / 10;
};
const latency = (xs) => ({ n: xs.length, p50: pct(xs, 50), p95: pct(xs, 95), p99: pct(xs, 99), max: xs.length ? Math.max(...xs) : null });

function agent(max) {
  return new http.Agent({ keepAlive: true, maxSockets: max, maxFreeSockets: max });
}

/**
 * One request. Resolves (never rejects) with { status, ms, bytes, body
 * (first 300 bytes as text), error }. `body`: a Buffer, or { size, chunk }
 * for a generated streamed upload.
 */
function request({ path, method = "GET", headers = {}, body = null, ag, keepBody = 300 }) {
  return new Promise((resolve) => {
    const start = now();
    let done = false;
    const finish = (r) => {
      if (done) return;
      done = true;
      resolve({ ms: Math.round((now() - start) * 10) / 10, ...r });
    };
    const h = { ...headers };
    if (body && Buffer.isBuffer(body)) h["content-length"] = body.length;
    if (body && body.size !== undefined) h["content-length"] = body.size;
    const req = http.request({ host: base.hostname, port: base.port || 80, path, method, headers: h, agent: ag }, (res) => {
      const chunks = [];
      let bytes = 0;
      let kept = 0;
      res.on("data", (c) => {
        bytes += c.length;
        if (kept < keepBody) {
          chunks.push(c.subarray(0, keepBody - kept));
          kept += Math.min(c.length, keepBody - kept);
        }
      });
      res.on("end", () => finish({ status: res.statusCode, bytes, body: Buffer.concat(chunks).toString("utf8") }));
      res.on("error", (e) => finish({ status: res.statusCode, bytes, error: e.code ?? e.message }));
      res.on("aborted", () => finish({ status: res.statusCode, bytes, error: "aborted" }));
    });
    req.setTimeout(timeoutMs, () => {
      req.destroy(new Error("timeout"));
      finish({ status: 0, error: "timeout" });
    });
    req.on("error", (e) => finish({ status: 0, error: e.code ?? e.message }));
    if (body && Buffer.isBuffer(body)) req.end(body);
    else if (body && body.size !== undefined) {
      // A generated body, written as fast as the server takes it.
      const chunk = Buffer.alloc(body.chunk ?? 64 * 1024, 0x61);
      let left = body.size;
      const pump = () => {
        while (left > 0) {
          const n = Math.min(left, chunk.length);
          left -= n;
          if (!req.write(n === chunk.length ? chunk : chunk.subarray(0, n))) return req.once("drain", pump);
        }
        req.end();
      };
      pump();
    } else req.end();
  });
}

function tally(results) {
  const statuses = {};
  const errors = {};
  for (const r of results) {
    statuses[r.status] = (statuses[r.status] ?? 0) + 1;
    if (r.error) errors[r.error] = (errors[r.error] ?? 0) + 1;
  }
  const ok = results.filter((r) => r.status >= 200 && r.status < 300 && !r.error).length;
  return { completed: results.length, ok, failed: results.length - ok, statuses, errors };
}

// Background series: health probes and /metrics.json, while the mode runs.
const series = { probe: [], metrics: [] };
const timers = [];
if (o.probe && o.probe !== true) {
  const ag = agent(1);
  const every = Number(o["probe-interval"] ?? 5) * 1000;
  const tick = async () => {
    const r = await request({ path: o.probe, ag, keepBody: 0 });
    series.probe.push({ t: Date.now(), status: r.status, ms: r.ms, error: r.error });
  };
  timers.push(setInterval(tick, every));
}
if (o["poll-metrics"] && o["poll-metrics"] !== true) {
  const every = Number(o["poll-interval"] ?? 2) * 1000;
  const tick = async () => {
    try {
      const r = await fetch(o["poll-metrics"], { signal: AbortSignal.timeout(2000) });
      series.metrics.push({ t: Date.now(), metrics: await r.json() });
    } catch (e) {
      series.metrics.push({ t: Date.now(), error: e.name });
    }
  };
  timers.push(setInterval(tick, every));
}

function specRequests() {
  const spec = JSON.parse(readFileSync(o.spec, "utf8"));
  return spec.requests.map((r) => ({
    path: r.path, method: r.method ?? "GET", headers: Object.fromEntries(r.headers ?? []),
    body: r.body ? Buffer.from(r.body) : null,
  }));
}

const headers = Object.fromEntries(o.header.map((h) => [h.slice(0, h.indexOf(":")).trim(), h.slice(h.indexOf(":") + 1).trim()]));

async function modeRequests() {
  const count = Number(o.count ?? 1);
  const concurrency = Number(o.concurrency ?? 1);
  const stopAfter = o["stop-after-fails"] === undefined ? Infinity : Number(o["stop-after-fails"]);
  const ag = agent(concurrency);
  const results = [];
  let next = 0;
  let fails = 0;
  const worker = async () => {
    while (next < count && fails < stopAfter) {
      const i = next++;
      const r = await request({ path: o.path, method: o.method ?? "GET", headers, ag });
      results.push({ i, t: Date.now(), ...r });
      if (!(r.status >= 200 && r.status < 300) || r.error) fails++;
    }
  };
  await Promise.all(Array.from({ length: concurrency }, worker));
  return { ...tally(results), stoppedEarly: next < count, results: results.sort((a, b) => a.i - b.i) };
}

async function modeClosed() {
  const reqs = specRequests();
  const connections = Number(o.connections ?? 16);
  const until = now() + Number(o.duration ?? 30) * 1000;
  const ag = agent(connections);
  const results = [];
  let n = 0;
  const worker = async () => {
    while (now() < until) {
      const q = reqs[n++ % reqs.length];
      const r = await request({ ...q, ag, keepBody: 0 });
      results.push({ status: r.status, ms: r.ms, error: r.error });
    }
  };
  await Promise.all(Array.from({ length: connections }, worker));
  const seconds = (now() - t0) / 1000;
  const t = tally(results);
  return {
    ...t, achieved_rps: t.completed / seconds, ok_rps: t.ok / seconds,
    latency: latency(results.map((r) => r.ms)), latencies_ms: results.map((r) => r.ms), statuses_seq: results.map((r) => r.status),
  };
}

async function modeOpen() {
  const reqs = specRequests();
  const rate = Number(o.rate);
  const duration = Number(o.duration ?? 30);
  const maxInflight = Number(o["max-inflight"] ?? 512);
  if (!(rate > 0)) throw new Error("--rate must be > 0");
  const ag = agent(maxInflight);
  const results = [];
  const pending = [];
  let inflight = 0;
  let dropped = 0;
  const total = Math.floor(rate * duration);
  const start = now();
  for (let i = 0; i < total; i++) {
    const due = start + (i * 1000) / rate;
    const wait = due - now();
    if (wait > 1) await new Promise((r) => setTimeout(r, wait));
    if (inflight >= maxInflight) {
      dropped++;
      continue;
    }
    inflight++;
    const q = reqs[i % reqs.length];
    pending.push(request({ ...q, ag, keepBody: 0 }).then((r) => {
      inflight--;
      // Latency from the scheduled send time (coordinated omission).
      results.push({ status: r.status, ms: Math.round((now() - due) * 10) / 10, error: r.error });
    }));
  }
  await Promise.all(pending);
  const t = tally(results);
  return {
    ...t, offered_rps: rate, scheduled: total, dropped_client_limit: dropped,
    ok_rps: t.ok / duration, latency: latency(results.map((r) => r.ms)), latencies_ms: results.map((r) => r.ms),
  };
}

async function modeUpload() {
  const size = Math.round(Number(o["size-mib"] ?? 8) * 2 ** 20);
  const concurrency = Number(o.concurrency ?? 8);
  const count = Number(o.count ?? concurrency);
  const ag = agent(concurrency);
  const results = [];
  let next = 0;
  const worker = async () => {
    while (next < count) {
      const i = next++;
      const r = await request({
        path: o.path, method: "POST", headers: { "content-type": "application/octet-stream", ...headers },
        body: { size }, ag,
      });
      results.push({ i, t: Date.now(), ...r });
    }
  };
  await Promise.all(Array.from({ length: concurrency }, worker));
  return { ...tally(results), sizeBytes: size, latency: latency(results.map((r) => r.ms)), results: results.sort((a, b) => a.i - b.i) };
}

async function modeSlowRead() {
  const streams = Number(o.streams ?? 16);
  const bps = Number(o["rate-bps"] ?? 1024);
  const duration = Number(o.duration ?? 120) * 1000;
  const state = [];
  const ag = agent(streams);
  await Promise.all(Array.from({ length: streams }, (_, i) => new Promise((resolve) => {
    const s = { i, status: 0, bytes: 0, ended: false, error: null, openedMs: null };
    state.push(s);
    const start = now();
    const req = http.request({ host: base.hostname, port: base.port || 80, path: o.path, agent: ag }, (res) => {
      s.status = res.statusCode;
      s.openedMs = Math.round(now() - start);
      res.pause();
      res.on("end", () => (s.ended = true));
      // Errors after the client hung up itself (the end of the stall) are not the server's.
      res.on("error", (e) => {
        if (!s.hungUp) s.error ??= e.code ?? e.message;
      });
      res.on("aborted", () => {
        if (!s.hungUp) s.error ??= "aborted";
      });
      res.on("close", () => (s.closed = true));
      const read = setInterval(() => {
        let budget = bps;
        while (budget > 0 && res.readableLength > 0) {
          const c = res.read(Math.min(budget, res.readableLength));
          if (!c) break;
          s.bytes += c.length;
          budget -= c.length;
        }
        if (res.readableLength === 0) res.read(0); // ask for more from the socket
      }, 1000);
      setTimeout(() => {
        clearInterval(read);
        s.stillOpen = !s.ended && !s.error;
        s.hungUp = true;
        req.destroy();
        resolve();
      }, duration);
    });
    req.setTimeout(duration + 5000, () => req.destroy(new Error("timeout")));
    req.on("error", (e) => {
      if (!s.hungUp) s.error ??= e.code ?? e.message;
      if (!s.status) resolve();
    });
    req.end();
  })));
  const statuses = {};
  for (const s of state) statuses[s.status] = (statuses[s.status] ?? 0) + 1;
  return {
    streams, rateBps: bps, statuses,
    stillOpen: state.filter((s) => s.stillOpen).length, endedStreams: state.filter((s) => s.ended).length,
    errors: state.filter((s) => s.error).reduce((a, s) => ((a[s.error] = (a[s.error] ?? 0) + 1), a), {}),
    bytes: state.reduce((a, s) => a + s.bytes, 0), perStream: state,
  };
}

async function modeHealth() {
  const until = now() + Number(o.duration ?? 30) * 1000;
  const every = Number(o.interval ?? 2) * 1000;
  const ag = agent(1);
  const out = [];
  let firstOkMs = null;
  while (now() < until) {
    const r = await request({ path: o.path, ag, keepBody: 0 });
    out.push({ t: Date.now(), status: r.status, ms: r.ms, error: r.error });
    if (firstOkMs === null && r.status === 200) firstOkMs = Math.round(now() - t0);
    await new Promise((res) => setTimeout(res, every));
  }
  return { checks: out, firstOkMs, lastStatus: out.at(-1)?.status ?? 0, okCount: out.filter((r) => r.status === 200).length, count: out.length };
}

const modes = { requests: modeRequests, closed: modeClosed, open: modeOpen, upload: modeUpload, "slow-read": modeSlowRead, health: modeHealth };
if (!modes[mode]) {
  console.error(`usage: fault-client.mjs ${Object.keys(modes).join("|")} --base URL ...`);
  process.exit(2);
}
let result;
try {
  result = await modes[mode]();
} catch (error) {
  result = { clientError: String(error.message) };
}
for (const t of timers) clearInterval(t);
process.stdout.write(JSON.stringify({ mode, started, ended: Date.now(), args: o, ...result, series }) + "\n");
process.exit(0);
