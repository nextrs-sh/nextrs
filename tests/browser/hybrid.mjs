// Browser-driven App Router + Pages Router navigation on fixtures/next-hybrid,
// against next start (baseline) and NextRS (candidate): links from one
// router to the other (each router's client hands the navigation over),
// back/forward across them, reload, cookie continuity, an authenticated
// page behind the proxy in each router, and an optimized image loading.
//
//   node tests/browser/hybrid.mjs --baseline URL --candidate URL [--json out.json]
import { writeFileSync } from "node:fs";
import { chromium } from "playwright-core";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const targets = { baseline: args.baseline, candidate: args.candidate };
const TIMEOUT = 15_000;
const pathOf = (page) => {
  const url = new URL(page.url());
  return `${url.pathname}${url.search}`;
};
const text = async (page, selector) => (await page.locator(selector).first().textContent({ timeout: 2000 }).catch(() => null))?.trim() ?? null;
async function waitTitle(page, expected) {
  const deadline = Date.now() + TIMEOUT;
  for (;;) {
    const got = await text(page, "#title");
    if (got === expected) return;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${expected} at ${pathOf(page)} (saw ${got})`);
    await page.waitForTimeout(50);
  }
}
// Either router's client is ready: the App Router's (window.next with
// appDir) or the Pages Router's (router.isReady).
async function ready(page) {
  await page.waitForFunction(() => window.next !== undefined && (window.next.appDir === true || window.next.router?.isReady === true), null, { timeout: TIMEOUT });
}
const router = (page) => page.evaluate(() => (window.next?.appDir ? "app" : window.next?.router ? "pages" : "none"));

const scenarios = {
  async "app-to-pages"(page, base) {
    await page.goto(`${base}/app-page`);
    await ready(page);
    const from = await router(page);
    await page.click("#to-pages");
    await waitTitle(page, "Pages Router page");
    await ready(page);
    return { from, to: await router(page), url: pathOf(page) };
  },
  async "pages-to-app"(page, base) {
    await page.goto(`${base}/pages-page`);
    await ready(page);
    const from = await router(page);
    await page.click("#to-app");
    await waitTitle(page, "App Router page");
    await ready(page);
    return { from, to: await router(page), url: pathOf(page) };
  },
  async "dynamic-across-routers"(page, base) {
    await page.goto(`${base}/app-items/3`);
    await ready(page);
    await page.click("#to-pages-ssr");
    await waitTitle(page, "Pages item 7");
    await ready(page);
    await page.click("#to-app-dynamic");
    await waitTitle(page, "App item 4");
    return { url: pathOf(page), slug: await text(page, "#slug") };
  },
  async "back-forward-across-routers"(page, base) {
    await page.goto(`${base}/pages-page`);
    await ready(page);
    await page.click("#to-app");
    await waitTitle(page, "App Router page");
    await ready(page);
    await page.click("#to-pages-ssr");
    await waitTitle(page, "Pages item 5");
    await page.goBack();
    await waitTitle(page, "App Router page");
    await page.goBack();
    await waitTitle(page, "Pages Router page");
    await page.goForward();
    await waitTitle(page, "App Router page");
    await page.reload();
    await ready(page);
    return { url: pathOf(page), title: await text(page, "#title") };
  },
  async "cookie-continuity"(page, base) {
    const url = new URL(base);
    await page.context().addCookies([{ name: "user", value: "u-cookie", domain: url.hostname, path: "/" }]);
    await page.goto(`${base}/app-items/3`);
    await ready(page);
    const app = await text(page, "#secret");
    await page.click("#to-pages-ssr");
    await waitTitle(page, "Pages item 7");
    return { app, pages: await text(page, "#secret") };
  },
  async "authenticated-both-routers"(page, base) {
    const url = new URL(base);
    await page.goto(`${base}/secure/pages-secret`);
    const anonymous = pathOf(page);
    await page.context().addCookies([{ name: "session", value: "synthetic-valid-session", domain: url.hostname, path: "/" }]);
    await page.goto(`${base}/pages-page`);
    await ready(page);
    await page.evaluate(() => window.next.router.push("/secure/pages-secret"));
    await waitTitle(page, "Pages secret");
    const pages = await text(page, "#secret");
    await page.goto(`${base}/secure/app-secret`);
    await waitTitle(page, "App secret");
    return { anonymous, pages, app: await text(page, "#secret") };
  },
  async "image-loads"(page, base) {
    await page.goto(`${base}/with-image`);
    await page.waitForFunction(() => document.querySelector("#swatch")?.complete === true, null, { timeout: TIMEOUT });
    return page.evaluate(() => {
      const img = document.querySelector("#swatch");
      return { width: img.naturalWidth, height: img.naturalHeight, optimized: img.currentSrc.includes("/_next/image") };
    });
  },
};

const expectations = {
  "app-to-pages": (o) => (o.from === "app" && o.to === "pages" && o.url === "/pages-page" ? null : JSON.stringify(o)),
  "pages-to-app": (o) => (o.from === "pages" && o.to === "app" && o.url === "/app-page" ? null : JSON.stringify(o)),
  "dynamic-across-routers": (o) => (o.url === "/app-items/4" && o.slug === "4" ? null : JSON.stringify(o)),
  "back-forward-across-routers": (o) => (o.url === "/app-page" && o.title === "App Router page" ? null : JSON.stringify(o)),
  "cookie-continuity": (o) => (o.app === "user=u-cookie" && o.pages === "user=u-cookie" ? null : JSON.stringify(o)),
  "authenticated-both-routers": (o) =>
    o.anonymous === "/pages-page?login=required" && o.pages === "pages-top-secret" && o.app === "app-top-secret" ? null : JSON.stringify(o),
  "image-loads": (o) => (o.width === 64 && o.height === 48 && o.optimized ? null : JSON.stringify(o)),
};

const browser = await chromium.launch();
const results = [];
let failures = 0;
for (const [name, run] of Object.entries(scenarios)) {
  const observed = {};
  try {
    for (const [side, base] of Object.entries(targets)) {
      const context = await browser.newContext();
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (error) => errors.push(String(error.message).slice(0, 200)));
      try {
        observed[side] = { ...(await run(page, base)), errors };
      } finally {
        await context.close();
      }
    }
    const wrong = expectations[name](observed.candidate);
    const oracleWrong = expectations[name](observed.baseline);
    const errors = observed.candidate.errors.length ? `script errors: ${observed.candidate.errors.join(" | ")}` : null;
    const same = JSON.stringify(observed.baseline) === JSON.stringify(observed.candidate);
    const verdict = oracleWrong ? "ORACLE-FAIL" : wrong || errors ? "FAIL" : same ? "MATCH" : "DIFF";
    if (verdict !== "MATCH") failures++;
    results.push({ name, verdict, observed });
    console.log(`${verdict.padEnd(10)} ${name}`);
    if (verdict !== "MATCH") console.log(`    ${JSON.stringify({ wrong: wrong ?? errors, oracleWrong, observed }).slice(0, 700)}`);
  } catch (error) {
    failures++;
    results.push({ name, verdict: "ERROR", error: String(error?.stack ?? error) });
    console.log(`ERROR      ${name}\n    ${String(error?.message ?? error).slice(0, 300)}`);
  }
}
await browser.close();
const count = (v) => results.filter((r) => r.verdict === v).length;
console.log(`browser-hybrid: ${count("MATCH")} MATCH, ${count("DIFF")} DIFF, ${count("FAIL")} FAIL, ${count("ORACLE-FAIL")} ORACLE-FAIL, ${count("ERROR")} ERROR`);
if (args.json) writeFileSync(args.json, JSON.stringify({ results }, null, 2));
process.exit(failures ? 1 : 0);
