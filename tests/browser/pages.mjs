// Browser-driven Pages Router scenarios on fixtures/next-pages, against
// next start (the baseline) and NextRS (the candidate): hydration, the
// custom App's global CSS, client navigation through /_next/data,
// back/forward, reload, redirects and notFound during client navigation,
// fallback pages, Preview Mode and a proxy-protected page reached by the
// client router. Each scenario returns what a user observes (URL, visible
// text, the data requests the page made, script errors); the two servers
// must agree, and each scenario also asserts its own outcome.
//
//   node tests/browser/pages.mjs --baseline URL --candidate URL [--base-path /base] [--json out.json]
//
// Needs playwright-core (npm ci in tests/browser) and its Chromium build.
import { writeFileSync } from "node:fs";
import { chromium } from "playwright-core";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const targets = { baseline: args.baseline, candidate: args.candidate };
if (!targets.baseline || !targets.candidate) {
  console.error("usage: pages.mjs --baseline URL --candidate URL [--base-path P] [--json FILE] [--only a,b]");
  process.exit(2);
}
const basePath = args["base-path"] ?? "";
const only = args.only ? new Set(args.only.split(",")) : null;
const TIMEOUT = 15_000;
const ns = `b${Date.now().toString(36)}`;

const pathOf = (page) => {
  const url = new URL(page.url());
  const pathname = basePath && url.pathname.startsWith(basePath) ? url.pathname.slice(basePath.length) || "/" : url.pathname;
  return `${pathname}${url.search}`;
};
const text = async (page, selector) =>
  (await page.locator(selector).first().textContent({ timeout: 2000 }).catch(() => null))?.trim() ?? null;

async function waitFor(page, selector, expected) {
  const deadline = Date.now() + TIMEOUT;
  for (;;) {
    const got = await text(page, selector);
    if (got === expected) return got;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${selector}=${expected} at ${pathOf(page)} (saw ${got})`);
    await page.waitForTimeout(50);
  }
}

// The distinct /_next/data requests a page sent and their statuses
// (next/link prefetches the links in view, so how often and in which
// order varies from run to run).
const distinct = (seen) => [...new Set(seen.map((d) => `${d.status} ${d.path}${d.search}`))].sort();

// The /_next/data requests a page sent, and how they were answered.
function trackData(page) {
  const seen = [];
  page.on("response", (response) => {
    const url = new URL(response.url());
    if (!url.pathname.includes("/_next/data/")) return;
    const pathname = basePath && url.pathname.startsWith(`${basePath}/`) ? url.pathname.slice(basePath.length) : url.pathname;
    seen.push({ path: pathname.replace(/\/_next\/data\/[^/]+\//, "/_next/data/BUILD/"), search: url.search, status: response.status() });
  });
  return seen;
}

// The Pages Router client is hydrated once its router exists and is ready
// (window.__NEXT_HYDRATED is set in Next's test mode only).
async function hydrated(page) {
  await page.waitForFunction(() => window.next?.router?.isReady === true, null, { timeout: TIMEOUT });
}

async function clientNavigate(page, href) {
  await page.evaluate((target) => window.next.router.push(target), href);
}

const scenarios = {
  async "hydration-and-global-css"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    const color = await page.evaluate(() => getComputedStyle(document.getElementById("app-shell")).color);
    const lang = await page.evaluate(() => document.documentElement.getAttribute("data-doc"));
    return { title: await text(page, "#title"), color, lang, page: await page.evaluate(() => window.next.router.pathname) };
  },
  async "client-nav-ssr-data"(page, base) {
    const data = trackData(page);
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    await page.click("#link-book");
    await waitFor(page, "#title", "Book 42");
    return { url: pathOf(page), slug: await text(page, "#slug"), query: await text(page, "#query"), data: distinct(data) };
  },
  async "client-nav-ssg-and-isr"(page, base) {
    const data = trackData(page);
    await page.goto(`${base}${basePath}/about`);
    await hydrated(page);
    await page.click("#link-post");
    await waitFor(page, "#title", "Post alpha");
    await page.click("#link-ssg");
    await waitFor(page, "#title", "SSG static");
    await page.click("#link-isr");
    await waitFor(page, "#title", "Pages ISR");
    return { url: pathOf(page), data: distinct(data) };
  },
  async "back-forward-reload"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    await page.click("#link-about");
    await waitFor(page, "#title", "About");
    await page.click("#link-book");
    await waitFor(page, "#title", "Book 42");
    await page.goBack();
    await waitFor(page, "#title", "About");
    await page.goBack();
    await waitFor(page, "#title", "NextRS Pages Router");
    await page.goForward();
    await waitFor(page, "#title", "About");
    await page.goForward();
    await waitFor(page, "#title", "Book 42");
    await page.reload();
    await hydrated(page);
    return { url: pathOf(page), title: await text(page, "#title") };
  },
  async "client-nav-ssr-redirect"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    await page.click("#link-ssr-redirect");
    await waitFor(page, "#title", "About");
    return { url: pathOf(page) };
  },
  async "client-nav-ssr-not-found"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    await page.click("#link-ssr-not-found");
    await waitFor(page, "#title", "Custom 404 (pages)");
    return { url: pathOf(page) };
  },
  async "client-nav-unknown-static-path"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    await clientNavigate(page, "/posts/gamma");
    await waitFor(page, "#title", "Custom 404 (pages)");
    return { url: pathOf(page) };
  },
  async "direct-fallback-true"(page, base, side) {
    const slug = `fb-${ns}-${side}`;
    await page.goto(`${base}${basePath}/fallback-true/${slug}`);
    await waitFor(page, "#kind", "fallback-true");
    return { url: pathOf(page).replace(side, "SIDE"), slug: (await text(page, "#slug"))?.replace(side, "SIDE") };
  },
  async "client-nav-fallback-blocking"(page, base, side) {
    const slug = `blk-${ns}-${side}`;
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    await clientNavigate(page, `/fallback-blocking/${slug}`);
    await waitFor(page, "#kind", "fallback-blocking");
    return { slug: (await text(page, "#slug"))?.replace(side, "SIDE") };
  },
  async "preview-mode"(page, base) {
    await page.goto(`${base}${basePath}/api/preview?secret=preview-secret&user=browser`);
    await page.goto(`${base}${basePath}/preview`);
    await hydrated(page);
    const kind = await text(page, "#kind");
    await page.goto(`${base}${basePath}/api/clear-preview`);
    await page.goto(`${base}${basePath}/preview`);
    return { previewKind: kind, afterClear: await text(page, "#kind") };
  },
  async "protected-client-nav-anonymous"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    await clientNavigate(page, "/protected/secret");
    await waitFor(page, "#title", "About");
    return { url: pathOf(page), leaked: (await page.content()).includes("top-secret") };
  },
  async "protected-client-nav-session"(page, base) {
    const url = new URL(base);
    await page.context().addCookies([
      { name: "session", value: "synthetic-valid-session", domain: url.hostname, path: "/" },
      { name: "user", value: "u-browser", domain: url.hostname, path: "/" },
    ]);
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    await clientNavigate(page, "/protected/secret");
    await waitFor(page, "#title", "Protected");
    return { url: pathOf(page), secret: await text(page, "#secret") };
  },
  async "api-from-the-page"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await hydrated(page);
    return page.evaluate(async (prefix) => {
      const r = await fetch(`${prefix}/api/echo?from=browser`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ hello: "world" }) });
      const body = await r.json();
      return { status: r.status, method: body.method, body: body.body, query: body.query };
    }, basePath);
  },
};

const expectations = {
  "hydration-and-global-css": (o) => (o.title === "NextRS Pages Router" && o.color === "rgb(12, 34, 56)" && o.lang === "nextrs-pages-doc" && o.page === "/" ? null : "not hydrated with the custom App/Document and global CSS"),
  "client-nav-ssr-data": (o) =>
    o.url === "/books/42?ref=nav" && o.slug === "42" && o.data.includes("200 /_next/data/BUILD/books/42.json?ref=nav&id=42") ? null : "client navigation did not fetch the page data",
  "client-nav-ssg-and-isr": (o) =>
    o.url === "/isr" && ["/posts/alpha.json", "/ssg/static.json", "/isr.json"].every((p) => o.data.some((d) => d.startsWith(`200 /_next/data/BUILD${p}`))) && o.data.every((d) => d.startsWith("200 ")) ? null : `data requests ${o.data}`,
  "back-forward-reload": (o) => (o.title === "Book 42" && o.url === "/books/42?ref=nav" ? null : "history"),
  "client-nav-ssr-redirect": (o) => (o.url === "/about?from=ssr" ? null : `ended at ${o.url}`),
  "client-nav-ssr-not-found": (o) => (o.url === "/ssr/not-found" ? null : `ended at ${o.url}`),
  "client-nav-unknown-static-path": (o) => (o.url === "/posts/gamma" ? null : `ended at ${o.url}`),
  "direct-fallback-true": (o) => (o.slug?.startsWith("fb-") ? null : "fallback page did not render"),
  "client-nav-fallback-blocking": (o) => (o.slug?.startsWith("blk-") ? null : "blocking fallback did not render"),
  "preview-mode": (o) => (o.previewKind === "preview" && o.afterClear === "published" ? null : `preview ${o.previewKind}, then ${o.afterClear}`),
  "protected-client-nav-anonymous": (o) => (o.url === "/about?login=required" && !o.leaked ? null : `ended at ${o.url}, leaked ${o.leaked}`),
  "protected-client-nav-session": (o) => (o.secret === "top-secret-for-u-browser" ? null : `saw ${o.secret}`),
  "api-from-the-page": (o) => (o.status === 200 && o.body?.hello === "world" && o.query?.from === "browser" ? null : "API call"),
};

const browser = await chromium.launch();
const results = [];
let failures = 0;
for (const [name, run] of Object.entries(scenarios)) {
  if (only && !only.has(name)) continue;
  const observed = {};
  try {
    for (const [side, base] of Object.entries(targets)) {
      const context = await browser.newContext();
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (error) => errors.push(String(error.message).slice(0, 200)));
      page.on("console", (message) => {
        if (message.type() === "error" && !/Failed to load resource/.test(message.text())) errors.push(message.text().slice(0, 200));
      });
      try {
        observed[side] = { ...(await run(page, base, side)), errors };
      } finally {
        await context.close();
      }
    }
    const same = JSON.stringify(observed.baseline) === JSON.stringify(observed.candidate);
    const wrong = expectations[name]?.(observed.candidate) ?? null;
    const oracleWrong = expectations[name]?.(observed.baseline) ?? null;
    const errors = observed.candidate.errors.length ? `script errors: ${observed.candidate.errors.join(" | ")}` : null;
    const verdict = oracleWrong ? "ORACLE-FAIL" : wrong || errors ? "FAIL" : same ? "MATCH" : "DIFF";
    if (verdict !== "MATCH") failures++;
    results.push({ name, verdict, observed, wrong, oracleWrong });
    console.log(`${verdict.padEnd(10)} ${name}`);
    if (verdict !== "MATCH") console.log(`    ${JSON.stringify({ wrong: wrong ?? errors, oracleWrong, observed }).slice(0, 900)}`);
  } catch (error) {
    failures++;
    results.push({ name, verdict: "ERROR", error: String(error?.stack ?? error) });
    console.log(`ERROR      ${name}\n    ${String(error?.message ?? error).slice(0, 400)}`);
  }
}
await browser.close();
const count = (v) => results.filter((r) => r.verdict === v).length;
console.log(`browser-pages: ${count("MATCH")} MATCH, ${count("DIFF")} DIFF, ${count("FAIL")} FAIL, ${count("ORACLE-FAIL")} ORACLE-FAIL, ${count("ERROR")} ERROR`);
if (args.json) writeFileSync(args.json, JSON.stringify({ results }, null, 2));
process.exit(failures ? 1 : 0);
