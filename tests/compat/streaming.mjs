// Streaming check for /dynamic: the shell (with the Suspense fallback) must
// reach the client before the boundary content, which the page delays with
// a 50 ms timer. A buffering runtime would deliver both at once.
//
//   node tests/compat/streaming.mjs --target http://127.0.0.1:3001 [--runs 10]
import http from "node:http";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const target = new URL(args.target ?? "http://127.0.0.1:3001");
const runs = Number(args.runs ?? 10);

function measure() {
  return new Promise((resolve, reject) => {
    const started = performance.now();
    const req = http.get({ host: target.hostname, port: target.port, path: "/dynamic", agent: false }, (res) => {
      let text = "";
      let fallbackAt = null;
      let boundaryAt = null;
      let chunks = 0;
      res.on("data", (chunk) => {
        chunks++;
        text += chunk;
        const now = performance.now() - started;
        if (fallbackAt === null && text.includes("Loading…")) fallbackAt = now;
        if (boundaryAt === null && text.includes("Streamed after Suspense boundary")) boundaryAt = now;
      });
      res.on("end", () => resolve({ chunks, fallbackAt, boundaryAt, total: performance.now() - started }));
    });
    req.on("error", reject);
  });
}

const results = [];
for (let i = 0; i < runs; i++) results.push(await measure());
const streamed = results.filter((r) => r.fallbackAt !== null && r.boundaryAt - r.fallbackAt >= 30);
const median = (xs) => xs.sort((a, b) => a - b)[Math.floor(xs.length / 2)];
console.log(
  JSON.stringify({
    target: target.href,
    runs,
    streamedRuns: streamed.length,
    medianFallbackMs: Number(median(results.map((r) => r.fallbackAt ?? NaN)).toFixed(1)),
    medianBoundaryMs: Number(median(results.map((r) => r.boundaryAt)).toFixed(1)),
    medianChunks: median(results.map((r) => r.chunks)),
  }),
);
process.exitCode = streamed.length === runs ? 0 : 1;
