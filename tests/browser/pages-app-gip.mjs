// A custom App with getInitialProps in Chromium (fixtures/next-pages-app-gip)
// against next start (baseline) and NextRS (candidate): hydration of the
// server-rendered App props, client navigation (Next then runs the App's
// and the page's getInitialProps in the browser, and fetches
// getServerSideProps data), the redirect, back/forward and the error page.
//
//   node tests/browser/pages-app-gip.mjs --baseline URL --candidate URL [--json out.json]
import { writeFileSync } from "node:fs";
import { chromium } from "playwright-core";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const targets = { baseline: args.baseline, candidate: args.candidate };
const only = args.only ? new Set(args.only.split(",")) : null;
const TIMEOUT = 15_000;
const pathOf = (page) => {
  const url = new URL(page.url());
  return `${url.pathname}${url.search}`;
};
const text = async (page, selector) => (await page.locator(selector).first().textContent({ timeout: 2000 }).catch(() => null))?.trim() ?? null;
async function waitFor(page, selector, expected) {
  const deadline = Date.now() + TIMEOUT;
  for (;;) {
    const got = await text(page, selector);
    if (got === expected) return got;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${selector}=${expected} at ${pathOf(page)} (saw ${got})`);
    await page.waitForTimeout(50);
  }
}
const hydrated = (page) => page.waitForFunction(() => window.next?.router?.isReady === true, null, { timeout: TIMEOUT });
const app = async (page) => ({ user: await text(page, "#app-user"), theme: await text(page, "#app-theme"), side: await text(page, "#app-side"), path: await text(page, "#app-path") });

const scenarios = {
  async "hydrate-server-app-props"(page, base) {
    await page.context().addCookies([{ name: "theme", value: "dark", url: base }]);
    await page.goto(`${base}/`);
    await hydrated(page);
    return { url: pathOf(page), app: await app(page), title: await text(page, "#title") };
  },
  async "client-nav-runs-gip-in-browser"(page, base) {
    await page.goto(`${base}/`);
    await hydrated(page);
    await page.click("#to-gip");
    await page.waitForURL(/\/gip\?q=nav$/, { timeout: TIMEOUT });
    await waitFor(page, "#page-side", "client");
    return { url: pathOf(page), app: await app(page), query: await text(page, "#page-query") };
  },
  async "client-nav-ssr-data"(page, base) {
    const data = [];
    page.on("response", (r) => {
      if (r.url().includes("/_next/data/") && r.url().includes("/ssr.json")) data.push(r.status());
    });
    await page.context().addCookies([{ name: "theme", value: "green", url: base }]);
    await page.goto(`${base}/gip`);
    await hydrated(page);
    await page.click("#to-ssr");
    await waitFor(page, "#title", "SSR");
    return { url: pathOf(page), app: await app(page), theme: await text(page, "#page-theme"), data: [...new Set(data)] };
  },
  async "client-redirect"(page, base) {
    await page.goto(`${base}/`);
    await hydrated(page);
    await page.evaluate(() => window.next.router.push("/redirect"));
    await page.waitForURL(/from=redirect/, { timeout: TIMEOUT });
    await waitFor(page, "#title", "Page getInitialProps");
    return { url: pathOf(page) };
  },
  async "back-forward"(page, base) {
    await page.goto(`${base}/ssg`);
    await hydrated(page);
    await page.click("#to-gip");
    await waitFor(page, "#page-side", "client");
    await page.goBack();
    await waitFor(page, "#title", "SSG");
    const back = pathOf(page);
    await page.goForward();
    await waitFor(page, "#title", "Page getInitialProps");
    return { back, forward: pathOf(page) };
  },
  async "error-page"(page, base) {
    const response = await page.goto(`${base}/throws`);
    await hydrated(page).catch(() => {});
    return { status: response.status(), title: await text(page, "#title"), side: await text(page, "#error-side") };
  },
};

const expectations = {
  "hydrate-server-app-props": (o) => (o.app.side === "server" && o.app.theme === "dark" && o.title === "Home" ? null : `hydrated ${JSON.stringify(o)}`),
  "client-nav-runs-gip-in-browser": (o) => (o.url === "/gip?q=nav" && o.app.side === "client" && o.app.user === "client" && o.query === '{"q":"nav"}' ? null : `client navigation ${JSON.stringify(o)}`),
  // A getServerSideProps page's data carries the App's props from the
  // server (its getInitialProps runs there for the data request).
  "client-nav-ssr-data": (o) => (o.url === "/ssr" && o.app.side === "server" && o.app.path === "/ssr" && o.app.theme === "green" && o.theme === "green" && o.data.includes(200) ? null : `ssr navigation ${JSON.stringify(o)}`),
  "client-redirect": (o) => (o.url === "/gip?from=redirect" ? null : `ended at ${o.url}`),
  "back-forward": (o) => (o.back === "/ssg" && o.forward === "/gip?q=nav" ? null : `history ${JSON.stringify(o)}`),
  "error-page": (o) => (o.status === 500 && o.title === "Error 500" ? null : `error page ${JSON.stringify(o)}`),
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
      try {
        observed[side] = { ...(await run(page, base, side)), errors: name === "error-page" ? [] : errors };
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
console.log(`browser-pages-app-gip: ${count("MATCH")} MATCH, ${count("DIFF")} DIFF, ${count("FAIL")} FAIL, ${count("ORACLE-FAIL")} ORACLE-FAIL, ${count("ERROR")} ERROR`);
if (args.json) writeFileSync(args.json, JSON.stringify({ results }, null, 2));
process.exit(failures ? 1 : 0);
