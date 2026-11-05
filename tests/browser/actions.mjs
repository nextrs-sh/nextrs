// Browser-driven Server Action scenarios, run against next start (the
// baseline) and nextrs (the candidate) serving the same build. Each
// scenario returns what a user can observe (visible text, URL, cookies) and
// what the browser sent and got back for each action request; the two
// servers' observations must be equal.
//
//   node tests/browser/actions.mjs --baseline http://127.0.0.1:3000 \
//     --candidate http://127.0.0.1:3001 --upstream http://127.0.0.1:4999 [--json out.json]
//
// Needs playwright-core (npm ci in tests/browser) and its Chromium build
// (npx playwright-core install chromium-headless-shell).
import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";
import { chromium } from "playwright-core";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const targets = { baseline: args.baseline, candidate: args.candidate };
const upstream = args.upstream;
if (!targets.baseline || !targets.candidate || !upstream) {
  console.error("usage: actions.mjs --baseline URL --candidate URL --upstream URL [--json FILE] [--only a,b]");
  process.exit(2);
}
const only = args.only ? new Set(args.only.split(",")) : null;
const TIMEOUT = 15_000;

// Deterministic upload bodies, the same for both servers.
const binary = Buffer.from(createHash("sha512").update("nextrs-1d").digest().toString("hex").repeat(2048), "hex"); // 128 KiB
const sha = (b) => createHash("sha256").update(b).digest("hex");

async function textOf(page, selector) {
  return (await page.locator(selector).textContent({ timeout: TIMEOUT }))?.trim() ?? null;
}

// Polls from Node (no code is evaluated in the page) until the element's
// text satisfies `predicate`.
async function waitText(page, selector, predicate) {
  const deadline = Date.now() + TIMEOUT;
  let last = null;
  for (;;) {
    last = (await page.locator(selector).textContent({ timeout: TIMEOUT }).catch(() => null))?.trim() ?? null;
    if (last !== null && predicate(last)) return last;
    if (Date.now() > deadline) {
      throw new Error(`timed out waiting on ${selector} at ${new URL(page.url()).pathname}; last text: ${JSON.stringify(last)?.slice(0, 300)}`);
    }
    await page.waitForTimeout(50);
  }
}

const changed = (t) => t !== "none";

// Action requests the page made: method, the request's content type family,
// and the response status, content type and Server Action headers.
function trackActions(page) {
  const seen = [];
  page.on("response", async (response) => {
    const request = response.request();
    const headers = request.headers();
    if (request.method() !== "POST") return;
    const h = await response.allHeaders();
    seen.push({
      action: Boolean(headers["next-action"]),
      requestType: (headers["content-type"] ?? "").split(";")[0],
      status: response.status(),
      type: (h["content-type"] ?? "").split(";")[0],
      redirect: h["x-action-redirect"] ?? null,
      revalidated: h["x-action-revalidated"] ?? null,
      notFound: h["x-nextjs-action-not-found"] ?? null,
      cacheControl: h["cache-control"] ?? null,
    });
  });
  return seen;
}

const scenarios = {
  async "simple-js"(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/simple`);
    await page.fill("input[name=title]", "Dune");
    await page.click("button[type=submit]");
    await page.waitForResponse((r) => r.request().method() === "POST", { timeout: TIMEOUT });
    await page.waitForTimeout(200);
    return { url: new URL(page.url()).pathname, actions };
  },

  async "simple-nojs"(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/simple`);
    await page.fill("input[name=title]", "Dune");
    await Promise.all([page.waitForNavigation({ timeout: TIMEOUT }), page.click("button[type=submit]")]);
    return { url: new URL(page.url()).pathname, form: await page.locator("form").count(), actions };
  },

  async form(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/form`);
    await page.fill("#title", "Dune");
    await page.fill("#author", "Frank Herbert");
    await page.check("#tag-sf");
    await page.check("#tag-classic");
    await page.fill("#note", "Ünïcödé ✓ 📚 ‮ RTL \u0000 nul");
    await page.click("#submit");
    const first = await waitText(page, "#result", changed);
    // Repeated submissions. React resets uncontrolled fields after a
    // successful form action, so the fields are filled again.
    const resetTitle = await page.inputValue("#title");
    await page.fill("#title", "Dune II");
    await page.fill("#note", "second");
    await page.click("#submit");
    const second = await waitText(page, "#result", (t) => t.includes("second"));
    await page.fill("#title", "   ");
    await page.click("#submit");
    const invalid = await waitText(page, "#result", (t) => t.includes("blank"));
    return {
      first: JSON.parse(first),
      second: JSON.parse(second),
      invalid: JSON.parse(invalid),
      resetTitle,
      pending: await textOf(page, "#pending"),
      submits: await textOf(page, "#submits"),
      actions,
    };
  },

  async "form-nojs"(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/form`);
    await page.fill("#title", "Dune");
    await page.check("#tag-sf");
    await page.fill("#note", "progressive ✓");
    await Promise.all([page.waitForNavigation({ timeout: TIMEOUT }), page.click("#submit")]);
    return { url: new URL(page.url()).pathname, result: await textOf(page, "#result"), actions };
  },

  async upload(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/upload`);
    await page.setInputFiles("#file", { name: "cover ✓.bin", mimeType: "application/octet-stream", buffer: binary });
    await page.setInputFiles("#many", [
      { name: "a.txt", mimeType: "text/plain", buffer: Buffer.from("alpha\n") },
      { name: "empty.dat", mimeType: "application/x-empty", buffer: Buffer.alloc(0) },
    ]);
    await page.click("#submit");
    const result = JSON.parse(await waitText(page, "#result", changed));
    return { result, expectedSha: sha(binary), actions };
  },

  async "upload-nojs"(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/upload`);
    await page.setInputFiles("#file", { name: "cover.bin", mimeType: "application/octet-stream", buffer: binary });
    await Promise.all([page.waitForNavigation({ timeout: TIMEOUT }), page.click("#submit")]);
    return { result: await textOf(page, "#result"), actions };
  },

  async bound(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/bound`);
    await page.click("#bound-submit");
    const bound = await waitText(page, "#bound-result", changed);
    await page.click("#closure-submit");
    const closure = await waitText(page, "#closure-result", changed);
    return { bound, closure, actions };
  },

  async "bound-nojs"(page, base) {
    await page.goto(`${base}/actions/bound`);
    await Promise.all([page.waitForNavigation({ timeout: TIMEOUT }), page.click("#closure-submit")]);
    return { closure: await textOf(page, "#closure-result") };
  },

  async "bound-dynamic"(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/bound-dynamic`);
    const rendered = await textOf(page, "#rendered");
    await page.click("#closure-submit");
    const closure = await waitText(page, "#closure-result", changed);
    await page.click("#bound-submit");
    const bound = await waitText(page, "#bound-result", changed);
    return { closureCarriesRender: closure.includes(rendered), bound, actions };
  },

  async redirect(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/redirect`);
    await page.click("#redirect-submit");
    await page.waitForURL("**/actions/redirect/done?from=action", { timeout: TIMEOUT });
    const heading = await textOf(page, "#redirected");
    await page.goto(`${base}/actions/redirect`);
    await page.click("#redirect-dynamic-submit");
    await page.waitForURL("**/books/7", { timeout: TIMEOUT });
    return { heading, dynamic: new URL(page.url()).pathname, actions };
  },

  async "redirect-nojs"(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/redirect`);
    await Promise.all([page.waitForURL("**/actions/redirect/done?from=action", { timeout: TIMEOUT }), page.click("#redirect-submit")]);
    return { heading: await textOf(page, "#redirected"), actions };
  },

  async error(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/error`);
    await page.click("#throw");
    const thrown = await waitText(page, "#throw-result", changed);
    await page.click("#error-submit");
    await page.waitForSelector("#error-boundary", { timeout: TIMEOUT });
    const boundary = { message: await textOf(page, "#error-message"), digest: await textOf(page, "#error-digest") };
    await page.goto(`${base}/actions/error`);
    await page.click("#not-found-submit");
    await page.waitForSelector("#action-not-found", { timeout: TIMEOUT });
    return { thrown: JSON.parse(thrown), boundary, notFound: await textOf(page, "#action-not-found"), actions };
  },

  async "error-nojs"(page, base) {
    await page.goto(`${base}/actions/error`);
    const [response] = await Promise.all([page.waitForNavigation({ timeout: TIMEOUT }), page.click("#error-submit")]);
    return { status: response?.status() ?? null, boundary: await page.locator("#error-boundary").count() };
  },

  async cookies(page, base, context) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/cookies`);
    const before = await textOf(page, "#flavor");
    await page.fill("#flavor-input", "mint");
    await page.click("#cookie-submit");
    const set = await waitText(page, "#flavor", (t) => t === "mint");
    const jar = (await context.cookies()).filter((c) => c.name === "nextrs-flavor").map(({ name, value, httpOnly, sameSite, path }) => ({ name, value, httpOnly, sameSite, path }));
    await page.click("#cookie-clear");
    const cleared = await waitText(page, "#flavor", (t) => t === "unset");
    const after = (await context.cookies()).filter((c) => c.name === "nextrs-flavor").length;
    await page.reload();
    return { before, set, jar, cleared, after, reloaded: await textOf(page, "#flavor"), actions };
  },

  async client(page, base, context) {
    const actions = trackActions(page);
    await context.addCookies([{ name: "nextrs-flavor", value: "vanilla", url: base }]);
    await page.goto(`${base}/actions/client`);
    await page.click("#who");
    const who = JSON.parse(await waitText(page, "#who-result", changed));
    await page.click("#large");
    const large = JSON.parse(await waitText(page, "#large-result", changed));
    return { who, large, actions };
  },

  async concurrent(page, base) {
    const actions = trackActions(page);
    await page.goto(`${base}/actions/concurrent`);
    await page.click("#run-8");
    const run = JSON.parse(await waitText(page, "#concurrent-result", (t) => t.includes("mismatched")));
    await page.click("#mutate-8");
    const mutate = JSON.parse(await waitText(page, "#concurrent-result", (t) => t.includes("recorded")));
    const ledger = await (await fetch(`${upstream}/mutations?key=${encodeURIComponent(mutate.key)}`)).json();
    return { run, recorded: mutate.recorded, ledger: ledger.ids.length, distinct: new Set(ledger.ids).size, actions: actions.length };
  },
};

async function runScenario(browser, name, base) {
  const nojs = name.endsWith("-nojs");
  const context = await browser.newContext({ javaScriptEnabled: !nojs });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(`console: ${message.text()}`);
  });
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
