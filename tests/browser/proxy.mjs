// Browser-driven proxy (proxy.ts) and next.config routing scenarios on
// fixtures/next-proxy, run against next start (the baseline) and nextrs
// (the candidate). Client-side navigations send RSC requests through the
// proxy and the config rules; each scenario returns what a user observes
// (URL, visible text, cookies) and the two servers must agree.
//
//   node tests/browser/proxy.mjs --baseline URL --candidate URL [--base-path /base] [--json out.json]
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
  console.error("usage: proxy.mjs --baseline URL --candidate URL [--base-path P] [--json FILE] [--only a,b]");
  process.exit(2);
}
const basePath = args["base-path"] ?? "";
const only = args.only ? new Set(args.only.split(",")) : null;
const TIMEOUT = 15_000;
const SESSION = { name: "session", value: "synthetic-valid-session" };

const pathOf = (page) => {
  const url = new URL(page.url());
  return `${url.pathname}${url.search}`;
};
const text = async (page, selector) =>
  (await page.locator(selector).textContent({ timeout: TIMEOUT }).catch(() => null))?.trim() ?? null;

async function waitTitle(page, title) {
  const deadline = Date.now() + TIMEOUT;
  for (;;) {
    const got = await text(page, "#title");
    if (got === title) return got;
    if (Date.now() > deadline) throw new Error(`timed out waiting for title ${title} at ${pathOf(page)} (saw ${got})`);
    await page.waitForTimeout(50);
  }
}

// RSC requests the page sent and how they were answered.
function trackRsc(page) {
  const seen = [];
  page.on("response", async (response) => {
    const headers = response.request().headers();
    if (headers.rsc !== "1") return;
    const h = await response.allHeaders();
    seen.push({
      path: new URL(response.url()).pathname,
      status: response.status(),
      type: (h["content-type"] ?? "").split(";")[0],
      proxied: h["x-proxy-response"] ?? null,
      rewrittenPath: h["x-nextjs-rewritten-path"] ?? null,
    });
  });
  return seen;
}

async function clickAndWait(page, id, title) {
  await page.click(`#${id}`);
  await waitTitle(page, title);
}

const cookieNames = async (context) => (await context.cookies()).map((c) => `${c.name}=${c.value}`).sort();

const scenarios = {
  async "client-nav-proxy-rewrite"(page, base) {
    const rsc = trackRsc(page);
    await page.goto(`${base}${basePath}/`);
    await waitTitle(page, "home");
    await clickAndWait(page, "link-proxy-rewrite", "target");
    // Prefetches fire on viewport timing: compare the distinct answers per
    // path, not their count or order.
    // Which other links get prefetched within the window varies too, so
    // only the navigation's own requests are compared.
    const own = rsc.filter((r) => [`${basePath}/proxy/rewrite`, `${basePath}/target`].includes(r.path));
    const distinct = [...new Set(own.map((r) => JSON.stringify(r)))].sort();
    return { url: pathOf(page), rsc: distinct.map((r) => JSON.parse(r)) };
  },
  async "client-nav-proxy-redirect"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await waitTitle(page, "home");
    await clickAndWait(page, "link-proxy-redirect", "target");
    return { url: pathOf(page) };
  },
  async "client-nav-protected-anonymous"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await waitTitle(page, "home");
    await clickAndWait(page, "link-protected", "login");
    return { url: pathOf(page), secret: await text(page, "#secret") };
  },
  async "client-nav-protected-session"(page, base, context) {
    await context.addCookies([{ ...SESSION, url: base }]);
    await page.goto(`${base}${basePath}/`);
    await waitTitle(page, "home");
    await clickAndWait(page, "link-protected", "protected");
    return { url: pathOf(page), secret: await text(page, "#secret") };
  },
  async "direct-load-protected"(page, base) {
    await page.goto(`${base}${basePath}/protected/isr`);
    await waitTitle(page, "login");
    return { url: pathOf(page), secret: await text(page, "#secret") };
  },
  async "client-nav-dynamic-headers"(page, base, context) {
    await page.goto(`${base}${basePath}/`);
    await waitTitle(page, "home");
    await clickAndWait(page, "link-dynamic", "dynamic");
    const info = JSON.parse((await text(page, "#info")) ?? "{}");
    return {
      url: pathOf(page),
      proxyRequest: info.proxyRequest,
      proxyVisit: info.proxyVisit,
      cookies: await cookieNames(context),
    };
  },
  async "client-nav-config-rewrite"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await waitTitle(page, "home");
    await clickAndWait(page, "link-af", "blog");
    return { url: pathOf(page), slug: await text(page, "#slug") };
  },
  async "client-nav-config-redirect"(page, base) {
    await page.goto(`${base}${basePath}/`);
    await waitTitle(page, "home");
    await clickAndWait(page, "link-old-blog", "blog");
    return { url: pathOf(page), slug: await text(page, "#slug") };
  },
  async "server-action-through-proxy"(page, base, context) {
    await page.goto(`${base}${basePath}/actions`);
    await waitTitle(page, "actions");
    await page.click("#inspect");
    const deadline = Date.now() + TIMEOUT;
    let result = "";
    while (!result && Date.now() < deadline) {
      result = (await text(page, "#result")) ?? "";
      if (!result) await page.waitForTimeout(50);
    }
    return { url: pathOf(page), result: result ? JSON.parse(result) : null, cookies: await cookieNames(context) };
  },
};

async function runScenario(browser, name, base) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));
  try {
    const observed = await scenarios[name](page, base, context);
    return { observed, errors };
  } catch (error) {
    return { failed: String(error?.message ?? error).split("\n")[0], errors };
  } finally {
    await context.close();
  }
}

const browser = await chromium.launch({ headless: true });
const results = [];
let failures = 0;
for (const name of Object.keys(scenarios)) {
  if (only && !only.has(name)) continue;
  const baseline = await runScenario(browser, name, targets.baseline);
  const candidate = await runScenario(browser, name, targets.candidate);
  const same = JSON.stringify(baseline) === JSON.stringify(candidate);
  const verdict = baseline.failed ? "BASELINE-FAILED" : same ? "PASS" : "DIFF";
  if (verdict !== "PASS") failures++;
  results.push({ name, verdict, baseline, candidate });
  console.log(`${verdict.padEnd(15)} ${name}`);
  if (verdict !== "PASS") {
    console.log(`  baseline:  ${JSON.stringify(baseline).slice(0, 1500)}`);
    console.log(`  candidate: ${JSON.stringify(candidate).slice(0, 1500)}`);
  }
}
await browser.close();
if (args.json) writeFileSync(args.json, JSON.stringify({ results }, null, 2));
console.log(`${results.length - failures}/${results.length} scenarios equal`);
process.exit(failures ? 1 : 0);
