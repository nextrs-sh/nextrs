// Runs in the tools container. Records what the storefront under test
// answers for every request the Series 1 workloads make, plus HTML pages,
// HEAD, conditional and range requests, so two runtimes can be compared
// (compare.mjs): status, the headers that matter for caching and
// encoding, the compressed size, and a SHA-256 of the decoded body.
//
//   node /s1/capture.mjs http://s1-sf:8080 /work/specs /work/capture-<runtime>.json
import crypto from "node:crypto";
import http from "node:http";
import { readFileSync, writeFileSync } from "node:fs";
import zlib from "node:zlib";

const [base, specs, out] = process.argv.slice(2);
const manifest = JSON.parse(readFileSync(`${specs}/manifest.json`, "utf8"));

function raw(path, headers = {}, method = "GET") {
  return new Promise((resolve, reject) => {
    const req = http.request(base + path, { method, headers }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      res.on("error", reject);
    });
    req.on("error", reject);
    req.end();
  });
}

const HEADERS = [
  "content-type", "content-encoding", "cache-control", "vary", "etag", "last-modified",
  "accept-ranges", "content-range", "x-nextjs-cache", "x-nextjs-prerender", "x-nextjs-postponed",
  "x-nextjs-stale-time", "x-nextjs-rewritten-path", "x-nextjs-rewritten-query", "x-powered-by",
];

async function record(name, path, headers, method = "GET") {
  const r = await raw(path, headers, method);
  let decoded = r.body;
  let decodeError = null;
  if (r.headers["content-encoding"] === "gzip") {
    try {
      decoded = zlib.gunzipSync(r.body);
    } catch (error) {
      decodeError = String(error.message);
    }
  }
  const kept = {};
  for (const h of HEADERS) if (r.headers[h] !== undefined) kept[h] = r.headers[h];
  return {
    name, path, method, request: headers, status: r.status, headers: kept,
    wireBytes: r.body.length, decodedBytes: decoded.length,
    sha256: crypto.createHash("sha256").update(decoded).digest("hex"),
    decodeError,
    // RSC payloads: first bytes, to see what kind of payload it is.
    preview: String(kept["content-type"] ?? "").includes("x-component") ? decoded.subarray(0, 80).toString("utf8") : undefined,
  };
}

const results = [];
for (const a of manifest.assets) {
  results.push(await record("static gzip", a.path, { "accept-encoding": "gzip" }));
  results.push(await record("static identity", a.path, { "accept-encoding": "identity" }));
}
const big = [...manifest.assets].sort((a, b) => b.bytes - a.bytes)[0];
results.push(await record("static HEAD gzip", big.path, { "accept-encoding": "gzip" }, "HEAD"));
const first = results.find((r) => r.path === big.path && r.name === "static identity");
if (first?.headers.etag) {
  results.push(await record("static If-None-Match", big.path, { "if-none-match": first.headers.etag }));
}
results.push(await record("static range", big.path, { range: "bytes=0-99" }));
results.push(await record("static range gzip", big.path, { range: "bytes=0-99", "accept-encoding": "gzip" }));
for (const r of manifest.rsc) {
  const q = `${r.path}?${manifest.rscQuery}`;
  results.push(await record("rsc gzip", q, { rsc: "1", "accept-encoding": "gzip" }));
  results.push(await record("rsc identity", q, { rsc: "1", "accept-encoding": "identity" }));
}
results.push(await record("rsc without _rsc", "/destinations", { rsc: "1" }));
for (const page of ["/", "/destinations", "/destinations/tokyo"]) {
  results.push(await record("html gzip", page, { "accept-encoding": "gzip" }));
  results.push(await record("html identity", page, { "accept-encoding": "identity" }));
}
writeFileSync(out, JSON.stringify(results, null, 1));
console.log(JSON.stringify({ recorded: results.length }));
