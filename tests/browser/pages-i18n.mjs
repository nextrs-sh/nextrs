// Next's native i18n routing in Chromium on fixtures/next-pages-i18n
// (default build), against next start (the baseline) and NextRS (the
// candidate): hydration under a locale, locale switching with next/link,
// client navigation through localized /_next/data routes, back/forward,
// locale detection from the browser's language and the NEXT_LOCALE cookie,
// a protected page and the localized 404 reached by the client router.
// Each scenario returns what a user observes; the two servers must agree,
// and each scenario also asserts its own outcome.
//
//   node tests/browser/pages-i18n.mjs --baseline URL --candidate URL [--json out.json] [--only a,b]
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
  console.error("usage: pages-i18n.mjs --baseline URL --candidate URL [--json FILE] [--only a,b]");
  process.exit(2);
}
const only = args.only ? new Set(args.only.split(",")) : null;
const TIMEOUT = 15_000;

const pathOf = (page) => {
  const url = new URL(page.url());
  return `${url.pathname}${url.search}`;
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

// The distinct /_next/data requests a page sent (next/link prefetches the
// links in view, so how often and in which order varies).
const distinct = (seen) => [...new Set(seen.map((d) => `${d.status} ${d.path}`))].sort();
function trackData(page) {
  const seen = [];
  page.on("response", (response) => {
    const url = new URL(response.url());
    if (!url.pathname.includes("/_next/data/")) return;
    seen.push({ path: url.pathname.replace(/\/_next\/data\/[^/]+\//, "/_next/data/BUILD/"), status: response.status() });
  });
  return seen;
}

async function hydrated(page) {
  await page.waitForFunction(() => window.next?.router?.isReady === true, null, { timeout: TIMEOUT });
}
// A marker on window survives client navigation, not a document load.
const mark = (page) => page.evaluate(() => (window.__nextrsMark = 1));
const marked = (page) => page.evaluate(() => window.__nextrsMark === 1);
const routerLocale = (page) => page.evaluate(() => window.next.router.locale);

const scenarios = {
  async "hydration-under-locale"(page, base) {
    await page.goto(`${base}/fr`);
    await hydrated(page);
    return { url: pathOf(page), title: await text(page, "#title"), locale: await text(page, "#locale"), router: await routerLocale(page), lang: await page.evaluate(() => document.documentElement.lang) };
  },
  async "switch-locale-link"(page, base) {
    await page.goto(`${base}/about`);
    await hydrated(page);
    await mark(page);
    await page.click("#to-ja");
    await page.waitForURL(/\/ja\/about$/, { timeout: TIMEOUT });
    await waitFor(page, "#router-locale", "ja");
    return { url: pathOf(page), locale: await text(page, "#locale"), router: await routerLocale(page), client: await marked(page) };
  },
  async "client-nav-ssr-data"(page, base) {
    const data = trackData(page);
    await page.goto(`${base}/fr`);
    await hydrated(page);
    await mark(page);
    await page.click("#to-ssr");
    await page.waitForURL(/\/fr\/ssr$/, { timeout: TIMEOUT });
    await waitFor(page, "#title", "SSR fr");
    return { url: pathOf(page), locale: await text(page, "#locale"), client: await marked(page), data: distinct(data).filter((d) => d.includes("/fr/ssr.json")) };
  },
  async "client-nav-other-locale-ssg"(page, base) {
    const data = trackData(page);
    await page.goto(`${base}/ja/about`);
    await hydrated(page);
    await mark(page);
    await page.click("#to-post-fr");
    await page.waitForURL(/\/fr\/posts\/hello$/, { timeout: TIMEOUT });
    await waitFor(page, "#locale", "fr");
    return { url: pathOf(page), slug: await text(page, "#slug"), router: await routerLocale(page), client: await marked(page), data: distinct(data).filter((d) => d.includes("/fr/posts/hello.json")) };
  },
  async "back-forward-across-locales"(page, base) {
    await page.goto(`${base}/fr/ssr`);
    await hydrated(page);
    await page.click("#to-ja");
    await page.waitForURL(/\/ja\/ssr$/, { timeout: TIMEOUT });
    await waitFor(page, "#title", "SSR ja");
    await page.goBack();
    await waitFor(page, "#title", "SSR fr");
    const back = { url: pathOf(page), router: await routerLocale(page) };
    await page.goForward();
    await waitFor(page, "#title", "SSR ja");
    await page.reload();
    await hydrated(page);
    return { back, forward: { url: pathOf(page), router: await routerLocale(page), title: await text(page, "#title") } };
  },
  async "detect-browser-language"(page, base) {
    await page.goto(`${base}/`);
    await hydrated(page);
    return { url: pathOf(page), title: await text(page, "#title") };
  },
  async "detect-next-locale-cookie"(page, base) {
    await page.context().addCookies([{ name: "NEXT_LOCALE", value: "ja", url: base }]);
    await page.goto(`${base}/`);
    await hydrated(page);
    return { url: pathOf(page), title: await text(page, "#title") };
  },
  async "protected-client-nav"(page, base) {
    await page.goto(`${base}/fr/about`);
    await hydrated(page);
    await page.evaluate(() => window.next.router.push("/protected/secret"));
    await page.waitForURL(/login=required/, { timeout: TIMEOUT });
    return { url: pathOf(page), leaked: (await page.content()).includes("secret-for") };
  },
  async "not-found-client-nav"(page, base) {
    await page.goto(`${base}/ja/about`);
    await hydrated(page);
    await page.evaluate(() => window.next.router.push("/nope"));
    await waitFor(page, "#kind", "404");
    return { url: pathOf(page), title: await text(page, "#title"), locale: await text(page, "#locale") };
  },
};

// Contexts per scenario: the browser's language decides detection.
// A browser set to French (Chromium then sends Accept-Language: fr; a bare
// "fr-FR" matches no configured locale in Next and redirects nowhere).
const contextOptions = { "detect-browser-language": { locale: "fr" } };

const expectations = {
  "hydration-under-locale": (o) => (o.title === "Bonjour" && o.locale === "fr" && o.router === "fr" && o.lang === "fr" ? null : `hydrated as ${JSON.stringify(o)}`),
  "switch-locale-link": (o) => (o.url === "/ja/about" && o.locale === "ja" && o.router === "ja" && o.client ? null : `after switching: ${JSON.stringify(o)}`),
  "client-nav-ssr-data": (o) => (o.url === "/fr/ssr" && o.locale === "fr" && o.client && o.data.includes("200 /_next/data/BUILD/fr/ssr.json") ? null : `client navigation: ${JSON.stringify(o)}`),
  "client-nav-other-locale-ssg": (o) => (o.url === "/fr/posts/hello" && o.slug === "hello" && o.router === "fr" && o.data.includes("200 /_next/data/BUILD/fr/posts/hello.json") ? null : `navigation to another locale: ${JSON.stringify(o)}`),
  "back-forward-across-locales": (o) => (o.back.url === "/fr/ssr" && o.back.router === "fr" && o.forward.url === "/ja/ssr" && o.forward.router === "ja" && o.forward.title === "SSR ja" ? null : `history: ${JSON.stringify(o)}`),
  "detect-browser-language": (o) => (o.url === "/fr" && o.title === "Bonjour" ? null : `a French browser ended at ${o.url}`),
  "detect-next-locale-cookie": (o) => (o.url === "/ja" && o.title === "こんにちは" ? null : `NEXT_LOCALE=ja ended at ${o.url}`),
  "protected-client-nav": (o) => (o.url === "/fr/about?login=required" && !o.leaked ? null : `ended at ${o.url}, leaked ${o.leaked}`),
  "not-found-client-nav": (o) => (o.url === "/ja/nope" && o.title === "見つかりません" && o.locale === "ja" ? null : `404 ${JSON.stringify(o)}`),
};

const browser = await chromium.launch();
const results = [];
let failures = 0;
for (const [name, run] of Object.entries(scenarios)) {
  if (only && !only.has(name)) continue;
  const observed = {};
  try {
    for (const [side, base] of Object.entries(targets)) {
      const context = await browser.newContext(contextOptions[name] ?? { locale: "en-US" });
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
console.log(`browser-pages-i18n: ${count("MATCH")} MATCH, ${count("DIFF")} DIFF, ${count("FAIL")} FAIL, ${count("ORACLE-FAIL")} ORACLE-FAIL, ${count("ERROR")} ERROR`);
if (args.json) writeFileSync(args.json, JSON.stringify({ results }, null, 2));
process.exit(failures ? 1 : 0);
