// Graceful shutdown with compressed and partial responses in flight
// (0.1.0-rc.2): SIGTERM reaches nextrs while gzip responses (cache misses
// being filled, cache hits, per-request streamed SSR) and range responses
// are being sent to slow readers; every one must complete with its full,
// decodable body, and new connections must then be refused.
//
//   node benchmarks/rc2/shutdown.mjs --nextrs PATH --project fixtures/next-app [--sigterm-after 15] [--json out.json]
import { request } from "node:http";
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { gunzipSync } from "node:zlib";
import * as L from "../phase3a/lib.mjs";

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i += 2) args[argv[i].slice(2)] = argv[i + 1];
const project = resolve(args.project ?? "fixtures/next-app");
const port = Number(args.port ?? 5990);

const chunks = join(project, ".next/static/chunks");
const largest = readdirSync(chunks).filter((f) => f.endsWith(".js")).map((f) => ({ f, size: statSync(join(chunks, f)).size })).sort((a, b) => b.size - a.size)[0];
const asset = `/_next/static/chunks/${largest.f}`;
const file = readFileSync(join(chunks, largest.f));

/** A request whose reader takes `delayMs` between chunks (a slow client). */
function slow(path, headers, delayMs) {
  return new Promise((done) => {
    const req = request({ host: "127.0.0.1", port, path, headers: { ...headers }, agent: false }, (res) => {
      const parts = [];
      res.on("data", (c) => { parts.push(c); res.pause(); setTimeout(() => res.resume(), delayMs); });
      res.on("end", () => done({ status: res.statusCode, headers: res.headers, body: Buffer.concat(parts), complete: res.complete }));
      res.on("error", (e) => done({ error: String(e) }));
    });
    req.on("error", (e) => done({ error: String(e) }));
    req.end();
  });
}

const s = await L.startServer({ kind: "nextrs", nextrs: resolve(args.nextrs), project, port, metricsPort: port + 100, cpus: "0-5", readyPath: "/api/health", extraArgs: ["--shutdown-grace", "15"] });
// One warm entry; the large chunk stays cold so its fill is in flight.
await L.request(port, { path: "/", headers: { "accept-encoding": "gzip" } });
const kinds = [];
for (let i = 0; i < 6; i++) kinds.push({ kind: "gzip-miss-then-hit", path: asset, headers: { "accept-encoding": "gzip" }, check: (r) => gunzipSync(r.body).equals(file) });
for (let i = 0; i < 6; i++) kinds.push({ kind: "gzip-cached-page", path: "/", headers: { "accept-encoding": "gzip" }, check: (r) => gunzipSync(r.body).toString().includes("</html>") && Number(r.headers["content-length"]) === r.body.length });
for (let i = 0; i < 6; i++) kinds.push({ kind: "gzip-streamed-ssr", path: "/dynamic", headers: { "accept-encoding": "gzip" }, check: (r) => gunzipSync(r.body).toString().includes("Streamed after Suspense boundary") });
for (let i = 0; i < 6; i++) kinds.push({ kind: "range", path: asset, headers: { range: "bytes=100000-199999" }, check: (r) => r.status === 206 && r.body.equals(file.subarray(100000, 200000)) });
const inflight = kinds.map((k) => slow(k.path, k.headers, 40).then((r) => ({ ...k, r })));
await L.sleep(Number(args["sigterm-after"] ?? 15));
const t0 = Date.now();
process.kill(s.pid, "SIGTERM");
const results = await Promise.all(inflight);
const after = await L.request(port, { path: "/api/health" });
for (let i = 0; i < 100 && existsAlive(s.pid); i++) await L.sleep(100);
function existsAlive(pid) { try { process.kill(pid, 0); return !readFileSync(`/proc/${pid}/stat`, "utf8").includes(") Z "); } catch { return false; } }
const rows = results.map(({ kind, r, check }) => {
  let ok = false;
  try { ok = !r.error && r.complete && r.status < 300 && check(r); } catch {}
  return { kind, status: r.status ?? null, bytes: r.body?.length ?? 0, ok, error: r.error ?? null };
});
const summary = {
  inflight: rows.length, ok: rows.filter((r) => r.ok).length,
  by_kind: Object.fromEntries([...new Set(rows.map((r) => r.kind))].map((k) => [k, `${rows.filter((r) => r.kind === k && r.ok).length}/${rows.filter((r) => r.kind === k).length}`])),
  drained_ms: Date.now() - t0, new_connection_after_drain: after.status === 0 ? "refused" : after.status,
  exited: !existsAlive(s.pid), stderr_tail: s.stderr().split("\n").filter((l) => /drain|shutdown|stopp|error/i.test(l)).slice(-6),
};
console.log(JSON.stringify(summary, null, 1));
if (args.json) writeFileSync(args.json, JSON.stringify({ summary, rows }, null, 1));
await L.stopServer(s);
process.exit(summary.ok === summary.inflight && summary.exited ? 0 : 1);
