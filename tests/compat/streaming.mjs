// Streaming check for /dynamic: the shell (with the Suspense fallback) must
// reach the client before the boundary content, which the page delays with
// a 50 ms timer. A buffering runtime would deliver both at once.
//
//   node tests/compat/streaming.mjs --target http://127.0.0.1:3001 [--runs 10]
//     [--encoding gzip]
//
// With --encoding gzip the request accepts gzip, the response must be
// gzip-encoded, and it is decoded as it arrives (zlib's streaming gunzip):
// the shell must still be readable before the boundary content, which
// needs the server to flush the compressor after each chunk.
import http from "node:http";
import zlib from "node:zlib";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const target = new URL(args.target ?? "http://127.0.0.1:3001");
const runs = Number(args.runs ?? 10);
const encoding = args.encoding ?? "identity";
if (!["identity", "gzip"].includes(encoding)) throw new Error(`unsupported --encoding ${encoding}`);

function measure() {
  return new Promise((resolve, reject) => {
    const started = performance.now();
    const headers = { "accept-encoding": encoding };
    const req = http.get({ host: target.hostname, port: target.port, path: "/dynamic", agent: false, headers }, (res) => {
      const contentEncoding = res.headers["content-encoding"] ?? "identity";
      if (contentEncoding !== encoding) {
        reject(new Error(`asked for ${encoding}, got content-encoding ${contentEncoding}`));
        res.resume();
        return;
      }
      let text = "";
      let fallbackAt = null;
      let boundaryAt = null;
      let firstByteAt = null;
      let chunks = 0;
      let wireBytes = 0;
      res.on("data", (chunk) => {
        chunks++;
        wireBytes += chunk.length;
        firstByteAt ??= performance.now() - started;
      });
      const decoded = encoding === "gzip" ? res.pipe(zlib.createGunzip()) : res;
      decoded.on("data", (chunk) => {
        text += chunk;
        const now = performance.now() - started;
        if (fallbackAt === null && text.includes("Loading…")) fallbackAt = now;
        if (boundaryAt === null && text.includes("Streamed after Suspense boundary")) boundaryAt = now;
      });
      decoded.on("error", reject);
      decoded.on("end", () =>
        resolve({ chunks, wireBytes, decodedBytes: Buffer.byteLength(text), firstByteAt, fallbackAt, boundaryAt, total: performance.now() - started }),
      );
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
    encoding,
    runs,
    streamedRuns: streamed.length,
    medianFallbackMs: Number(median(results.map((r) => r.fallbackAt ?? NaN)).toFixed(1)),
    medianBoundaryMs: Number(median(results.map((r) => r.boundaryAt)).toFixed(1)),
    medianChunks: median(results.map((r) => r.chunks)),
    medianFirstByteMs: Number(median(results.map((r) => r.firstByteAt)).toFixed(1)),
    medianTotalMs: Number(median(results.map((r) => r.total)).toFixed(1)),
    medianWireBytes: median(results.map((r) => r.wireBytes)),
    medianDecodedBytes: median(results.map((r) => r.decodedBytes)),
  }),
);
process.exitCode = streamed.length === runs ? 0 : 1;
