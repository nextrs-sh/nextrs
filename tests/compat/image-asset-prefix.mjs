// next/image under assetPrefix: next start against NextRS on an
// assetPrefix build of fixtures/next-image (NEXTRS_VARIANT=asset-prefix:
// "/cdn"; asset-prefix-url: http://127.0.0.1:<cdn-port>, served by a local
// stand-in for a CDN that answers /_next/static/* from the build).
//
// The build decides the URLs (the static import's src carries the prefix;
// the loader's /_next/image path does not); the runtime must serve them.
// Compared on both servers: the image URLs the page renders (src, srcset,
// preload links), every /_next/image URL it renders (status, type, decoded
// size), the prefixed static media and JavaScript, and a remote source.
//
//   node tests/compat/image-asset-prefix.mjs --baseline URL --candidate URL --project DIR
//     --variant asset-prefix|asset-prefix-url [--upstream-port 4317] [--cdn-port 4320] [--json out]
import { createRequire } from "node:module";
import { readFileSync, writeFileSync, existsSync, statSync } from "node:fs";
import { join, resolve, normalize } from "node:path";
import http from "node:http";
import { parseArgs, differences } from "./routing-lib.mjs";
import { startUpstream } from "./image-upstream.mjs";

const args = parseArgs();
const project = resolve(args.project ?? "fixtures/next-image");
const variant = args.variant ?? "asset-prefix";
const upstreamPort = Number(args["upstream-port"] ?? 4317);
const cdnPort = Number(args["cdn-port"] ?? upstreamPort + 3);
const sharp = createRequire(join(project, "package.json"))("sharp");
const dist = join(project, `.next-${variant}`);
const prefix = variant === "asset-prefix" ? "/cdn" : `http://127.0.0.1:${cdnPort}`;

function request(url, accept = "image/webp,*/*") {
  return new Promise((done) => {
    const u = new URL(url);
    http
      .get({ host: u.hostname, port: u.port, path: u.pathname + u.search, headers: { accept }, agent: false }, (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => done({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      })
      .on("error", (e) => done({ error: String(e.code ?? e.message) }));
  });
}

// A CDN stand-in: /_next/static/* from the build's static directory.
function startCdn() {
  const root = join(dist, "static");
  const server = http.createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, "http://cdn").pathname);
    const file = normalize(join(root, path.replace(/^\/_next\/static\//, "")));
    if (!path.startsWith("/_next/static/") || !file.startsWith(root) || !existsSync(file) || !statSync(file).isFile()) {
      res.writeHead(404).end();
      return;
    }
    const type = file.endsWith(".js") ? "application/javascript" : file.endsWith(".css") ? "text/css" : file.endsWith(".jpg") ? "image/jpeg" : file.endsWith(".png") ? "image/png" : "application/octet-stream";
    res.writeHead(200, { "content-type": type, "cache-control": "public, max-age=31536000, immutable" }).end(readFileSync(file));
  });
  return new Promise((r) => server.listen(cdnPort, "127.0.0.1", () => r(server)));
}

const attr = (html, re) => [...html.matchAll(re)].map((m) => m[1].replaceAll("&amp;", "&"));
function imageUrls(html) {
  const src = attr(html, /<img[^>]*?\ssrc="([^"]+)"/g);
  const srcset = attr(html, /\ssrcSet="([^"]+)"/gi).flatMap((s) => s.split(/,\s*/).map((c) => c.trim().split(/\s+/)[0]));
  const preload = attr(html, /<link[^>]*rel="preload"[^>]*imageSrcSet="([^"]+)"/gi).flatMap((s) => s.split(/,\s*/).map((c) => c.trim().split(/\s+/)[0]));
  const scripts = attr(html, /<script[^>]*\ssrc="([^"]+)"/g);
  return { src: [...new Set(src)].sort(), srcset: [...new Set(srcset)].sort(), preload: [...new Set(preload)].sort(), scripts: [...new Set(scripts)].sort() };
}
// The upstream's nonce-free URL, and NextRS/next start's own origin removed.
const strip = (u) => u.replace(/([?&])n=[^&]*/g, "$1").replace(/[?&]$/, "");

async function describe(r) {
  if (r.error) return { error: r.error };
  const out = { status: r.status, type: r.headers["content-type"] ?? null };
  if (r.status === 200 && String(out.type).startsWith("image/")) {
    const m = await sharp(r.body).metadata();
    out.image = { format: m.format, width: m.width, height: m.height };
  }
  return out;
}

async function main() {
  const cdn = await startCdn();
  const up = await startUpstream(join(project, "public/photos"), upstreamPort);
  const results = [];
  let failures = 0;
  const record = (name, verdict, detail) => {
    results.push({ name, verdict, detail });
    if (["DIFF", "FAIL", "ERROR"].includes(verdict)) failures++;
    console.log(`${verdict.padEnd(10)} ${variant}: ${name}`);
    if (verdict !== "MATCH" && verdict !== "PASS") console.log(`    ${JSON.stringify(detail).slice(0, 700)}`);
  };
  const compare = (name, a, b, expect) => {
    const diffs = differences(a, b);
    const wrong = expect?.(a) ?? null;
    const wrongB = expect?.(b) ?? null;
    record(name, wrong ? "ORACLE-FAIL" : wrongB ? "FAIL" : diffs.length ? "DIFF" : "MATCH", { diffs, wrong, wrongB, baseline: a, candidate: b });
    if (wrong) failures++;
  };
  try {
    const [pa, pb] = await Promise.all([request(`${args.baseline}/`, "text/html"), request(`${args.candidate}/`, "text/html")]);
    const ua = imageUrls(pa.body.toString()), ub = imageUrls(pb.body.toString());
    compare("page renders the same image URLs", { status: pa.status, ...ua }, { status: pb.status, ...ub }, (o) =>
      o.status !== 200 ? `status ${o.status}` : o.src.some((s) => s.includes("/_next/static/media/") && !s.startsWith(prefix)) ? "static media without the prefix" : null);
    // The build puts the prefix on static media and scripts, never on the
    // optimizer's own path.
    const staticSrc = ua.src.find((s) => s.includes("/_next/static/media/"));
    const optimized = [...new Set([...ua.src, ...ua.srcset, ...ua.preload].filter((s) => s.startsWith("/_next/image")))];
    record("optimizer path is not prefixed", optimized.length > 0 && !ua.src.some((s) => s.startsWith(`${prefix}/_next/image`)) ? "PASS" : "FAIL", { optimized: optimized.length, staticSrc });
    for (const path of optimized) {
      const [a, b] = await Promise.all([request(args.baseline + path).then(describe), request(args.candidate + path).then(describe)]);
      compare(`image ${strip(decodeURIComponent(path)).replace(/^\/_next\/image\?url=/, "")}`, a, b, (o) => (o.status === 200 && o.image ? null : JSON.stringify(o)));
    }
    if (staticSrc) {
      // A path prefix is served by the server itself; an origin by the CDN.
      const at = (base) => (staticSrc.startsWith("http") ? staticSrc : base + staticSrc);
      const [a, b] = await Promise.all([request(at(args.baseline)).then(describe), request(at(args.candidate)).then(describe)]);
      compare("prefixed static media", a, b, (o) => (o.status === 200 && o.image ? null : JSON.stringify(o)));
    }
    const script = ua.scripts.find((s) => s.includes("/_next/static/"));
    if (script && !script.startsWith("http")) {
      const [a, b] = await Promise.all([request(args.baseline + script), request(args.candidate + script)]);
      compare("prefixed JavaScript", { status: a.status, type: a.headers?.["content-type"] }, { status: b.status, type: b.headers?.["content-type"] }, (o) => (o.status === 200 ? null : `status ${o.status}`));
    }
    const remote = `/_next/image?url=${encodeURIComponent(`http://127.0.0.1:${upstreamPort}/img/photo.jpg?n=ap-${process.pid}`)}&w=384&q=75`;
    const [ra, rb] = await Promise.all([request(args.baseline + remote).then(describe), request(args.candidate + remote).then(describe)]);
    compare("remote source", ra, rb, (o) => (o.status === 200 && o.image?.width === 384 ? null : JSON.stringify(o)));
  } catch (e) {
    record("suite", "ERROR", String(e?.stack ?? e));
  }
  await up.close();
  cdn.close();
  const count = (v) => results.filter((r) => r.verdict === v).length;
  const summary = { title: `image-${variant}`, total: results.length, match: count("MATCH"), pass: count("PASS"), diff: count("DIFF"), fail: count("FAIL"), oracleFail: count("ORACLE-FAIL"), error: count("ERROR") };
  console.log(`${summary.title}: ${summary.match} MATCH, ${summary.pass} PASS, ${summary.diff} DIFF, ${summary.fail} FAIL, ${summary.oracleFail} ORACLE-FAIL, ${summary.error} ERROR`);
  if (args.json) writeFileSync(args.json, JSON.stringify({ summary, results }, null, 2));
  process.exit(failures ? 1 : 0);
}

main();
