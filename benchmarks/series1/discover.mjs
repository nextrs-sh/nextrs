// Runs in the tools container (Node.js 24). Learns, from the storefront
// under test, what the Railway Series 1 k6 scenarios learned in setup():
//
// * static-assets: every /_next/static URL referenced by `src`/`href` in
//   the HTML of `/` and `/destinations` (with any `?dpl=` query);
// * rsc: `/`, `/destinations` and the 12 destination pages with the
//   `RSC: 1` header and the `_rsc` key the server redirects to.
//
// Writes loadgen specs and a manifest (sizes, types) to /work/<dir>:
//   node /s1/discover.mjs http://s1-sf:8080 /work/specs
import http from "node:http";
import { mkdirSync, writeFileSync } from "node:fs";

const [base, out] = process.argv.slice(2);
mkdirSync(out, { recursive: true });

const DESTINATIONS = [
  "bangkok", "cebu", "davao", "ho-chi-minh-city", "hong-kong", "kuala-lumpur",
  "manila", "osaka", "seoul", "singapore", "taipei", "tokyo",
];
const RSC_PATHS = ["/", "/destinations", ...DESTINATIONS.map((d) => `/destinations/${d}`)];

/** A raw request (no decompression, no redirects). */
export function raw(url, headers = {}) {
  return new Promise((resolve, reject) => {
    const req = http.get(url, { headers }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      res.on("error", reject);
    });
    req.on("error", reject);
  });
}

const assets = new Set();
for (const page of ["/", "/destinations"]) {
  const r = await raw(base + page, { "accept-encoding": "identity" });
  if (r.status !== 200) throw new Error(`${page}: ${r.status}`);
  // Stop at a quote, space or comma: the first `dpl=` can sit in a srcset.
  for (const m of r.body.toString("utf8").matchAll(/(?:src|href)="(\/_next\/static\/[^"#\s,]+)"/g)) {
    assets.add(m[1].replaceAll("&amp;", "&"));
  }
}
if (assets.size === 0) throw new Error("no /_next/static URLs in / or /destinations");

const manifest = { assets: [], rscQuery: null, rsc: [] };
for (const path of [...assets].sort()) {
  const r = await raw(base + path, { "accept-encoding": "identity" });
  if (r.status !== 200) throw new Error(`${path}: ${r.status}`);
  const type = String(r.headers["content-type"] ?? "");
  const kind = /javascript/.test(type) ? "js" : /css/.test(type) ? "css" : /font/.test(type) ? "font" : "other";
  manifest.assets.push({ path, bytes: r.body.length, type, kind });
}

// The `_rsc` key: Next.js answers a bare `RSC: 1` request with a 307 to
// the URL that carries it.
const probe = await raw(`${base}/`, { rsc: "1" });
const location = String(probe.headers.location ?? "");
manifest.rscQuery = probe.status === 200 ? "_rsc" : location.slice(location.indexOf("?") + 1);
if (!manifest.rscQuery.startsWith("_rsc")) throw new Error(`rsc: unexpected ${probe.status} ${location}`);
for (const path of RSC_PATHS) {
  const r = await raw(`${base}${path}?${manifest.rscQuery}`, { rsc: "1", "accept-encoding": "identity" });
  if (r.status !== 200 || !String(r.headers["content-type"]).includes("text/x-component")) {
    throw new Error(`rsc ${path}: ${r.status} ${r.headers["content-type"]}`);
  }
  manifest.rsc.push({ path, bytes: r.body.length });
}

const spec = (requests) => JSON.stringify({ requests }, null, 1);
const gzip = [["accept-encoding", "gzip"]];
writeFileSync(`${out}/static-gzip.json`, spec(manifest.assets.map((a) => ({ path: a.path, headers: gzip, kind: a.kind }))));
writeFileSync(`${out}/static-identity.json`, spec(manifest.assets.map((a) => ({ path: a.path, kind: a.kind }))));
writeFileSync(
  `${out}/rsc-gzip.json`,
  spec(RSC_PATHS.map((p) => ({ path: `${p}?${manifest.rscQuery}`, headers: [["rsc", "1"], ...gzip], kind: "rsc" }))),
);
writeFileSync(
  `${out}/rsc-identity.json`,
  spec(RSC_PATHS.map((p) => ({ path: `${p}?${manifest.rscQuery}`, headers: [["rsc", "1"]], kind: "rsc" }))),
);
// The largest asset alone: concurrent misses for one uncached body.
const largest = [...manifest.assets].sort((a, b) => b.bytes - a.bytes)[0];
writeFileSync(`${out}/static-largest.json`, spec([{ path: largest.path, headers: gzip, kind: largest.kind }]));
writeFileSync(`${out}/manifest.json`, JSON.stringify(manifest, null, 1));
console.log(JSON.stringify({ assets: manifest.assets.length, rscQuery: manifest.rscQuery, largest: largest.bytes }));
