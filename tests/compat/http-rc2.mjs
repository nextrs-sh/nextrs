// 0.1.0-rc.2 HTTP changes, compared between next start (the baseline) and
// nextrs (the candidate) serving the same build of fixtures/next-app:
//
//   node tests/compat/http-rc2.mjs --baseline http://127.0.0.1:3000 \
//     --candidate http://127.0.0.1:3001 --project fixtures/next-app [--json out.json]
//
// 1. RSC requests (`rsc: 1`) to route handlers and pages: status and
//    Location must match next start exactly (the `_rsc` cache-busting 307
//    and its target), for every method, router header combination and
//    query, and following the redirect must reach the same answer; route
//    handler JSON bodies must be equal.
// 2. Byte ranges on a static file: each case either matches next start
//    (status, Content-Range, Content-Length, bytes) or is one of the
//    intended differences listed with it (where next start answers 500 or
//    sends a compressed partial body; see crates/nextrs-server/src/range.rs).
//    Every candidate 206 is checked against the file's bytes, every 416
//    for its `bytes */<size>`.
// 3. Gzip of immutable responses (the compression cache): prerendered
//    HTML, RSC payloads and static chunks give the same compressed bytes on
//    every request, decoding to what next start sends; dynamic (SSR)
//    responses stay streamed (no Content-Length), and revalidation with the
//    compressed response's ETag gets a 304 from both servers.
import { request as httpRequest } from "node:http";
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const { baseline, candidate, project } = args;
if (!baseline || !candidate || !project) {
  console.error("usage: http-rc2.mjs --baseline URL --candidate URL --project DIR [--json FILE]");
  process.exit(2);
}

/** One raw request (no decoding, repeated headers as given). */
function send(base, path, { method = "GET", headers = [], body = null } = {}) {
  const url = new URL(path, base);
  return new Promise((resolve, reject) => {
    // Raw header lists are sent as given: Host too.
    const flat = ["host", url.host];
    for (const [name, value] of headers) flat.push(name, value);
    if (body !== null) flat.push("content-length", String(Buffer.byteLength(body)));
    const req = httpRequest(
      { host: url.hostname, port: url.port, path: url.pathname + url.search, method, headers: flat, agent: false },
      (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
        res.on("error", reject);
      },
    );
    req.on("error", reject);
    req.setTimeout(30000, () => req.destroy(new Error(`timeout ${method} ${path}`)));
    if (body !== null) req.write(body);
    req.end();
  });
}

const failures = [];
const report = { rsc: [], range: [], compression: [] };
const fail = (section, what, detail) => {
  failures.push({ section, what, detail });
  console.log(`FAIL [${section}] ${what}: ${JSON.stringify(detail)}`);
};

// ------------------------------------------------------------ 1. RSC
const routerHeaders = {
  rsc: [["rsc", "1"]],
  "rsc-uppercase-name": [["RSC", "1"]],
  prefetch: [["rsc", "1"], ["next-router-prefetch", "1"]],
  "prefetch-2": [["rsc", "1"], ["next-router-prefetch", "2"]],
  "segment-prefetch": [["rsc", "1"], ["next-router-prefetch", "1"], ["next-router-segment-prefetch", "/_tree"]],
  "state-tree": [["rsc", "1"], ["next-router-state-tree", "%5B%22%22%2C%7B%7D%5D"]],
  "next-url": [["rsc", "1"], ["next-url", "/books/1"]],
  "no-rsc": [],
  "rsc-0": [["rsc", "0"]],
  "rsc-true": [["rsc", "true"]],
  "rsc-repeated": [["rsc", "1"], ["rsc", "1"]],
  "rsc-empty": [["rsc", ""]],
};
const targets = [
  { path: "/api/health", methods: ["GET", "HEAD", "OPTIONS"], json: true },
  { path: "/api/echo", methods: ["POST"], json: true, body: "hello rc2" },
  { path: "/api/echo?mode=stream", methods: ["POST"], body: "streamed body" },
  { path: "/dynamic", methods: ["GET"] },
  { path: "/books/42", methods: ["GET"] },
  { path: "/", methods: ["GET"] },
];
const queries = ["", "a=1&b=2", "_rsc=bogus", "_rsc", "a=1&_rsc=bogus&b=%20x"];

function withQuery(path, query) {
  if (!query) return path;
  return path + (path.includes("?") ? "&" : "?") + query;
}

for (const target of targets) {
  for (const method of target.methods) {
    for (const [kind, headers] of Object.entries(routerHeaders)) {
      for (const query of queries) {
        const path = withQuery(target.path, query);
        const opts = { method, headers, body: target.body ?? null };
        const [b, c] = await Promise.all([send(baseline, path, opts), send(candidate, path, opts)]);
        const row = { method, path, kind, baseline: { status: b.status, location: b.headers.location ?? null }, candidate: { status: c.status, location: c.headers.location ?? null } };
        report.rsc.push(row);
        const what = `${method} ${path} [${kind}]`;
        if (b.status !== c.status || (b.headers.location ?? null) !== (c.headers.location ?? null)) {
          fail("rsc", what, row);
          continue;
        }
        if (b.status === 307) {
          if (c.body.length !== 0) fail("rsc", `${what}: redirect body`, c.body.length);
          // Follow once (same headers): the same answer. A bare `_rsc` (no
          // `=`) is kept by next start's setCacheBustingSearchParamWithHash
          // and read back first, so next start redirects that URL again:
          // the candidate must do exactly the same, and loop nowhere else.
          const loc = b.headers.location;
          const [b2, c2] = await Promise.all([send(baseline, loc, opts), send(candidate, loc, opts)]);
          row.followed = { baseline: b2.status, candidate: c2.status, location: c2.headers.location ?? null };
          if (b2.status !== c2.status || (b2.headers.location ?? null) !== (c2.headers.location ?? null)) {
            fail("rsc", `${what} -> ${loc}`, row.followed);
          }
          continue;
        }
        const type = (h) => (h["content-type"] ?? "").split(";")[0];
        if (type(b.headers) !== type(c.headers)) fail("rsc", `${what}: content-type`, { baseline: type(b.headers), candidate: type(c.headers) });
        if (target.json && method !== "HEAD" && b.status === 200 && b.body.toString() !== c.body.toString()) {
          fail("rsc", `${what}: body`, { baseline: b.body.toString(), candidate: c.body.toString() });
        }
        if (target.body && !target.json && method === "POST" && b.status === 200 && c.body.toString() !== target.body) {
          fail("rsc", `${what}: echoed body`, c.body.toString());
        }
      }
    }
  }
}
const redirects = report.rsc.filter((r) => r.baseline.status === 307).length;
const loops = report.rsc.filter((r) => r.followed?.baseline === 307).length;
console.log(`rsc: ${report.rsc.length} requests compared, ${redirects} cache-busting redirects (${loops} of them redirect again in next start too: a bare \`_rsc\` in the query)`);

// ------------------------------------------------------------ 2. Range
const chunks = join(project, ".next/static/chunks");
const largest = readdirSync(chunks)
  .filter((f) => f.endsWith(".js"))
  .map((f) => ({ f, size: statSync(join(chunks, f)).size }))
  .sort((a, b) => b.size - a.size)[0];
const asset = `/_next/static/chunks/${largest.f}`;
const file = readFileSync(join(chunks, largest.f));
const n = file.length;
const full = await send(candidate, asset, { headers: [["accept-encoding", "identity"]] });
const etag = full.headers.etag;
const modified = full.headers["last-modified"];
const later = new Date(Date.parse(modified) + 1000).toUTCString();

// expect: "match", or the intended { baseline, candidate } statuses.
const rangeCases = [
  { name: "first byte", headers: [["range", "bytes=0-0"]], expect: "match" },
  { name: "last byte", headers: [["range", `bytes=${n - 1}-${n - 1}`]], expect: "match" },
  { name: "middle", headers: [["range", "bytes=1000-1999"]], expect: "match" },
  { name: "whole file", headers: [["range", `bytes=0-${n - 1}`]], expect: "match" },
  { name: "open-ended", headers: [["range", "bytes=1000-"]], expect: "match" },
  { name: "suffix", headers: [["range", "bytes=-500"]], expect: "match" },
  // RFC 9110: a suffix longer than the representation selects all of it;
  // range-parser computes a negative start and next start answers 500.
  { name: "suffix longer than file", headers: [["range", `bytes=-${n + 10}`]], expect: { baseline: 500, candidate: 206 } },
  { name: "last-pos past the end", headers: [["range", "bytes=10-99999999999999999999"]], expect: "match" },
  { name: "whitespace and empty elements", headers: [["range", "bytes= 0-9,"]], expect: "match" },
  { name: "several ranges", headers: [["range", "bytes=0-0,5-9"]], expect: "match" },
  { name: "unknown unit", headers: [["range", "items=0-5"]], expect: "match" },
  { name: "unsatisfiable", headers: [["range", `bytes=${n}-`]], expect: { baseline: 500, candidate: 416 } },
  { name: "unsatisfiable suffix", headers: [["range", "bytes=-0"]], expect: { baseline: 500, candidate: 416 } },
  { name: "malformed", headers: [["range", "bytes=abc"]], expect: { baseline: 500, candidate: 200 } },
  { name: "last before first", headers: [["range", "bytes=5-2"]], expect: { baseline: 500, candidate: 200 } },
  { name: "HEAD", method: "HEAD", headers: [["range", "bytes=0-9"]], expect: { baseline: 206, candidate: 200 } },
  { name: "If-Range: Last-Modified", headers: [["range", "bytes=0-9"], ["if-range", modified]], expect: "match" },
  { name: "If-Range: later date", headers: [["range", "bytes=0-9"], ["if-range", later]], expect: { baseline: 206, candidate: 200 } },
  { name: "If-Range: weak ETag", headers: [["range", "bytes=0-9"], ["if-range", etag]], expect: { baseline: 206, candidate: 200 } },
  { name: "If-Range: other ETag", headers: [["range", "bytes=0-9"], ["if-range", '"other"']], expect: "match" },
  { name: "If-None-Match wins", headers: [["range", "bytes=0-9"], ["if-none-match", etag]], expect: "match" },
  { name: "If-Modified-Since wins", headers: [["range", "bytes=0-9"], ["if-modified-since", modified]], expect: "match" },
  { name: "If-None-Match mismatch", headers: [["range", "bytes=0-9"], ["if-none-match", '"x"']], expect: "match" },
  // next start compresses the partial body: its Content-Range describes the
  // identity bytes, its body is gzip. Same status, different bytes.
  { name: "gzip accepted", headers: [["range", "bytes=100-2099"], ["accept-encoding", "gzip"]], expect: { baseline: 206, candidate: 206, encoding: { baseline: "gzip", candidate: undefined } } },
  { name: "gzip + If-Range", headers: [["range", "bytes=0-9"], ["accept-encoding", "gzip"], ["if-range", modified]], expect: { baseline: 206, candidate: 200 } },
];

function checkCandidate(r, method, what) {
  const h = r.headers;
  if (r.status === 206) {
    const m = /^bytes (\d+)-(\d+)\/(\d+)$/.exec(h["content-range"] ?? "");
    if (!m) return fail("range", `${what}: content-range`, h["content-range"]);
    const [start, end, total] = m.slice(1).map(Number);
    if (total !== n || start > end || end >= n) fail("range", `${what}: content-range values`, h["content-range"]);
    if (Number(h["content-length"]) !== end - start + 1) fail("range", `${what}: content-length`, h["content-length"]);
    if (h["content-encoding"]) fail("range", `${what}: encoded partial`, h["content-encoding"]);
    if (method !== "HEAD" && !r.body.equals(file.subarray(start, end + 1))) fail("range", `${what}: bytes`, { start, end, got: r.body.length });
  } else if (r.status === 416) {
    if (h["content-range"] !== `bytes */${n}`) fail("range", `${what}: 416 content-range`, h["content-range"]);
  } else if (r.status === 200) {
    if (h["content-range"]) fail("range", `${what}: content-range on 200`, h["content-range"]);
    const body = h["content-encoding"] === "gzip" ? gunzipSync(r.body) : r.body;
    if (method !== "HEAD" && !body.equals(file)) fail("range", `${what}: full body`, body.length);
    if (method === "HEAD" && Number(h["content-length"]) !== n) fail("range", `${what}: HEAD length`, h["content-length"]);
  } else if (r.status !== 304) {
    fail("range", `${what}: status`, r.status);
  }
}

for (const c of rangeCases) {
  const method = c.method ?? "GET";
  const opts = { method, headers: c.headers.some(([k]) => k === "accept-encoding") ? c.headers : [...c.headers, ["accept-encoding", "identity"]] };
  const [b, r] = await Promise.all([send(baseline, asset, opts), send(candidate, asset, opts)]);
  const row = {
    name: c.name,
    request: opts.headers,
    baseline: { status: b.status, range: b.headers["content-range"] ?? null, length: b.headers["content-length"] ?? null, encoding: b.headers["content-encoding"] ?? null },
    candidate: { status: r.status, range: r.headers["content-range"] ?? null, length: r.headers["content-length"] ?? null, encoding: r.headers["content-encoding"] ?? null },
    expect: c.expect,
  };
  report.range.push(row);
  checkCandidate(r, method, c.name);
  if (c.expect === "match") {
    const same = b.status === r.status && row.baseline.range === row.candidate.range && (method === "HEAD" || b.status === 304 || b.body.equals(r.body));
    if (!same) fail("range", `${c.name}: differs from next start`, row);
  } else if (b.status !== c.expect.baseline || r.status !== c.expect.candidate) {
    fail("range", `${c.name}: not the recorded difference`, row);
  } else if (c.expect.encoding && (row.baseline.encoding ?? undefined) !== c.expect.encoding.baseline) {
    fail("range", `${c.name}: next start's encoding changed`, row);
  }
}
const intended = report.range.filter((r) => r.expect !== "match").length;
console.log(`range: ${report.range.length} cases, ${report.range.length - intended} equal to next start, ${intended} intended differences`);

// ------------------------------------------------------------ 3. gzip
const gz = [["accept-encoding", "gzip"]];
const immutable = [
  { path: "/", headers: [] },
  { path: "/?_rsc", headers: [["rsc", "1"]] },
  { path: asset, headers: [] },
  { path: "/no-such-page", headers: [] },
];
for (const t of immutable) {
  const identity = await send(baseline, t.path, { headers: [...t.headers, ["accept-encoding", "identity"]] });
  const bodies = [];
  for (let i = 0; i < 3; i++) {
    const r = await send(candidate, t.path, { headers: [...t.headers, ...gz] });
    if (r.headers["content-encoding"] !== "gzip") { fail("compression", `${t.path}: not gzip`, r.headers); break; }
    if (Number(r.headers["content-length"]) !== r.body.length) fail("compression", `${t.path}: content-length`, r.headers["content-length"]);
    if (!/accept-encoding/i.test(r.headers.vary ?? "")) fail("compression", `${t.path}: vary`, r.headers.vary);
    bodies.push(r.body);
    // Revalidation with the compressed response's validator, on both.
    if (i === 0 && r.headers.etag) {
      for (const base of [baseline, candidate]) {
        const v = await send(base, t.path, { headers: [...t.headers, ...gz, ["if-none-match", r.headers.etag]] });
        if (v.status !== 304) fail("compression", `${t.path}: revalidation on ${base}`, v.status);
      }
    }
  }
  if (bodies.length === 3 && !(bodies[0].equals(bodies[1]) && bodies[1].equals(bodies[2]))) fail("compression", `${t.path}: cached bytes differ`, bodies.map((b) => b.length));
  const decoded = bodies.length ? gunzipSync(bodies[0]) : Buffer.alloc(0);
  // Prerendered output and files are byte-identical across the servers.
  if (!decoded.equals(identity.body)) fail("compression", `${t.path}: decoded differs from next start`, { decoded: decoded.length, next: identity.body.length });
  report.compression.push({ path: t.path, identity_bytes: identity.body.length, gzip_bytes: bodies[0]?.length ?? null });
}
for (const path of ["/dynamic", "/books/42"]) {
  const r = await send(candidate, path, { headers: gz });
  if (r.headers["content-encoding"] !== "gzip") fail("compression", `${path}: not gzip`, r.headers["content-encoding"]);
  if (r.headers["content-length"] !== undefined) fail("compression", `${path}: dynamic output has a length (cached?)`, r.headers["content-length"]);
  if (!gunzipSync(r.body).toString().includes("</html>")) fail("compression", `${path}: decoded`, r.body.length);
  report.compression.push({ path, dynamic: true, gzip_bytes: r.body.length });
}
console.log(`compression: ${report.compression.length} responses checked`);

if (args.json) writeFileSync(args.json, JSON.stringify({ failures, ...report }, null, 2));
if (failures.length) {
  console.log(`${failures.length} failure(s)`);
  process.exit(1);
}
console.log("all checks passed");
