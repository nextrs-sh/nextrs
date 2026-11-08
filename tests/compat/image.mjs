// /_next/image: next start (the oracle, sharp) against NextRS (Rust), on
// fixtures/next-image with a deterministic local upstream (no public
// hosts).
//
// Independently encoded images are not byte-identical, so an image answer
// is compared by what it decodes to: format, dimensions, channels and
// alpha must be equal; pixels must agree within tolerances fixed before any
// comparison was run (TOLERANCE below). Headers are compared exactly except
// the ETag value (a hash of each server's own bytes) and transport
// framing. Every scenario also asserts its own outcome (`expect`) on both
// servers: right dimensions, a refusal that really refused, a forbidden
// source that was never fetched.
//
//   node tests/compat/image.mjs --baseline URL --candidate URL --project fixtures/next-image
//     [--variant default|alt] [--upstream-port 4317] [--metrics URL] [--json out.json]
//     [--only a,b] [--quick]
import { createRequire } from "node:module";
import { readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";
import http from "node:http";
import { parseArgs, differences, sleep } from "./routing-lib.mjs";
import { startUpstream } from "./image-upstream.mjs";

const args = parseArgs();
const project = args.project ?? "fixtures/next-image";
const variant = args.variant ?? "default";
const alt = variant === "alt";
const prefix = alt ? "/base" : "";
const upstreamPort = Number(args["upstream-port"] ?? process.env.NEXTRS_IMAGE_UPSTREAM_PORT ?? 4317);
const sharp = createRequire(resolve(project, "package.json"))("sharp");
const photos = join(project, "public/photos");
const source = (name) => readFileSync(join(photos, name));
const sha = (buf) => createHash("sha256").update(buf).digest("hex").slice(0, 16);
const nonce = `${process.pid}-${Date.now()}`;
const remote = (path) => `http://127.0.0.1:${upstreamPort}${path}${path.includes("?") ? "&" : "?"}n=${nonce}`;
const MIN_TTL = 2;

// Decoded pixels may differ by encoder and resampler, never by geometry,
// orientation, colour space or transparency.
//
// TOLERANCE (fixed before the first run) compares the two outputs
// directly; it is reported, not decisive. The first run showed it fails on
// faithful outputs: the fixtures carry per-pixel noise, and sharp shrinks
// JPEGs on load (DCT scaling) before Lanczos3 while NextRS applies Lanczos3
// to the full image; sharp's PNG output is palette-quantized. The verdict
// therefore uses FIDELITY: each output against an unencoded Lanczos3
// reference made from the fully decoded source, NextRS at least as
// faithful as next start (within the margins), plus QUADRANTS: the mean
// colour of each quadrant (and alpha) equal within 8 levels, which catches
// orientation, colour-space, channel and transparency errors.
export const TOLERANCE = { mae: 6, alphaMae: 6, ssim: 0.95 };
export const FIDELITY = { ssimMargin: 0.02, ssimFloor: 0.97, maeMargin: 2, maeFloor: 3 };
export const QUADRANTS = { maxDelta: 8 };

const ACCEPT = {
  webp: "image/webp,*/*",
  avif: "image/avif,image/webp,*/*",
  any: "*/*",
  chrome: "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
};

// With basePath a local source carries it too (the optimizer's own request
// is routed like any other); `bare` keeps the URL as given.
function image(url, { w = 640, q = 75, extra = "", bare = false } = {}) {
  const src = alt && !bare && url.startsWith("/") && !url.startsWith("//") && !url.startsWith(prefix) ? prefix + url : url;
  return `${prefix}/_next/image?url=${encodeURIComponent(src)}&w=${w}&q=${q}${extra}`;
}

function rawRequest(base, { method = "GET", path, headers = {} }) {
  return new Promise((resolve) => {
    const u = new URL(base);
    const req = http.request({ host: u.hostname, port: u.port, method, path, headers, agent: false }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      res.on("error", (e) => resolve({ error: String(e.code ?? e.message) }));
    });
    req.setTimeout(30000, () => req.destroy(new Error("client timeout")));
    req.on("error", (e) => resolve({ error: String(e.code ?? e.message) }));
    req.end();
  });
}

const HEADERS = ["vary", "cache-control", "content-type", "content-disposition", "content-security-policy", "x-nextjs-cache", "retry-after"];

async function decode(buf) {
  try {
    const img = sharp(buf, { limitInputPixels: false, animated: false });
    const meta = await img.metadata();
    const { data, info } = await sharp(buf, { limitInputPixels: false }).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
    return {
      summary: { format: meta.format, width: info.width, height: info.height, channels: meta.channels, hasAlpha: Boolean(meta.hasAlpha) },
      pixels: { data, width: info.width, height: info.height },
    };
  } catch (e) {
    return { summary: { undecodable: String(e.message).slice(0, 60) } };
  }
}

// One request, as both servers are compared on it.
async function observe(base, request, opts = {}) {
  const r = await rawRequest(base, typeof request === "string" ? { path: request, headers: { accept: ACCEPT.webp } } : request);
  if (r.error) return { error: r.error };
  const headers = {};
  for (const name of HEADERS) if (r.headers[name] !== undefined) headers[name] = r.headers[name];
  // next start compresses compressible bodies (SVG): transport, not semantics.
  if (headers.vary) headers.vary = headers.vary.split(/,\s*/).filter((t) => t.toLowerCase() !== "accept-encoding").join(", ");
  const out = { status: r.status, headers, etag: r.headers.etag ? "present" : "absent" };
  const type = String(r.headers["content-type"] ?? "");
  if (r.status === 200 && type.startsWith("image/")) {
    const decoded = type === "image/svg+xml" ? { summary: { svg: true } } : await decode(r.body);
    out.image = decoded.summary;
    if (opts.source) out.sameAsSource = sha(r.body) === sha(source(opts.source));
    Object.defineProperty(out, "_pixels", { value: decoded.pixels, enumerable: false });
    Object.defineProperty(out, "_bytes", { value: r.body, enumerable: false });
    Object.defineProperty(out, "_etag", { value: r.headers.etag, enumerable: false });
  } else {
    out.body = r.body.toString("utf8").slice(0, 200);
  }
  return out;
}

// Pixel agreement of two decodes of the same dimensions.
function similarity(a, b) {
  if (!a || !b || a.width !== b.width || a.height !== b.height) return null;
  const { width: W, height: H } = a;
  let err = 0, aerr = 0;
  for (let i = 0; i < a.data.length; i += 4) {
    // Colour is compared where both are visible.
    const vis = Math.min(a.data[i + 3], b.data[i + 3]) / 255;
    for (let c = 0; c < 3; c++) err += Math.abs(a.data[i + c] - b.data[i + c]) * vis;
    aerr += Math.abs(a.data[i + 3] - b.data[i + 3]);
  }
  const n = W * H;
  // Luma of the visible colour (premultiplied by alpha): the colour under a
  // fully transparent pixel is arbitrary (encoders may rewrite it).
  const L = (d, i) => ((0.299 * d[i] + 0.587 * d[i + 1] + 0.114 * d[i + 2]) * d[i + 3]) / 255;
  const C1 = (0.01 * 255) ** 2, C2 = (0.03 * 255) ** 2;
  let ssim = 0, blocks = 0;
  const step = W * H > 1_000_000 ? 16 : 8;
  for (let y = 0; y + 8 <= H; y += step) for (let x = 0; x + 8 <= W; x += step) {
    let ma = 0, mb = 0;
    const va = [], vb = [];
    for (let j = 0; j < 8; j++) for (let k = 0; k < 8; k++) {
      const i = ((y + j) * W + x + k) * 4;
      const p = L(a.data, i), q = L(b.data, i);
      va.push(p); vb.push(q); ma += p; mb += q;
    }
    ma /= 64; mb /= 64;
    let sa = 0, sb = 0, cv = 0;
    for (let t = 0; t < 64; t++) { sa += (va[t] - ma) ** 2; sb += (vb[t] - mb) ** 2; cv += (va[t] - ma) * (vb[t] - mb); }
    sa /= 63; sb /= 63; cv /= 63;
    ssim += ((2 * ma * mb + C1) * (2 * cv + C2)) / ((ma * ma + mb * mb + C1) * (sa + sb + C2));
    blocks++;
  }
  return { mae: +(err / (n * 3)).toFixed(2), alphaMae: +(aerr / n).toFixed(2), ssim: blocks ? +(ssim / blocks).toFixed(4) : 1 };
}

// The source bytes behind a request's `url`, for the reference.
function sourceFor(path) {
  const url = new URL(path, "http://x").searchParams.get("url") ?? "";
  const local = url.startsWith("http") ? new URL(url).pathname : url;
  const remoteNames = {
    "/img/photo.jpg": "photo.jpg", "/img/alpha.png": "alpha.png", "/img/no-cache-headers.jpg": "odd.jpg",
    "/img/s-maxage.jpg": "odd.jpg", "/img/redirect.jpg": "photo.jpg", "/img/redirect-chain-2.jpg": "photo.jpg",
    "/img/wrong-type.png": "photo.jpg", "/img/gzip.jpg": "photo.jpg", "/img/brotli.jpg": "photo.jpg",
  };
  if (remoteNames[local]) return source(remoteNames[local]);
  let m = /^(?:\/base)?\/(?:photos|rewritten)\/([^/?]+)$/.exec(local);
  if (m) { try { return source(decodeURIComponent(m[1])); } catch { return null; } }
  m = /\/_next\/static\/media\/(still|wide)\.[\w-]+\.(jpg|png)$/.exec(local);
  if (m) return readFileSync(join(project, "app/assets", `${m[1]}.${m[2]}`));
  if (/\/api\/image\//.test(local)) {
    const data = readFileSync(join(project, "app/api/image/[name]/data.ts"), "utf8");
    return Buffer.from(/"([^"]+)"/.exec(data)[1], "base64");
  }
  return null;
}

// An unencoded Lanczos3 rendering of the fully decoded source at the
// output's size (flattened onto black when the output has no alpha, as
// libvips and NextRS flatten for JPEG).
async function reference(src, width, height, alpha) {
  const { data, info } = await sharp(src, { limitInputPixels: false }).rotate().ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  let img = sharp(data, { raw: { width: info.width, height: info.height, channels: 4 } })
    .resize(width, height, { fit: "fill", kernel: "lanczos3" });
  if (!alpha) img = img.flatten({ background: "#000000" });
  const out = await img.ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  return { data: out.data, width: out.info.width, height: out.info.height };
}

function quadrants(px) {
  const q = [];
  for (const [x0, y0] of [[0, 0], [0.5, 0], [0, 0.5], [0.5, 0.5]]) q.push(regionMean(px, x0, y0, x0 + 0.5, y0 + 0.5));
  return q;
}

// Mean colour of a region (fractions of the image), for orientation and
// colour-space checks: [r, g, b, a].
function regionMean(px, x0, y0, x1, y1) {
  const s = [0, 0, 0, 0];
  let n = 0;
  for (let y = Math.floor(y0 * px.height); y < Math.floor(y1 * px.height); y++)
    for (let x = Math.floor(x0 * px.width); x < Math.floor(x1 * px.width); x++) {
      const i = (y * px.width + x) * 4;
      // Visible colour: premultiplied by alpha.
      for (let c = 0; c < 3; c++) s[c] += (px.data[i + c] * px.data[i + 3]) / 255;
      s[3] += px.data[i + 3];
      n++;
    }
  return s.map((v) => Math.round(v / Math.max(n, 1)));
}

// --- outcome checks ----------------------------------------------------
const is = (want) => (o) => {
  for (const [k, v] of Object.entries(want)) {
    const got = k === "status" ? o.status : k === "body" ? o.body : k === "sameAsSource" ? o.sameAsSource : o.image?.[k] ?? o.headers?.[k];
    if (typeof v === "function" ? !v(got) : got !== v) return `${k}: ${JSON.stringify(got)}, want ${typeof v === "function" ? v.toString() : JSON.stringify(v)}`;
  }
  return null;
};
const refused = (status, body) => is({ status, body });
const optimized = (format, width, height, extra = {}) =>
  is({ status: 200, format, width, height, "content-type": `image/${format === "jpeg" ? "jpeg" : format === "heif" ? "avif" : format}`, ...extra });
const unchanged = (type, extra = {}) => is({ status: 200, "content-type": type, sameAsSource: true, ...extra });
const NOT_ALLOWED = '"url" parameter is not allowed';
const UPSTREAM_INVALID = '"url" parameter is valid but upstream response is invalid';
const INTERNAL_INVALID = '"url" parameter is valid but internal response is invalid';

// The hashed path of a static import, from the page's HTML.
async function staticMedia(base, name) {
  const r = await rawRequest(base, { path: prefix || "/" });
  const m = new RegExp(`(?:${prefix})?/_next/static/media/${name}\\.[\\w-]+\\.(?:jpg|png)`).exec(r.body.toString());
  return m?.[0];
}

// --- scenarios ---------------------------------------------------------
// kind: "compare" (default), "known-diff" (documented, candidate asserted
// with expectCandidate), "candidate" (NextRS only).
function scenarios(up) {
  const get = (path, accept = "webp", headers = {}) => ({ path, headers: { accept: ACCEPT[accept] ?? accept, ...headers } });
  const s = {};
  const add = (name, def) => (s[name] = def);

  // Parameter validation: next start's messages, in its order.
  const v = (name, path, status, body, headers) => add(`validate: ${name}`, { req: get(path, "webp", headers), expect: refused(status, body) });
  v("url required", `${prefix}/_next/image?w=640&q=75`, 400, '"url" parameter is required');
  v("url array", `${prefix}/_next/image?url=%2Fa.jpg&url=%2Fb.jpg&w=640&q=75`, 400, '"url" parameter cannot be an array');
  v("url too long", image("/photos/" + "a".repeat(3070)), 400, '"url" parameter is too long');
  v("protocol-relative", image("//evil.example/a.jpg"), 400, '"url" parameter cannot be a protocol-relative URL (//)');
  v("recursive", image(`/_next/image?url=%2Fphotos%2Fphoto.jpg&w=64&q=75`), 400, '"url" parameter cannot be recursive');
  v("recursive encoded", image(`/%5Fnext/image`), 400, '"url" parameter cannot be recursive');
  v("invalid absolute", image("http://[bad"), 400, '"url" parameter is invalid');
  v("non-http scheme", image("ftp://127.0.0.1/a.jpg"), 400, '"url" parameter is invalid');
  v("file scheme", image("file:///etc/passwd"), 400, '"url" parameter is invalid');
  v("width required", `${prefix}/_next/image?url=%2Fphotos%2Fphoto.jpg&q=75`, 400, '"w" parameter (width) is required');
  v("width array", `${prefix}/_next/image?url=%2Fphotos%2Fphoto.jpg&w=640&w=750&q=75`, 400, '"w" parameter (width) cannot be an array');
  v("width not a number", image("/photos/photo.jpg", { w: "abc" }), 400, '"w" parameter (width) must be an integer greater than 0');
  v("width negative", image("/photos/photo.jpg", { w: "-640" }), 400, '"w" parameter (width) must be an integer greater than 0');
  v("width zero", image("/photos/photo.jpg", { w: "0" }), 400, '"w" parameter (width) must be an integer greater than 0');
  v("width not allowed", image("/photos/photo.jpg", { w: 641 }), 400, '"w" parameter (width) of 641 is not allowed');
  v("width huge", image("/photos/photo.jpg", { w: "99999999" }), 400, '"w" parameter (width) of 99999999 is not allowed');
  v("quality required", `${prefix}/_next/image?url=%2Fphotos%2Fphoto.jpg&w=640`, 400, '"q" parameter (quality) is required');
  v("quality not a number", image("/photos/photo.jpg", { q: "high" }), 400, '"q" parameter (quality) must be an integer between 1 and 100');
  v("quality zero", image("/photos/photo.jpg", { q: "0" }), 400, '"q" parameter (quality) must be an integer between 1 and 100');
  v("quality above 100", image("/photos/photo.jpg", { q: "101" }), 400, '"q" parameter (quality) must be an integer between 1 and 100');
  v("quality not allowed", image("/photos/photo.jpg", { q: "80" }), 400, '"q" parameter (quality) of 80 is not allowed');
  add("validate: leading zeros accepted", { req: get(image("/photos/small.png", { w: "0032", q: "075" })), expect: optimized("webp", 32, 21) });
  add("validate: accept parameter without value", { req: get(image("/photos/photo.jpg", { w: 32 }), "image/webp;q") });

  // Local access control: patterns, traversal, files outside public/.
  if (!alt) {
    v("local outside patterns", image("/secret/photo.jpg"), 400, NOT_ALLOWED);
    v("local with query", image("/photos/photo.jpg?v=1"), 400, NOT_ALLOWED);
    v("traversal", image("/photos/../next.config.ts"), 400, NOT_ALLOWED);
    v("encoded traversal", image("/photos/%2e%2e/next.config.ts"), 400, NOT_ALLOWED);
    v("traversal to .next", image("/photos/../.next/BUILD_ID"), 400, NOT_ALLOWED);
  }
  const notFound = (name, url) => add(`access: ${name}`, { req: get(image(url)), expect: refused(404, INTERNAL_INVALID) });
  notFound("double-encoded traversal", "/photos/%252e%252e/next.config.ts");
  notFound("encoded slash traversal", "/photos/..%2fnext.config.ts");
  notFound("missing file", "/photos/missing.jpg");
  if (alt) {
    add("access: source without basePath", { req: get(image("/photos/photo.jpg", { bare: true })), expect: refused(404, INTERNAL_INVALID) });
    // Without localPatterns every path may be requested; only files the
    // server serves can be read.
    notFound("source file", "/next.config.ts");
    add("access: source file outside basePath", { req: get(image("/next.config.ts", { bare: true })), expect: refused(404, INTERNAL_INVALID) });
    notFound("build id", "/.next-alt/BUILD_ID");
    notFound("server bundle", "/.next-alt/server/app/page.js");
    notFound("node_modules", "/node_modules/next/package.json");
    notFound("package.json", "/package.json");
    add("access: page HTML is not an image", { req: get(image("/second")), expect: refused(400, "The requested resource isn't a valid image.") });
  }
  add("access: dot-segment normalization", { req: get(image("/photos/./photo.jpg")), expect: optimized("webp", 640, 480) });

  // Formats and geometry.
  const o = (name, url, opts, accept, expect, extra = {}) => add(`local: ${name}`, { req: get(image(url, opts), accept), expect, ...extra });
  o("jpeg to webp", "/photos/photo.jpg", { w: 640 }, "webp", optimized("webp", 640, 480, { "x-nextjs-cache": "MISS" }));
  o("jpeg to avif", "/photos/photo.jpg", { w: 640 }, "avif", optimized("heif", 640, 480));
  o("jpeg kept for */*", "/photos/photo.jpg", { w: 640 }, "any", optimized("jpeg", 640, 480));
  o("chrome accept", "/photos/photo.jpg", { w: 384 }, "chrome", optimized("heif", 384, 288));
  o("no enlargement", "/photos/photo.jpg", { w: 3840 }, "webp", optimized("webp", 1600, 1200));
  o("aspect rounding", "/photos/odd.jpg", { w: 640 }, "webp", optimized("webp", 640, 426));
  o("aspect rounding 2", "/photos/odd.jpg", { w: 96 }, "webp", optimized("webp", 96, 64));
  o("small png kept size", "/photos/small.png", { w: 640 }, "any", optimized("png", 48, 32));
  o("quality 50", "/photos/photo.jpg", { w: 750, q: 50 }, "webp", optimized("webp", 750, 563));
  o("quality 90", "/photos/photo.jpg", { w: 750, q: 90 }, "webp", optimized("webp", 750, 563));
  o("alpha png to webp", "/photos/alpha.png", { w: 384 }, "webp", (x) =>
    optimized("webp", 384, 288, { hasAlpha: true })(x) ??
    (regionMean(x._pixels, 0, 0, 0.05, 0.05)[3] > 10 ? "corner not transparent" : null) ??
    (regionMean(x._pixels, 0.45, 0.45, 0.55, 0.55)[3] < 245 ? "centre not opaque" : null));
  o("alpha png kept png", "/photos/alpha.png", { w: 256 }, "any", optimized("png", 256, 192, { hasAlpha: true }));
  o("webp to jpeg for */*", "/photos/photo.webp", { w: 640 }, "any", optimized("jpeg", 640, 427));
  o("alpha webp to jpeg", "/photos/alpha.webp", { w: 256 }, "any", optimized("jpeg", 256, 192));
  o("webp to avif", "/photos/photo.webp", { w: 640 }, "avif", optimized("heif", 640, 427));
  o("static gif to webp", "/photos/still.gif", { w: 256 }, "webp", optimized("webp", 256, 171));
  add("local: static gif kept gif", {
    req: get(image("/photos/still.gif", { w: 256 }), "any"),
    kind: "known-diff",
    reason: "sharp's GIF writer declares a transparency index on an opaque image (decodes with an alpha channel); NextRS writes none; the pixels are opaque in both",
    expect: optimized("gif", 256, 171),
    expectCandidate: optimized("gif", 256, 171, { hasAlpha: false }),
  });
  o("lzw tiff to webp", "/photos/lzw.tiff", { w: 384 }, "webp", optimized("webp", 384, 288));
  add("local: jpeg-compressed tiff", {
    req: get(image("/photos/photo.tiff", { w: 384 })),
    source: "photo.tiff",
    kind: "known-diff",
    reason: "the tiff decoder has no YCbCr (JPEG-in-TIFF): the source is served unchanged, next start converts it",
    expect: optimized("webp", 384, 288),
    expectCandidate: unchanged("image/tiff", { "cache-control": `public, max-age=${MIN_TTL}, must-revalidate` }),
  });
  o("exif orientation", "/photos/rotated.jpg", { w: 256 }, "webp", (x) =>
    optimized("webp", 256, 384)(x) ??
    // Stored with red top-left; rotated 90° clockwise, red is top-right.
    (regionMean(x._pixels, 0.6, 0.05, 0.95, 0.4)[0] < 150 ? `top-right not red: ${regionMean(x._pixels, 0.6, 0.05, 0.95, 0.4)}` : null));
  o("display p3 to srgb", "/photos/p3.jpg", { w: 384 }, "webp", optimized("webp", 384, 288));
  o("large photo", "/photos/large.jpg", { w: 1080 }, "webp", optimized("webp", 1080, 720));
  o("animated gif unchanged", "/photos/animated.gif", { w: 64 }, "webp", unchanged("image/gif"), { source: "animated.gif" });
  o("animated webp unchanged", "/photos/animated.webp", { w: 64 }, "webp", unchanged("image/webp"), { source: "animated.webp" });
  o("corrupt served unchanged", "/photos/corrupt.jpg", { w: 64 }, "webp", unchanged("image/jpeg", { "cache-control": `public, max-age=${MIN_TTL}, must-revalidate` }), { source: "corrupt.jpg" });
  o("truncated served unchanged", "/photos/truncated.png", { w: 64 }, "webp", unchanged("image/png"), { source: "truncated.png" });
  o("bomb served unchanged", "/photos/bomb.png", { w: 640 }, "webp", unchanged("image/png"), { source: "bomb.png" });
  o("not an image", "/photos/not-an-image.png", { w: 64 }, "webp", refused(400, "The requested resource isn't a valid image."));
  o("pdf refused", "/photos/document.pdf", { w: 64 }, "webp", refused(400, "The requested resource isn't a valid image."));
  if (alt) o("svg allowed unchanged", "/photos/icon.svg", { w: 64 }, "webp", unchanged("image/svg+xml", { "content-disposition": 'inline; filename="icon.svg"' }), { source: "icon.svg" });
  else o("svg refused", "/photos/icon.svg", { w: 64 }, "webp", refused(400, '"url" parameter is valid but image type is not allowed'));
  add("local: avif source", {
    req: get(image("/photos/photo.avif", { w: 384 })),
    source: "photo.avif",
    kind: "known-diff",
    reason: "NextRS has no AVIF decoder: the source is served unchanged (next start's fallback when sharp fails), next start converts it",
    expect: optimized("webp", 384, 288),
    expectCandidate: unchanged("image/avif", { "cache-control": `public, max-age=${MIN_TTL}, must-revalidate` }),
  });
  add("local: dynamic route source", { req: get(image("/api/image/dyn.png", { w: 256 })), expect: optimized("webp", 256, 192) });
  if (!alt) {
    add("local: rewritten source", { req: get(image("/rewritten/photo.jpg", { w: 256 })), expect: optimized("webp", 256, 192) });
    add("local: source rewritten to the endpoint", { req: get(image("/rewritten-loop/photo.jpg", { w: 256 })) });
  }
  add("local: static import", {
    async run(base) {
      const media = await staticMedia(base, "still");
      if (!media) return { error: "no static media path in the page" };
      return observe(base, get(image(media, { w: 640 })));
    },
    expect: optimized("webp", 640, 480, { "cache-control": "public, max-age=315360000, immutable" }),
  });
  add("local: head request", {
    async run(base) {
      const r = await rawRequest(base, { method: "HEAD", path: image("/photos/odd.jpg", { w: 128 }), headers: { accept: ACCEPT.webp } });
      return { status: r.status, type: r.headers["content-type"], length: r.headers["content-length"] ? "present" : "absent", body: r.body?.length ?? 0 };
    },
    expect: (o) => (o.status === 200 && o.body === 0 && o.type === "image/webp" ? null : JSON.stringify(o)),
  });

  // Conditional requests and caching.
  add("cache: hit returns the same image", {
    async run(base) {
      const path = image("/photos/odd.jpg", { w: 384 });
      const first = await observe(base, get(path));
      const second = await observe(base, get(path));
      return { first: first.headers["x-nextjs-cache"], second: second.headers["x-nextjs-cache"], same: Boolean(first._bytes?.equals(second._bytes ?? Buffer.alloc(0))), image: second.image };
    },
    expect: (o) => (o.first === "MISS" && o.second === "HIT" && o.same && o.image?.width === 384 ? null : JSON.stringify(o)),
  });
  add("cache: widths do not share entries", {
    async run(base) {
      const a = await observe(base, get(image("/photos/photo.jpg", { w: 828 })));
      const b = await observe(base, get(image("/photos/photo.jpg", { w: 1080 })));
      const c = await observe(base, get(image("/photos/photo.jpg", { w: 828 })));
      return { a: [a.image?.width, a.image?.height], b: [b.image?.width, b.image?.height], c: [c.image?.width, c.headers["x-nextjs-cache"]] };
    },
    expect: (o) => (o.a[0] === 828 && o.b[0] === 1080 && o.c[0] === 828 && o.c[1] === "HIT" ? null : JSON.stringify(o)),
  });
  add("cache: formats do not share entries", {
    async run(base) {
      const a = await observe(base, get(image("/photos/photo.jpg", { w: 1200 }), "webp"));
      const b = await observe(base, get(image("/photos/photo.jpg", { w: 1200 }), "any"));
      const c = await observe(base, get(image("/photos/photo.jpg", { w: 1200 }), "avif"));
      return [a.image?.format, b.image?.format, c.image?.format, a.headers.vary];
    },
    expect: (o) => (JSON.stringify(o) === JSON.stringify(["webp", "jpeg", "heif", "Accept"]) ? null : JSON.stringify(o)),
  });
  add("cache: conditional request", {
    async run(base) {
      const path = image("/photos/odd.jpg", { w: 256 });
      const first = await observe(base, get(path));
      const etag = first._etag;
      const r304 = await rawRequest(base, { path, headers: { accept: ACCEPT.webp, "if-none-match": etag } });
      const weak = await rawRequest(base, { path, headers: { accept: ACCEPT.webp, "if-none-match": `W/${etag}` } });
      const star = await rawRequest(base, { path, headers: { accept: ACCEPT.webp, "if-none-match": "*" } });
      const other = await rawRequest(base, { path, headers: { accept: ACCEPT.webp, "if-none-match": "nope" } });
      const nocache = await rawRequest(base, { path, headers: { accept: ACCEPT.webp, "if-none-match": etag, "cache-control": "no-cache" } });
      const pick = (r) => [r.status, r.body.length > 0, r.headers.etag === etag, r.headers["cache-control"], r.headers.vary, r.headers["content-type"] ?? null];
      return { r304: pick(r304), weak: pick(weak), star: pick(star), other: pick(other)[0], nocache: pick(nocache)[0] };
    },
    expect: (o) => (o.r304[0] === 304 && !o.r304[1] && o.r304[2] && o.weak[0] === 304 && o.star[0] === 304 && o.other === 200 && o.nocache === 200 ? null : JSON.stringify(o)),
  });
  add("cache: stale entry is served, then revalidated", {
    async run(base) {
      const path = image("/photos/photo.webp", { w: 256 });
      const first = await observe(base, get(path));
      await sleep((MIN_TTL + 1) * 1000);
      const stale = await observe(base, get(path));
      await sleep(1500);
      const after = await observe(base, get(path));
      return [first.headers["x-nextjs-cache"], stale.headers["x-nextjs-cache"], after.headers["x-nextjs-cache"], after.image?.width];
    },
    expect: (o) => (o[0] === "MISS" && o[1] === "STALE" && o[2] === "HIT" && o[3] === 256 ? null : JSON.stringify(o)),
  });

  // Remote sources (the local upstream).
  const r = (name, path, opts, expect, extra = {}) => add(`remote: ${name}`, { req: get(image(remote(path), opts)), expect, ...extra });
  r("jpeg to webp", "/img/photo.jpg", { w: 640 }, optimized("webp", 640, 480, { "cache-control": "public, max-age=60, must-revalidate" }));
  r("alpha png", "/img/alpha.png", { w: 384 }, optimized("webp", 384, 288, { hasAlpha: true }));
  r("no cache headers", "/img/no-cache-headers.jpg", { w: 256 }, optimized("webp", 256, 170, { "cache-control": `public, max-age=${MIN_TTL}, must-revalidate` }));
  r("s-maxage wins", "/img/s-maxage.jpg", { w: 256 }, optimized("webp", 256, 170, { "cache-control": "public, max-age=30, must-revalidate" }));
  r("redirect followed", "/img/redirect.jpg", { w: 384 }, optimized("webp", 384, 288));
  r("three redirects", "/img/redirect-chain-2.jpg", { w: 384 }, optimized("webp", 384, 288));
  r("four redirects", "/img/redirect-chain-3.jpg", { w: 384 }, refused(508, UPSTREAM_INVALID));
  r("redirect loop", "/img/redirect-loop.jpg", { w: 384 }, refused(508, UPSTREAM_INVALID));
  r("missing", "/img/missing.jpg", { w: 384 }, refused(404, UPSTREAM_INVALID));
  r("upstream 503", "/img/server-error.jpg", { w: 384 }, refused(503, UPSTREAM_INVALID));
  r("text body", "/img/text.jpg", { w: 384 }, refused(400, "The requested resource isn't a valid image."));
  r("html body", "/img/html.jpg", { w: 384 }, refused(400, "The requested resource isn't a valid image."));
  r("empty body", "/img/empty.jpg", { w: 384 }, refused(400, UPSTREAM_INVALID));
  r("corrupt served unchanged", "/img/corrupt.jpg", { w: 384 }, unchanged("image/jpeg"), { source: "corrupt.jpg" });
  r("content type is sniffed", "/img/wrong-type.png", { w: 384 }, optimized("webp", 384, 288));
  r("oversized body", "/img/oversized.jpg", { w: 384 }, refused(413, UPSTREAM_INVALID));
  r("gzip body", "/img/gzip.jpg", { w: 384 }, optimized("webp", 384, 288));
  r("brotli body", "/img/brotli.jpg", { w: 384 }, optimized("webp", 384, 288));
  r("animated unchanged", "/img/animated.gif", { w: 64 }, unchanged("image/gif"), { source: "animated.gif" });
  if (!alt) r("svg refused", "/img/icon.svg", { w: 64 }, refused(400, '"url" parameter is valid but image type is not allowed'));
  r("connection reset", "/img/reset.jpg", { w: 384 }, (x) => (x.status >= 500 ? null : `status ${x.status}`));
  if (!args.quick) {
    r("slow head times out", "/img/slow-head.jpg", { w: 384 }, refused(504, '"url" parameter is valid but upstream response timed out'));
    r("slow body times out", "/img/slow-body.jpg", { w: 384 }, refused(504, '"url" parameter is valid but upstream response timed out'));
  }
  // Forbidden destinations: never fetched by NextRS.
  const forbidden = (name, url, opts = {}) =>
    add(`remote: ${name}`, {
      async run(base) {
        const before = up.count("/forbidden/secret.jpg") + up.count("/other/secret.jpg");
        const o = await observe(base, get(image(url, { w: 384 })));
        o.fetched = up.count("/forbidden/secret.jpg") + up.count("/other/secret.jpg") - before;
        return o;
      },
      ...opts,
    });
  forbidden("path outside remotePatterns", remote("/other/secret.jpg"), { expect: (o) => refused(400, NOT_ALLOWED)(o) ?? (o.fetched ? "fetched" : null) });
  forbidden("port outside remotePatterns", `http://127.0.0.1:${upstreamPort + 1}/img/photo.jpg`, { expect: refused(400, NOT_ALLOWED) });
  forbidden("host outside remotePatterns", "http://example.com/img/photo.jpg", { expect: refused(400, NOT_ALLOWED) });
  forbidden("https outside remotePatterns", `https://127.0.0.1:${upstreamPort}/img/photo.jpg`, { expect: refused(400, NOT_ALLOWED) });
  forbidden("credentials in url", `http://user:pw@127.0.0.1:${upstreamPort}/img/photo.jpg`, {
    kind: "known-diff",
    reason: "next start fetches URLs with credentials; NextRS's network policy refuses them",
    expectCandidate: (o) => refused(400, NOT_ALLOWED)(o) ?? (o.fetched ? "fetched" : null),
  });
  forbidden("redirect to another address", remote("/img/redirect-other-addr.jpg"), {
    kind: "known-diff",
    reason: "next start (dangerouslyAllowLocalIP) follows a redirect to 127.0.0.2; NextRS's --net-allow-addr covers 127.0.0.1 only",
    expectCandidate: (o) => refused(400, NOT_ALLOWED)(o) ?? (o.fetched ? "fetched" : null),
  });
  forbidden("redirect to another host", remote("/img/redirect-other-host.jpg"), {
    kind: "known-diff",
    reason: "next start follows a redirect to localhost; --net-allow allows only http://127.0.0.1:<port>",
    expectCandidate: (o) => refused(400, NOT_ALLOWED)(o) ?? (o.fetched ? "fetched" : null),
  });
  add("remote: redirect to cloud metadata", {
    kind: "candidate",
    async run(base) {
      const o = await observe(base, get(image(remote("/img/redirect-metadata.jpg"), { w: 384 })));
      const err = refused(400, NOT_ALLOWED)(o);
      return { ok: !err, detail: err ?? o };
    },
  });
  add("remote: one fetch for concurrent requests", {
    async run(base, side) {
      const path = image(remote(`/img/photo.jpg?c=${side}`), { w: 1920 });
      const before = up.count("/img/photo.jpg");
      const results = await Promise.all(Array.from({ length: 100 }, () => rawRequest(base, { path, headers: { accept: ACCEPT.webp } })));
      const bodies = new Set(results.map((x) => (x.body ? sha(x.body) : x.error)));
      return { statuses: [...new Set(results.map((x) => x.status))], distinctBodies: bodies.size, fetches: up.count("/img/photo.jpg") - before };
    },
    expect: (o) => (o.statuses.length === 1 && o.statuses[0] === 200 && o.distinctBodies === 1 && o.fetches === 1 ? null : JSON.stringify(o)),
  });
  add("remote: changed source after revalidation", {
    async run(base, side) {
      const path = image(remote(`/img/versioned.jpg?s=${side}`), { w: 256 });
      await rawRequest(`http://127.0.0.1:${upstreamPort}`, { path: "/control/reset" });
      const a = await observe(base, get(path));
      await rawRequest(`http://127.0.0.1:${upstreamPort}`, { path: "/control/bump" });
      await sleep((MIN_TTL + 1) * 1000);
      const stale = await observe(base, get(path));
      await sleep(1500);
      const b = await observe(base, get(path));
      return { first: a.image?.height, stale: stale.headers["x-nextjs-cache"], after: [b.image?.height, b.headers["x-nextjs-cache"]] };
    },
    // photo.jpg is 4:3 (256x192), odd.jpg 3:2 (256x170).
    expect: (o) => (o.first === 192 && o.stale === "STALE" && o.after[0] === 170 ? null : JSON.stringify(o)),
  });
  return s;
}

// --- runner ------------------------------------------------------------
async function main() {
  const up = await startUpstream(photos, upstreamPort);
  const only = args.only ? new Set(args.only.split(",")) : null;
  const all = scenarios(up);
  const results = [];
  let failures = 0;
  for (const [name, sc] of Object.entries(all)) {
    if (only && ![...only].some((o) => name.includes(o))) continue;
    const run = sc.run ?? ((base) => observe(base, sc.req, { source: sc.source }));
    let verdict, detail;
    try {
      if (sc.kind === "candidate") {
        const r = await run(args.candidate, "candidate");
        verdict = r.ok ? "PASS" : "FAIL";
        detail = r.detail;
      } else {
        const a = await run(args.baseline, "baseline");
        const b = await run(args.candidate, "candidate");
        const diffs = differences(a, b);
        const sim = similarity(a._pixels, b._pixels);
        let fidelity = null;
        if (sim && !(a.sameAsSource && b.sameAsSource)) {
          sim.direct = sim.mae <= TOLERANCE.mae && sim.alphaMae <= TOLERANCE.alphaMae && sim.ssim >= TOLERANCE.ssim;
          const qa = quadrants(a._pixels), qb = quadrants(b._pixels);
          const delta = Math.max(...qa.flatMap((m, i) => m.map((v, c) => Math.abs(v - qb[i][c]))));
          if (delta > QUADRANTS.maxDelta) diffs.push(`quadrant colours differ by ${delta}: ${JSON.stringify(qa)} | ${JSON.stringify(qb)}`);
          const src = sc.req ? sourceFor(sc.req.path) : sc.refSource?.();
          if (src) {
            const ref = await reference(src, a._pixels.width, a._pixels.height, Boolean(a.image?.hasAlpha));
            const fa = similarity(ref, a._pixels), fb = similarity(ref, b._pixels);
            const colour = (f) => ({ ssim: f.ssim, mae: f.mae });
            fidelity = { nextStart: colour(fa), nextrs: colour(fb) };
            const ssimOk = fb.ssim >= fa.ssim - FIDELITY.ssimMargin || fb.ssim >= FIDELITY.ssimFloor;
            const maeOk = fb.mae <= fa.mae + FIDELITY.maeMargin || fb.mae <= FIDELITY.maeFloor;
            if (!ssimOk || !maeOk) diffs.push(`fidelity: ${JSON.stringify(fidelity)} (margins ${JSON.stringify(FIDELITY)})`);
          } else if (!sim.direct) {
            diffs.push(`pixels: ${JSON.stringify(sim)} outside ${JSON.stringify(TOLERANCE)} (no reference)`);
          }
        }
        const oracleWrong = sc.kind !== "known-diff" && sc.expect ? sc.expect(a) : null;
        const check = sc.expectCandidate ?? sc.expect;
        const candidateWrong = check ? check(b) : null;
        if (sc.kind === "known-diff") verdict = candidateWrong ? "FAIL" : diffs.length ? "KNOWN-DIFF" : "MATCH";
        else verdict = oracleWrong ? "ORACLE-FAIL" : candidateWrong ? "FAIL" : diffs.length ? "DIFF" : "MATCH";
        detail = { diffs, similarity: sim, fidelity, oracleWrong, candidateWrong, asserted: Boolean(check), baseline: a, candidate: b, reason: sc.reason };
      }
    } catch (e) {
      verdict = "ERROR";
      detail = String(e?.stack ?? e);
    }
    if (["DIFF", "FAIL", "ERROR", "ORACLE-FAIL"].includes(verdict)) failures++;
    results.push({ name, verdict, detail });
    const f = detail?.fidelity;
    const sim = detail?.similarity
      ? ` [direct mae ${detail.similarity.mae} ssim ${detail.similarity.ssim}${f ? `; vs reference next ${f.nextStart.ssim} nextrs ${f.nextrs.ssim}` : ""}]`
      : "";
    console.log(`${verdict.padEnd(10)} ${name}${sim}`);
    if (verdict === "DIFF") for (const d of detail.diffs) console.log(`    ${d}`);
    if (verdict === "ORACLE-FAIL") console.log(`    next start: ${detail.oracleWrong}`);
    if (verdict === "FAIL") console.log(`    NextRS: ${detail?.candidateWrong ?? JSON.stringify(detail).slice(0, 600)}`);
    if (verdict === "ERROR") console.log(`    ${detail.slice(0, 800)}`);
    if (verdict === "KNOWN-DIFF") console.log(`    (${sc.reason})`);
  }
  await up.close();
  const count = (v) => results.filter((r) => r.verdict === v).length;
  const summary = {
    title: `image (${variant})`,
    total: results.length,
    match: count("MATCH"),
    asserted: results.filter((r) => r.verdict === "MATCH" && r.detail?.asserted).length,
    pass: count("PASS"),
    knownDiff: count("KNOWN-DIFF"),
    diff: count("DIFF"),
    fail: count("FAIL"),
    oracleFail: count("ORACLE-FAIL"),
    error: count("ERROR"),
    negativeControl: process.env.NEXTRS_IMAGE_NEGATIVE_CONTROL || null,
    tolerance: TOLERANCE,
    fidelity: FIDELITY,
    quadrants: QUADRANTS,
    directWithinTolerance: results.filter((r) => r.detail?.similarity?.direct === true).length,
    directOutsideTolerance: results.filter((r) => r.detail?.similarity?.direct === false).length,
  };
  console.log(
    `${summary.title}: ${summary.match} MATCH (${summary.asserted} asserted), ${summary.pass} PASS, ${summary.knownDiff} KNOWN-DIFF, ${summary.diff} DIFF, ${summary.fail} FAIL, ${summary.oracleFail} ORACLE-FAIL, ${summary.error} ERROR`,
  );
  if (args.json) {
    const strip = (r) => ({ ...r, detail: typeof r.detail === "object" ? JSON.parse(JSON.stringify(r.detail)) : r.detail });
    writeFileSync(args.json, JSON.stringify({ summary, results: results.map(strip) }, null, 2));
  }
  process.exit(failures ? 1 : 0);
}

main();
