// Load with a distinct URL per request, which ab cannot do: every request
// to /api/data uses a fresh cache key, so a force-cache fetch always
// misses, fetches upstream and stores (the cold path).
//
//   node benchmarks/cold_bench.mjs --target http://127.0.0.1:3001 \
//        [--requests 2000] [--concurrency 32] [--query "cache=force-cache"]
//
// Prints one JSON object: requests_per_second, p50/p95/p99 (ms), failed.
import http from "node:http";
import { randomBytes } from "node:crypto";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const target = new URL(args.target);
const total = Number(args.requests ?? 2000);
const concurrency = Number(args.concurrency ?? 32);
const query = args.query ?? "cache=force-cache";
const run = randomBytes(4).toString("hex");
const agent = new http.Agent({ keepAlive: true, maxSockets: concurrency });

function get(path) {
  return new Promise((resolve) => {
    const started = performance.now();
    http
      .get({ host: target.hostname, port: target.port, path, agent }, (res) => {
        res.resume();
        res.on("end", () => resolve({ ok: res.statusCode === 200, ms: performance.now() - started }));
      })
      .on("error", () => resolve({ ok: false, ms: performance.now() - started }));
  });
}

let next = 0;
const latencies = [];
let failed = 0;
const started = performance.now();
await Promise.all(
  Array.from({ length: concurrency }, async () => {
    while (next < total) {
      const n = next++;
      const r = await get(`/api/data?key=cold-${run}-${n}&${query}`);
      if (!r.ok) failed++;
      latencies.push(r.ms);
    }
  }),
);
const seconds = (performance.now() - started) / 1000;
latencies.sort((a, b) => a - b);
const pct = (p) => +latencies[Math.min(latencies.length - 1, Math.floor((p / 100) * latencies.length))].toFixed(2);
console.log(JSON.stringify({ requests_per_second: +(total / seconds).toFixed(1), p50_ms: pct(50), p95_ms: pct(95), p99_ms: pct(99), failed }));
agent.destroy();
