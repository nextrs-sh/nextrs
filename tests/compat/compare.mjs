// Compares HTTP responses of a candidate server against the `next start`
// baseline for the fixture app.
//
//   node tests/compat/compare.mjs --baseline http://127.0.0.1:3000 \
//        --candidate http://127.0.0.1:3001 [--project fixtures/next-app] [--json out.json]
//
// Uses node:http directly so no Accept-Encoding is sent (next start would
// gzip; NextRS Phase 0 does not compress) and raw paths reach the server
// unnormalized. A candidate answering 501 with `x-nextrs-unsupported` is
// reported as UNSUPPORTED (explicit refusal), never as a match.
import http from "node:http";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const baseline = new URL(args.baseline ?? "http://127.0.0.1:3000");
const candidate = new URL(args.candidate ?? "http://127.0.0.1:3001");
const projectDir = path.resolve(args.project ?? "fixtures/next-app");

// Headers whose difference is transport-level, not semantic.
const IGNORED = new Set(["date", "connection", "keep-alive", "transfer-encoding", "content-length"]);

function request(base, { method = "GET", path: rawPath, headers = {}, body }) {
  return new Promise((resolve, reject) => {
    const req = http.request(
      { host: base.hostname, port: base.port, method, path: rawPath, headers, agent: false },
      (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => {
          const body = Buffer.concat(chunks);
          resolve({ status: res.statusCode, headers: res.headers, body });
        });
      },
    );
    req.on("error", reject);
    req.end(body);
  });
}

function normalizeHeaders(headers) {
  const out = {};
  for (const [name, value] of Object.entries(headers)) {
    if (IGNORED.has(name)) continue;
    let v = Array.isArray(value) ? [...new Set(value)].join(", ") : value;
    // next start lists x-nextjs-prerender twice; NextRS sends it once.
    if (name === "x-nextjs-prerender") v = [...new Set(v.split(/,\s*/))].join(", ");
    // Accept-Encoding is appended by next start's compression middleware.
    if (name === "vary") v = v.split(/,\s*/).filter((t) => t.toLowerCase() !== "accept-encoding").join(", ");
    if (v === "") continue;
    out[name] = v;
  }
  return out;
}

const sha = (buf) => createHash("sha256").update(buf).digest("hex").slice(0, 16);

const buildManifest = JSON.parse(readFileSync(path.join(projectDir, ".next/build-manifest.json"), "utf8"));
const chunk = `/_next/${buildManifest.rootMainFiles[0]}`;

const cases = [
  { name: "static page", path: "/" },
  { name: "static page HEAD", method: "HEAD", path: "/" },
  { name: "static page conditional", path: "/", etagFromBaseline: true },
  { name: "static chunk", path: chunk },
  { name: "missing chunk", path: "/_next/static/chunks/does-not-exist.js" },
  { name: "traversal ../", path: "/_next/static/../../BUILD_ID" },
  { name: "traversal %2e%2e", path: "/_next/static/%2e%2e/%2e%2e/prerender-manifest.json" },
  { name: "traversal encoded slash", path: "/_next/static/..%2f..%2frequired-server-files.json" },
  { name: "manifest not public", path: "/_next/prerender-manifest.json" },
  { name: "trailing slash redirect", path: "/books/42/" },
  { name: "unknown path -> not-found", path: "/does-not-exist" },
  { name: "framework route /_not-found", path: "/_not-found" },
  { name: "framework route /_global-error", path: "/_global-error" },
  { name: "pages data route", path: "/_next/data/x.json" },
  { name: "rsc without _rsc", path: "/", headers: { rsc: "1" } },
  { name: "rsc full payload", path: "/?_rsc", headers: { rsc: "1" } },
  {
    name: "rsc segment prefetch",
    path: "/?_rsc=_i_aeImnuN6u1u1r",
    headers: { rsc: "1", "next-router-prefetch": "1", "next-router-segment-prefetch": "/_tree" },
  },
  { name: "rsc wrong hash", path: "/?_rsc=bogus", headers: { rsc: "1", "next-router-prefetch": "1" } },
  { name: "POST static page", method: "POST", path: "/" },
  { name: "dynamic server component", path: "/dynamic" },
  { name: "dynamic segment", path: "/books/42" },
  { name: "route handler", path: "/api/health" },
  {
    name: "request context",
    path: "/request-info",
    headers: { "x-nextrs-probe": "hello", cookie: "theme=dark" },
  },
  { name: "image optimizer", path: "/_next/image?url=%2Ffoo.png&w=64&q=75" },
  // Phase 1A: dynamic routes rendered by the route handler.
  { name: "dynamic HEAD", method: "HEAD", path: "/dynamic" },
  { name: "dynamic rsc without _rsc", path: "/books/42", headers: { rsc: "1" } },
  {
    name: "dynamic rsc payload",
    path: "/books/42?_rsc",
    headers: { rsc: "1" },
    // next start itself emits a fresh random 22-character key per request
    // for the metadata boundary element (two next start runs differ here).
    normalize: (text) => text.replace(/(\["\$","\$L[0-9a-z]+",")[A-Za-z0-9_-]{22}(")/g, "$1<per-request-key>$2"),
  },
  { name: "dynamic segment, encoded param", path: "/books/%E2%9C%93" },
  { name: "dynamic segment, query string", path: "/books/7?ref=nextrs&x=1" },
  { name: "request context, no inputs", path: "/request-info" },
  {
    name: "request context, several cookies",
    path: "/request-info",
    headers: { "x-nextrs-probe": "two", cookie: "a=1; theme=light; b=2" },
  },
  { name: "route handler POST", method: "POST", path: "/api/health" },
  { name: "route handler HEAD", method: "HEAD", path: "/api/health" },
  // Phase 1B fixture routes.
  { name: "request body, content-length", method: "POST", path: "/api/echo", body: Buffer.alloc(300_000, 7) },
  {
    name: "request body, chunked",
    method: "POST",
    path: "/api/echo",
    headers: { "transfer-encoding": "chunked" },
    body: Buffer.alloc(200_000, 9),
  },
  { name: "request body, empty", method: "POST", path: "/api/echo" },
  {
    name: "request body streamed back",
    method: "POST",
    path: "/api/echo?mode=stream",
    headers: { "transfer-encoding": "chunked" },
    body: Buffer.alloc(500_000, 3),
  },
  { name: "large streamed response", path: "/api/stream?size=4194304" },
  { name: "after() route", path: "/api/after?id=compare", headers: { "x-nextrs-id": "compare" } },
  {
    name: "error boundary, rendered",
    path: "/isolation/boundary",
    headers: { "x-nextrs-id": "cmp-even", "x-nextrs-seq": "2" },
  },
  {
    name: "error boundary, thrown",
    path: "/isolation/boundary",
    headers: { "x-nextrs-id": "cmp-odd", "x-nextrs-seq": "1" },
  },
  { name: "generateStaticParams, prerendered", path: "/gsp/alpha" },
  { name: "generateStaticParams, unknown param", path: "/gsp/gamma" },
];

const results = [];
for (const testCase of cases) {
  const headers = { ...(testCase.headers ?? {}) };
  if (testCase.etagFromBaseline) {
    const first = await request(baseline, { path: testCase.path });
    headers["if-none-match"] = first.headers.etag;
  }
  const req = { method: testCase.method, path: testCase.path, headers, body: testCase.body };
  const [a, b] = await Promise.all([request(baseline, req), request(candidate, req)]);
  const ha = normalizeHeaders(a.headers);
  const hb = normalizeHeaders(b.headers);
  const headerDiffs = [...new Set([...Object.keys(ha), ...Object.keys(hb)])]
    .filter((k) => ha[k] !== hb[k])
    .map((k) => ({ header: k, baseline: ha[k] ?? null, candidate: hb[k] ?? null }));
  const bodyMatch = a.body.equals(b.body);
  const normalizedMatch =
    !bodyMatch && testCase.normalize && testCase.normalize(a.body.toString()) === testCase.normalize(b.body.toString());
  let verdict;
  if (b.status === 501 && b.headers["x-nextrs-unsupported"]) verdict = "UNSUPPORTED";
  else if (a.status === b.status && bodyMatch && headerDiffs.length === 0) verdict = "MATCH";
  else if (a.status === b.status && bodyMatch) verdict = "MATCH (header diffs)";
  else if (a.status === b.status && normalizedMatch && headerDiffs.length === 0) verdict = "MATCH (normalized)";
  else verdict = "DIFF";
  results.push({
    name: testCase.name,
    request: `${testCase.method ?? "GET"} ${testCase.path}`,
    verdict,
    baseline: { status: a.status, bytes: a.body.length, sha: sha(a.body) },
    candidate: {
      status: b.status,
      bytes: b.body.length,
      sha: sha(b.body),
      unsupported: b.headers["x-nextrs-unsupported"] ?? null,
    },
    headerDiffs,
  });
}

for (const r of results) {
  console.log(
    `${r.verdict.padEnd(20)} ${r.request.padEnd(70)} baseline=${r.baseline.status}/${r.baseline.bytes}B candidate=${r.candidate.status}/${r.candidate.bytes}B${r.candidate.unsupported ? ` [${r.candidate.unsupported}]` : ""}`,
  );
  for (const d of r.headerDiffs) {
    console.log(`${" ".repeat(21)}header ${d.header}: baseline=${JSON.stringify(d.baseline)} candidate=${JSON.stringify(d.candidate)}`);
  }
}
const tally = results.reduce((acc, r) => ({ ...acc, [r.verdict]: (acc[r.verdict] ?? 0) + 1 }), {});
console.log("\nSummary:", JSON.stringify(tally));
if (args.json) writeFileSync(args.json, JSON.stringify({ baseline: baseline.href, candidate: candidate.href, results }, null, 2));
process.exitCode = results.some((r) => r.verdict === "DIFF") ? 1 : 0;
