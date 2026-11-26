// The per-request work NextRS's cache handler did for an ISR / Partial
// Prerendering page before entries were reused (js/03_server.js
// #responseEntry): turn the stored envelope into a string, JSON.parse it,
// base64-decode the RSC payload and every segment. Measured on the real
// prerendered pages of the build, in Node.js's V8, single-threaded.
//
//   docker run --rm --cpus 1 -v $PWD/benchmarks/series1:/s1:ro \
//     --entrypoint node velasia-node:s1 /s1/envelope-decode.mjs
import fs from "node:fs";
import path from "node:path";

const dir = process.argv[2] ?? "/app/apps/web/.next/server/app";
const b64 = (b) => Buffer.from(b).toString("base64");
const unb64 = (t) => Buffer.from(t, "base64");
const rows = [];
for (const p of ["index", "destinations", "destinations/tokyo"]) {
  const html = fs.readFileSync(path.join(dir, `${p}.html`), "utf8");
  const rsc = fs.readFileSync(path.join(dir, `${p}.rsc`));
  const segments = [];
  const walk = (d, rel = "") => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const f = path.join(d, e.name);
      if (e.isDirectory()) walk(f, `${rel}/${e.name}`);
      else segments.push([`${rel}/${e.name}`, fs.readFileSync(f)]);
    }
  };
  walk(path.join(dir, `${p}.segments`));
  const meta = JSON.parse(fs.readFileSync(path.join(dir, `${p}.meta`), "utf8"));
  // The envelope as storeResponse writes it.
  const envelope = JSON.stringify({
    v: 1,
    value: { kind: "APP_PAGE", html, rscData: b64(rsc), postponed: meta.postponed, headers: meta.headers, status: 200, segmentData: segments.map(([k, v]) => [k, b64(v)]) },
    cacheControl: { revalidate: 3600, expire: 86400 },
  });
  const bytes = Buffer.from(envelope, "utf8");
  const decode = (text) => {
    const v = JSON.parse(text).value;
    return { html: v.html, rsc: unb64(v.rscData), seg: new Map(v.segmentData.map(([k, d]) => [k, unb64(d)])) };
  };
  const time = (fn, n) => {
    for (let i = 0; i < 200; i++) fn();
    const t = process.hrtime.bigint();
    for (let i = 0; i < n; i++) fn();
    return Number(process.hrtime.bigint() - t) / n / 1e6;
  };
  const toString = time(() => bytes.toString("utf8"), 500);
  const full = time(() => decode(bytes.toString("utf8")), 500);
  rows.push({ page: p, envelopeKiB: Math.round(bytes.length / 1024), postponed: meta.postponed != null, segments: segments.length, stringMs: Number(toString.toFixed(3)), stringParseDecodeMs: Number(full.toFixed(3)) });
}
console.log(JSON.stringify({ node: process.version, rows }, null, 1));
