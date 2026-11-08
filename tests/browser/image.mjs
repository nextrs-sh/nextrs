// next/image in Chromium on fixtures/next-image, against next start (the
// baseline) and NextRS (the candidate): what a visitor gets. Every image
// element must load (or, on /broken, fail) the same way on both servers,
// with the same selected source; decoded pixels are sampled from a canvas
// (orientation, colours); lazy images load only when scrolled to; preload
// links, blur placeholders, responsive selection, client navigation,
// hydration errors and the browser's revalidation (304) are checked.
//
//   node tests/browser/image.mjs --baseline URL --candidate URL --photos DIR
//     [--base-path /base] [--upstream-port 4317] [--json out.json]
import { writeFileSync } from "node:fs";
import { chromium } from "playwright-core";
import { startUpstream } from "../compat/image-upstream.mjs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const basePath = args["base-path"] ?? "";
const TIMEOUT = 20_000;

// /_next/image requests a page made: their parameters and answers.
function trackImages(page) {
  const seen = [];
  page.on("response", (response) => {
    const url = new URL(response.url());
    if (!url.pathname.endsWith("/_next/image")) return;
    const p = url.searchParams;
    seen.push({
      url: p.get("url")?.replace(/^http:\/\/127\.0\.0\.1:\d+/, "UPSTREAM").replace(/[?&]n=[^&]*/, ""),
      w: Number(p.get("w")),
      q: Number(p.get("q")),
      status: response.status(),
      type: response.headers()["content-type"] ?? null,
    });
  });
  return seen;
}

const sort = (list) => [...list].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));

// Every <img>: as rendered, as loaded, and (when loaded) quadrant colours
// read back through a canvas.
async function images(page) {
  return page.evaluate(() =>
    [...document.querySelectorAll("img")].map((img) => {
      const out = {
        id: img.id || img.alt,
        complete: img.complete,
        natural: [img.naturalWidth, img.naturalHeight],
        current: img.currentSrc.replace(location.origin, "").replace(/http:\/\/127\.0\.0\.1:\d+/, "UPSTREAM").replace(/%3Fn%3D[^&]*/, ""),
        loading: img.getAttribute("loading"),
        fetchpriority: img.getAttribute("fetchpriority"),
        sizes: img.getAttribute("sizes"),
        srcset: (img.getAttribute("srcset") ?? "").replace(/http%3A%2F%2F127\.0\.0\.1%3A\d+/g, "UPSTREAM"),
        placeholder: /data:image\/svg\+xml/.test(img.style.backgroundImage) ? "blur" : null,
      };
      if (img.complete && img.naturalWidth > 0) {
        const c = document.createElement("canvas");
        c.width = img.naturalWidth;
        c.height = img.naturalHeight;
        const ctx = c.getContext("2d");
        ctx.drawImage(img, 0, 0);
        const mean = (x0, y0) => {
          const d = ctx.getImageData(Math.floor(x0 * c.width), Math.floor(y0 * c.height), Math.max(1, Math.floor(c.width / 4)), Math.max(1, Math.floor(c.height / 4))).data;
          const s = [0, 0, 0, 0];
          for (let i = 0; i < d.length; i += 4) for (let k = 0; k < 4; k++) s[k] += d[i + k];
          return s.map((v) => Math.round(v / (d.length / 4)));
        };
        out.quadrants = [mean(0.125, 0.125), mean(0.625, 0.125), mean(0.125, 0.625), mean(0.625, 0.625)];
      }
      return out;
    }),
  );
}

async function scenarioHome(browser, base, viewport) {
  const context = await browser.newContext({ viewport, deviceScaleFactor: 1 });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e.message)));
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  const requests = trackImages(page);
  // The blur placeholder is in the server-rendered HTML; next/image removes
  // it once the image has loaded.
  const html = await (await context.request.get(`${base}${basePath}/`)).text();
  const blurInHtml = /<img[^>]*id="static"[^>]*background-image:url\(&quot;data:image\/svg\+xml/.test(html) ||
    /<img[^>]*background-image:url\(&quot;data:image\/svg\+xml[^>]*id="static"/.test(html);
  await page.goto(`${base}${basePath}/`, { waitUntil: "load", timeout: TIMEOUT });
  await page.waitForTimeout(500);
  const before = await images(page);
  const lazyRequestedEarly = requests.some((r) => r.url?.includes("rotated.jpg"));
  // Scroll through the page so every lazy image is requested.
  await page.evaluate(async () => {
    for (let y = 0; y < document.body.scrollHeight; y += 400) {
      window.scrollTo(0, y);
      await new Promise((r) => setTimeout(r, 60));
    }
    window.scrollTo(0, document.body.scrollHeight);
  });
  await page.waitForFunction(() => [...document.querySelectorAll("img")].every((i) => i.complete && i.naturalWidth > 0), null, { timeout: TIMEOUT });
  const after = await images(page);
  const preload = await page.evaluate(() =>
    [...document.querySelectorAll('link[rel="preload"][as="image"]')].map((l) => (l.getAttribute("imagesrcset") ?? l.href).replace(location.origin, "")),
  );
  await context.close();
  return { viewport: viewport.width, before, after, blurInHtml, lazyRequestedEarly, preload, requests: sort(requests), errors };
}

async function scenarioNavigation(browser, base) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e.message)));
  await page.goto(`${base}${basePath}/`, { waitUntil: "load", timeout: TIMEOUT });
  await page.click("#to-second");
  await page.waitForSelector("#second-title", { timeout: TIMEOUT });
  await page.waitForFunction(() => document.querySelector("#second-local")?.complete, null, { timeout: TIMEOUT });
  const second = (await images(page)).find((i) => i.id === "second-local");
  await page.click("#to-home");
  await page.waitForSelector("#local", { timeout: TIMEOUT });
  await page.waitForFunction(() => document.querySelector("#local")?.complete, null, { timeout: TIMEOUT });
  const home = (await images(page)).find((i) => i.id === "local");
  await context.close();
  return { second: { natural: second?.natural, current: second?.current }, home: { natural: home?.natural, complete: home?.complete }, errors };
}

async function scenarioBroken(browser, base) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const requests = trackImages(page);
  await page.goto(`${base}${basePath}/broken`, { waitUntil: "load", timeout: TIMEOUT });
  await page.waitForTimeout(500);
  const list = (await images(page)).map((i) => ({ id: i.id, complete: i.complete, natural: i.natural }));
  await context.close();
  return { images: list, requests: sort(requests).map((r) => ({ url: r.url, status: r.status })) };
}

// The browser's own cache: after max-age (2 s) a reload revalidates with
// If-None-Match and gets 304s.
async function scenarioRevalidation(browser, base) {
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.goto(`${base}${basePath}/second`, { waitUntil: "load", timeout: TIMEOUT });
  await page.waitForTimeout(3000);
  const statuses = [];
  page.on("response", (r) => r.url().includes("/_next/image") && statuses.push(r.status()));
  await page.reload({ waitUntil: "load" });
  await page.waitForTimeout(500);
  await context.close();
  return { statuses: [...new Set(statuses)].sort() };
}

function check(name, o) {
  if (name.startsWith("home")) {
    const bad = o.after.filter((i) => !i.complete || i.natural[0] === 0);
    if (bad.length) return `not loaded: ${bad.map((i) => i.id)}`;
    if (o.requests.some((r) => r.status !== 200)) return `failed image requests: ${JSON.stringify(o.requests.filter((r) => r.status !== 200))}`;
    if (o.lazyRequestedEarly) return "lazy image requested before it was scrolled to";
    if (!o.preload.length) return "no preload link";
    if (o.errors.length) return `page errors: ${o.errors}`;
    const local = o.after.find((i) => i.id === "local");
    if (local.quadrants[0][0] < 150 || local.quadrants[3][1] < 180) return `colours: ${JSON.stringify(local.quadrants)}`;
    const lazy = o.after.find((i) => i.id === "lazy");
    if (lazy.natural[0] >= lazy.natural[1] || lazy.quadrants[1][0] < 150) return `orientation: ${JSON.stringify(lazy)}`;
    if (!o.blurInHtml) return "no blur placeholder in the HTML";
  }
  if (name === "navigation" && (o.errors.length || !o.home.complete || !o.second.natural?.[0])) return JSON.stringify(o);
  if (name === "broken") {
    const by = Object.fromEntries(o.images.map((i) => [i.id, i.natural[0]]));
    // next/image renders an .svg source unoptimized itself (no /_next/image
    // request) unless dangerouslyAllowSVG.
    if (by.missing !== 0 || by.invalid !== 0) return "a refused source rendered";
    if (!by["svg-optimized"]) return "the svg (served unoptimized) did not render";
    // dangerouslyAllowSVG (the basePath build) sends it through the optimizer.
    const optimizedSvg = o.requests.some((r) => r.url?.endsWith(".svg"));
    if (optimizedSvg !== Boolean(basePath)) return `svg through the optimizer: ${optimizedSvg}`;
  }
  if (name === "revalidation" && JSON.stringify(o.statuses) !== "[304]") return `statuses ${o.statuses}`;
  return null;
}

// Rounds canvas colours: the two servers' encoders differ slightly.
// `before` (what had loaded 500 ms after `load`) is timing, not behaviour:
// it is kept for the record and asserted only through lazyRequestedEarly.
function comparable(o) {
  return JSON.parse(
    JSON.stringify(o, (k, v) =>
      k === "before" ? undefined : k === "quadrants" ? v.map((q) => q.map((c) => Math.round(c / 32))) : k === "errors" ? v.length : v,
    ),
  );
}

const up = await startUpstream(args.photos, Number(args["upstream-port"] ?? 4317));
const browser = await chromium.launch();
const scenarios = {
  "home (1280px)": (b) => scenarioHome(browser, b, { width: 1280, height: 900 }),
  "home (375px)": (b) => scenarioHome(browser, b, { width: 375, height: 800 }),
  navigation: (b) => scenarioNavigation(browser, b),
  broken: (b) => scenarioBroken(browser, b),
  revalidation: (b) => scenarioRevalidation(browser, b),
};
const results = [];
let failures = 0;
for (const [name, run] of Object.entries(scenarios)) {
  let verdict, detail;
  try {
    const a = await run(args.baseline);
    const b = await run(args.candidate);
    const oracle = check(name, a), wrong = check(name, b);
    const same = JSON.stringify(comparable(a)) === JSON.stringify(comparable(b));
    verdict = oracle ? "ORACLE-FAIL" : wrong ? "FAIL" : same ? "MATCH" : "DIFF";
    detail = { oracle, wrong, baseline: a, candidate: b };
    if (verdict === "DIFF") {
      const ca = comparable(a), cb = comparable(b);
      detail.diff = Object.keys(ca).filter((k) => JSON.stringify(ca[k]) !== JSON.stringify(cb[k]));
    }
  } catch (e) {
    verdict = "ERROR";
    detail = String(e?.stack ?? e);
  }
  if (verdict !== "MATCH") failures++;
  results.push({ name, verdict, detail });
  console.log(`${verdict.padEnd(11)} ${name}`);
  if (verdict === "FAIL") console.log(`    NextRS: ${detail.wrong}`);
  if (verdict === "ORACLE-FAIL") console.log(`    next start: ${detail.oracle}`);
  if (verdict === "DIFF") console.log(`    differs in: ${detail.diff}`);
  if (verdict === "ERROR") console.log(`    ${detail.slice(0, 600)}`);
}
await browser.close();
await up.close();
const summary = { title: `browser image${basePath ? ` (${basePath})` : ""}`, total: results.length, match: results.filter((r) => r.verdict === "MATCH").length, failures };
console.log(`${summary.title}: ${summary.match}/${summary.total} MATCH`);
if (args.json) writeFileSync(args.json, JSON.stringify({ summary, results }, null, 2));
process.exit(failures ? 1 : 0);
