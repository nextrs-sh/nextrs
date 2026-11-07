// Browser-driven scenarios for the real-world fixtures, run against next
// start (the baseline) and nextrs (the candidate): what a user sees (URL,
// visible text, cookie names) must agree.
//
//   node tests/browser/realworld.mjs --app auth|intl --baseline URL --candidate URL \
//        [--upstream URL] [--json out.json] [--only a,b]
//
//   auth  fixtures/next-auth-proxy: jose sessions, login/logout Server
//         Actions, redirects back to the destination, client navigation
//         through the proxy, a protected mutation
//   intl  fixtures/next-intl-proxy: next-intl locale detection from the
//         browser's language, prefixed navigation, locale switching, a
//         locale-aware Server Action
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
if (!targets.baseline || !targets.candidate || !["auth", "intl"].includes(args.app)) {
  console.error("usage: realworld.mjs --app auth|intl --baseline URL --candidate URL [--upstream URL] [--json FILE] [--only a,b]");
  process.exit(2);
}
const only = args.only ? new Set(args.only.split(",")) : null;
const TIMEOUT = 15_000;
const run = Date.now().toString(36);

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
async function waitText(page, selector) {
  const deadline = Date.now() + TIMEOUT;
  for (;;) {
    const got = await text(page, selector);
    if (got) return got;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${selector} at ${pathOf(page)}`);
    await page.waitForTimeout(50);
  }
}
const cookieNames = async (context) => (await context.cookies()).map((c) => c.name).sort();

async function login(page, base, { username = "alice", password = "alice-test-password", from = "/dashboard" } = {}) {
  await page.goto(`${base}${from}`);
  await waitTitle(page, "Log in");
  await page.fill("#username", username);
  await page.fill("#password", password);
  await page.click("#submit");
}

const auth = {
  async "anonymous-direct-load-redirects-to-login"(page, base) {
    await page.goto(`${base}/dashboard?tab=1`);
    await waitTitle(page, "Log in");
    return { url: pathOf(page), query: await text(page, "#query"), secret: await text(page, "#secret") };
  },
  async "anonymous-client-nav-redirects-to-login"(page, base) {
    await page.goto(`${base}/`);
    await waitTitle(page, "Home");
    await page.click("#to-account");
    await waitTitle(page, "Log in");
    return { url: pathOf(page), secret: await text(page, "#secret") };
  },
  async "login-then-dashboard"(page, base, context) {
    await login(page, base);
    await waitTitle(page, "Dashboard");
    return { url: pathOf(page), secret: await text(page, "#secret"), cookies: await cookieNames(context) };
  },
  async "login-redirects-back-to-isr-page"(page, base) {
    await login(page, base, { from: "/reports" });
    await waitTitle(page, "Reports");
    return { url: pathOf(page), secret: await text(page, "#secret") };
  },
  async "login-wrong-password"(page, base, context) {
    await login(page, base, { password: "wrong" });
    await page.waitForURL(/error=1/, { timeout: TIMEOUT });
    await waitTitle(page, "Log in");
    return { url: pathOf(page), query: await text(page, "#query"), cookies: await cookieNames(context) };
  },
  async "client-nav-after-login"(page, base) {
    await login(page, base, { username: "bob", password: "bob-test-password" });
    await waitTitle(page, "Dashboard");
    await page.click("#to-project");
    await waitTitle(page, "Project");
    return { url: pathOf(page), secret: await text(page, "#secret") };
  },
  async "protected-mutation"(page, base) {
    const key = `browser-auth-${run}-${base.slice(-4)}`;
    await login(page, base, { from: `/actions/protected?key=${key}` });
    await waitTitle(page, "Protected action");
    await page.click("#record");
    const result = JSON.parse(await waitText(page, "#result"));
    let executions = null;
    if (args.upstream) {
      await page.waitForTimeout(100);
      executions = (await (await fetch(`${args.upstream}/mutations?key=${encodeURIComponent(key)}`)).json()).ids;
    }
    return { url: pathOf(page).replace(key, "KEY"), result, executions };
  },
  async "logout-ends-the-session"(page, base, context) {
    await login(page, base);
    await waitTitle(page, "Dashboard");
    await page.click("#logout");
    await waitTitle(page, "Log in");
    const afterLogout = pathOf(page);
    await page.goto(`${base}/dashboard`);
    await waitTitle(page, "Log in");
    return { afterLogout, revisit: pathOf(page), cookies: await cookieNames(context) };
  },
};

const intl = {
  async "browser-language-redirect"(page, base) {
    await page.goto(`${base}/`);
    await waitTitle(page, "Hallo");
    return { url: pathOf(page), lang: await page.getAttribute("html", "lang") };
  },
  async "client-nav-prefixed"(page, base) {
    await page.goto(`${base}/de`);
    await waitTitle(page, "Hallo");
    await page.click("#to-about");
    await waitTitle(page, "Über uns");
    return { url: pathOf(page) };
  },
  async "client-nav-dynamic"(page, base) {
    await page.goto(`${base}/fr`);
    await waitTitle(page, "Bonjour");
    await page.click("#to-product");
    await waitTitle(page, "Produit 42");
    return { url: pathOf(page), slug: await text(page, "#slug") };
  },
  async "locale-switch"(page, base, context) {
    await page.goto(`${base}/en`);
    await waitTitle(page, "Hello");
    await page.click("#to-de");
    await waitTitle(page, "Hallo");
    return { url: pathOf(page), cookies: (await context.cookies()).filter((c) => c.name === "NEXT_LOCALE").map((c) => c.value) };
  },
  async "unprefixed-direct-load"(page, base) {
    await page.goto(`${base}/about`);
    await waitTitle(page, "Über uns");
    return { url: pathOf(page) };
  },
  async "locale-aware-action"(page, base) {
    await page.goto(`${base}/fr/actions`);
    await waitTitle(page, "Salutation");
    await page.click("#greet");
    return { result: JSON.parse(await waitText(page, "#result")) };
  },
};

const scenarios = args.app === "auth" ? auth : intl;
const contextOptions = args.app === "intl" ? { locale: "de-DE" } : {};

async function runScenario(browser, name, base) {
  const context = await browser.newContext(contextOptions);
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
