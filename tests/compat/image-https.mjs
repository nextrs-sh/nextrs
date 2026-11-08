// /_next/image with HTTPS sources: next start (trusting the suite's test CA
// through NODE_EXTRA_CA_CERTS) against NextRS (--net-ca-file), on
// fixtures/next-image with local HTTPS upstreams (no public hosts):
//
//   https://localhost:<https-port>    certificate from the test CA (DNS:localhost)
//   https://127.0.0.1:<https-port>    the same server: the name does not match
//   https://localhost:<https-port+1>  certificate from a CA nobody trusts
//
// Answers are compared as tests/compat/image.mjs compares them (status,
// headers, decoded format and size), and every scenario asserts its own
// outcome on both servers.
//
//   node tests/compat/image-https.mjs --baseline URL --candidate URL --project fixtures/next-image
//     --certs DIR [--upstream-port 4317] [--https-port 4318] [--json out.json]
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import http from "node:http";
import { parseArgs, differences } from "./routing-lib.mjs";
import { startUpstream } from "./image-upstream.mjs";

const args = parseArgs();
const project = args.project ?? "fixtures/next-image";
const upstreamPort = Number(args["upstream-port"] ?? 4317);
const httpsPort = Number(args["https-port"] ?? upstreamPort + 1);
const certs = args.certs;
const sharp = createRequire(resolve(project, "package.json"))("sharp");
const photos = join(project, "public/photos");
const nonce = `${process.pid}-${Date.now()}`;
const pem = (name) => readFileSync(join(certs, name));
const UPSTREAM_INVALID = '"url" parameter is valid but upstream response is invalid';
const NOT_ALLOWED = '"url" parameter is not allowed';
const TIMED_OUT = '"url" parameter is valid but upstream response timed out';
const HEADERS = ["vary", "cache-control", "content-type", "content-disposition", "x-nextjs-cache"];

const src = (origin, path) => `${origin}${path}${path.includes("?") ? "&" : "?"}n=${nonce}`;
const image = (url, w = 640) => `/_next/image?url=${encodeURIComponent(url)}&w=${w}&q=75`;
const trusted = `https://localhost:${httpsPort}`;
const mismatch = `https://127.0.0.1:${httpsPort}`;
const untrusted = `https://localhost:${httpsPort + 1}`;

function request(base, path, { abortAfterMs } = {}) {
  return new Promise((resolveRequest) => {
    const u = new URL(base);
    const req = http.request({ host: u.hostname, port: u.port, path, headers: { accept: "image/webp,*/*" }, agent: false }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolveRequest({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
    });
    req.setTimeout(30000, () => req.destroy(new Error("client timeout")));
    req.on("error", (e) => resolveRequest({ error: String(e.code ?? e.message) }));
    if (abortAfterMs) setTimeout(() => req.destroy(), abortAfterMs);
    req.end();
  });
}

async function observe(base, path) {
  const r = await request(base, path);
  if (r.error) return { error: r.error };
  const headers = {};
  for (const name of HEADERS) if (r.headers[name] !== undefined) headers[name] = r.headers[name];
  const out = { status: r.status, headers };
  if (r.status === 200 && String(r.headers["content-type"]).startsWith("image/")) {
    const meta = await sharp(r.body).metadata();
    out.image = { format: meta.format, width: meta.width, height: meta.height };
  } else {
    out.body = r.body.toString("utf8").slice(0, 200);
  }
  return out;
}

const optimized = (w, h) => (o) =>
  o.status === 200 && o.image?.format === "webp" && o.image.width === w && o.image.height === h ? null : JSON.stringify(o).slice(0, 300);
const refused = (status, body) => (o) => (o.status === status && (body === undefined || o.body === body) ? null : JSON.stringify(o).slice(0, 300));
const failed = (o) => (o.status >= 500 && !o.image ? null : `expected a 5xx without an image: ${JSON.stringify(o).slice(0, 300)}`);

async function main() {
  const http1 = await startUpstream(photos, upstreamPort);
  const tlsUp = await startUpstream(photos, httpsPort, "0.0.0.0", { key: pem("localhost.key"), cert: pem("localhost.pem") });
  const bad = await startUpstream(photos, httpsPort + 1, "0.0.0.0", { key: pem("untrusted.key"), cert: pem("untrusted.pem") });
  const scenarios = [];
  const add = (name, sc) => scenarios.push({ name, ...sc });
  const get = (url, w) => (base) => observe(base, image(url, w));

  add("authorized https image", { run: get(src(trusted, "/img/photo.jpg"), 640), expect: optimized(640, 480) });
  add("invalid certificate (untrusted CA)", {
    async run(base) {
      const before = bad.count("/img/photo.jpg");
      const o = await observe(base, image(src(untrusted, "/img/photo.jpg"), 640));
      return { ...o, requestsReached: bad.count("/img/photo.jpg") - before };
    },
    // The handshake fails: no HTTP request ever reaches the server.
    expect: (o) => failed(o) ?? (o.requestsReached ? "the request reached the server" : null),
  });
  add("hostname mismatch", {
    async run(base) {
      const before = tlsUp.count("/img/photo.jpg");
      const o = await observe(base, image(src(mismatch, "/img/photo.jpg"), 640));
      return { ...o, requestsReached: tlsUp.count("/img/photo.jpg") - before };
    },
    expect: (o) => failed(o) ?? (o.requestsReached ? "the request reached the server" : null),
  });
  add("redirect from https to https", { run: get(src(trusted, "/img/redirect.jpg"), 384), expect: optimized(384, 288) });
  add("redirect from https to an allowed http origin", {
    run: get(src(trusted, `/img/redirect-downgrade.jpg?http=${upstreamPort}`), 384),
    expect: optimized(384, 288),
  });
  add("redirect to an unauthorized destination", {
    kind: "known-diff",
    reason: "next start (dangerouslyAllowLocalIP) follows a redirect to 127.0.0.2 (whose TLS name check then fails); NextRS (--net-allow-addr 127.0.0.1 and ::1) never connects",
    async run(base) {
      const o = await observe(base, image(src(trusted, "/img/redirect-other-addr.jpg"), 384));
      return o;
    },
    expect: (o) => (o.status !== 200 ? null : "served"),
    expectCandidate: refused(400, NOT_ALLOWED),
  });
  add("body above maximumResponseBody", { run: get(src(trusted, "/img/oversized.jpg"), 384), expect: refused(413, UPSTREAM_INVALID) });
  add("slow head times out", { run: get(src(trusted, "/img/slow-head.jpg"), 384), expect: refused(504, TIMED_OUT) });
  add("slow streaming body within the limits", { run: get(src(trusted, "/img/trickle.jpg"), 384), expect: optimized(384, 288) });
  add("client abort", {
    kind: "candidate",
    reason: "NextRS cancels the download when the last client leaves (next start lets it finish)",
    async run(base) {
      const before = tlsUp.aborted();
      const r = await request(base, image(src(trusted, "/img/stream-long.jpg"), 384), { abortAfterMs: 400 });
      let upstreamAborted = 0;
      for (let i = 0; i < 40 && !upstreamAborted; i++) {
        await new Promise((r2) => setTimeout(r2, 100));
        upstreamAborted = tlsUp.aborted() - before;
      }
      const after = await observe(base, image(src(trusted, "/img/photo.jpg?after-abort"), 256));
      const ok = r.error !== undefined && upstreamAborted === 1 && !optimized(256, 192)(after);
      return { ok, detail: { client: r.error ?? r.status, upstreamAborted, after: after.status } };
    },
  });
  add("concurrent downloads", {
    async run(base, side) {
      const before = tlsUp.count("/img/photo.jpg");
      const distinct = Array.from({ length: 24 }, (_, i) => image(src(trusted, `/img/photo.jpg?d=${side}-${i}`), 256));
      const same = Array.from({ length: 50 }, () => image(src(trusted, `/img/photo.jpg?s=${side}`), 384));
      const results = await Promise.all([...distinct, ...same].map((p) => request(base, p)));
      const o = { statuses: [...new Set(results.map((x) => x.status ?? x.error))] };
      // Not compared: next start coalesced 50 identical requests into one
      // fetch alone (tests/compat/image.mjs) but not beside 24 other
      // downloads (50 fetches measured); NextRS must, always.
      Object.defineProperty(o, "fetches", { value: tlsUp.count("/img/photo.jpg") - before, enumerable: false });
      return o;
    },
    expect: (o) => (o.statuses.length === 1 && o.statuses[0] === 200 ? null : JSON.stringify(o)),
    // 24 distinct sources, and one for the 50 identical requests.
    expectCandidate: (o) => (o.statuses.length === 1 && o.statuses[0] === 200 && o.fetches === 25 ? null : `${JSON.stringify(o)} fetches ${o.fetches}`),
  });

  const results = [];
  let failures = 0;
  for (const sc of scenarios) {
    let verdict, detail;
    try {
      if (sc.kind === "candidate") {
        const r = await sc.run(args.candidate, "candidate");
        verdict = r.ok ? "PASS" : "FAIL";
        detail = { ...r.detail, reason: sc.reason };
      } else {
        const [a, b] = await Promise.all([sc.run(args.baseline, "baseline"), sc.run(args.candidate, "candidate")]);
        const diffs = differences(a, b);
        const oracleWrong = sc.expect ? sc.expect(a) : null;
        const candidateWrong = (sc.expectCandidate ?? sc.expect)?.(b) ?? null;
        if (sc.kind === "known-diff") verdict = candidateWrong ? "FAIL" : diffs.length ? "KNOWN-DIFF" : "MATCH";
        else verdict = oracleWrong ? "ORACLE-FAIL" : candidateWrong ? "FAIL" : diffs.length ? "DIFF" : "MATCH";
        detail = { diffs, oracleWrong, candidateWrong, baseline: a, candidate: b, reason: sc.reason };
      }
    } catch (e) {
      verdict = "ERROR";
      detail = String(e?.stack ?? e);
    }
    if (["DIFF", "FAIL", "ERROR", "ORACLE-FAIL"].includes(verdict)) failures++;
    results.push({ name: sc.name, verdict, detail });
    console.log(`${verdict.padEnd(10)} https: ${sc.name}`);
    if (verdict === "DIFF") for (const d of detail.diffs) console.log(`    ${d}`);
    if (verdict === "ORACLE-FAIL") console.log(`    next start: ${detail.oracleWrong}`);
    if (verdict === "FAIL") console.log(`    NextRS: ${detail.candidateWrong ?? JSON.stringify(detail).slice(0, 400)}`);
    if (verdict === "ERROR") console.log(`    ${detail.slice(0, 600)}`);
    if (verdict === "KNOWN-DIFF") console.log(`    (${sc.reason})`);
  }
  await Promise.all([http1.close(), tlsUp.close(), bad.close()]);
  const count = (v) => results.filter((r) => r.verdict === v).length;
  const summary = { title: "image-https", total: results.length, match: count("MATCH"), pass: count("PASS"), knownDiff: count("KNOWN-DIFF"), diff: count("DIFF"), fail: count("FAIL"), oracleFail: count("ORACLE-FAIL"), error: count("ERROR") };
  console.log(`${summary.title}: ${summary.match} MATCH, ${summary.pass} PASS, ${summary.knownDiff} KNOWN-DIFF, ${summary.diff} DIFF, ${summary.fail} FAIL, ${summary.oracleFail} ORACLE-FAIL, ${summary.error} ERROR`);
  if (args.json) writeFileSync(args.json, JSON.stringify({ summary, results }, null, 2));
  process.exit(failures ? 1 : 0);
}

main();
