// Client navigation in a build made with a deployment ID, in a browser,
// against next start (the baseline) and nextrs (the candidate) serving the
// same build of fixtures/next-app (both with NEXT_DEPLOYMENT_ID):
//
//   node tests/browser/deployment-id.mjs --baseline http://127.0.0.1:3000 \
//     --candidate http://127.0.0.1:3001 --deployment-id ID [--json out.json]
//
// With a deployment ID, Next leaves the build ID out of RSC payloads and the
// client router compares the `x-nextjs-deployment-id` response header with
// its own ID (from <html data-dpl-id>). Scenarios:
// - same deployment: clicking a <Link> is a client navigation (RSC fetch,
//   no new document request), and back/forward stays client-side;
// - skew: the RSC response's header is rewritten in the browser to another
//   deployment's ID, as a newer deployment behind the same URL would
//   answer: the router must fall back to a full page load (Next's
//   doMpaNavigation) instead of applying the foreign payload;
// - a Server Action from the page still runs (it sends x-deployment-id).
// Needs playwright-core (npm ci in tests/browser) and its Chromium.
import { writeFileSync } from "node:fs";
import { chromium } from "playwright-core";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const id = args["deployment-id"];
const targets = { baseline: args.baseline, candidate: args.candidate };
if (!targets.baseline || !targets.candidate || !id) {
  console.error("usage: deployment-id.mjs --baseline URL --candidate URL --deployment-id ID [--json FILE]");
  process.exit(2);
}
const TIMEOUT = 15_000;

// Documents and RSC requests the page made, with what came back.
function track(page) {
  const seen = { documents: [], rsc: [], actions: [] };
  page.on("response", (response) => {
    const request = response.request();
    const path = new URL(request.url()).pathname;
    const headers = request.headers();
    if (request.resourceType() === "document") seen.documents.push(path);
    else if (headers["next-action"]) {
      seen.actions.push({ path, status: response.status(), sentDeployment: headers["x-deployment-id"] ?? null });
    } else if (headers.rsc === "1") {
      seen.rsc.push({
        path,
        status: response.status(),
        sentDeployment: headers["x-deployment-id"] ?? null,
        deployment: response.headers()["x-nextjs-deployment-id"] ?? null,
      });
    }
  });
  return seen;
}

async function sameDeployment(browser, base) {
  const page = await browser.newPage();
  const seen = track(page);
  await page.goto(new URL("/", base).href, { waitUntil: "networkidle" });
  await page.getByRole("link", { name: "Dynamic server component" }).click();
  await page.waitForURL("**/dynamic", { timeout: TIMEOUT });
  await page.locator("main").first().waitFor({ timeout: TIMEOUT });
  await page.goBack();
  await page.waitForURL((url) => url.pathname === "/", { timeout: TIMEOUT });
  await page.getByRole("link", { name: "Book 42" }).click();
  await page.waitForURL("**/books/42", { timeout: TIMEOUT });
  await page.waitForLoadState("networkidle");
  const title = await page.title();
  await page.close();
  return {
    documents: seen.documents,
    // Prefetches and navigations, deduplicated: their number and order
    // depend on the viewport and timing, not on the server.
    rscPaths: [...new Set(seen.rsc.map((r) => r.path))].sort(),
    rscDeployments: [...new Set(seen.rsc.map((r) => r.deployment))],
    sentDeployments: [...new Set(seen.rsc.map((r) => r.sentDeployment))],
    title,
  };
}

async function skew(browser, base) {
  const page = await browser.newPage();
  const seen = track(page);
  await page.goto(new URL("/", base).href, { waitUntil: "networkidle" });
  // From here on, RSC responses claim another deployment.
  await page.route("**/*", async (route) => {
    const request = route.request();
    if (request.headers().rsc !== "1") return route.continue();
    const response = await route.fetch();
    const headers = { ...response.headers(), "x-nextjs-deployment-id": "a-newer-deployment" };
    await route.fulfill({ response, headers });
  });
  await page.getByRole("link", { name: "Dynamic server component" }).click();
  await page.waitForURL("**/dynamic", { timeout: TIMEOUT });
  await page.waitForLoadState("networkidle");
  await page.close();
  return { documents: seen.documents };
}

async function action(browser, base) {
  const page = await browser.newPage();
  const seen = track(page);
  await page.goto(new URL("/actions/form", base).href, { waitUntil: "networkidle" });
  await page.locator("#title").fill("skew");
  const answered = page.waitForResponse((r) => Boolean(r.request().headers()["next-action"]), { timeout: TIMEOUT });
  await page.locator("#submit").click({ timeout: TIMEOUT });
  await answered;
  await page.waitForLoadState("networkidle");
  await page.close();
  return {
    documents: seen.documents,
    actions: seen.actions.map((a) => ({ status: a.status, sentDeployment: a.sentDeployment })),
  };
}

const scenarios = { sameDeployment, skew, action };
const browser = await chromium.launch({ headless: true });
const results = {};
let failed = 0;
try {
  for (const [name, run] of Object.entries(scenarios)) {
    const observed = {};
    for (const [label, base] of Object.entries(targets)) observed[label] = await run(browser, base);
    const same = JSON.stringify(observed.baseline) === JSON.stringify(observed.candidate);
    results[name] = { same, ...observed };
    if (!same) failed++;
    console.log(`${same ? "ok  " : "DIFF"} ${name}${same ? "" : `\n  next start: ${JSON.stringify(observed.baseline)}\n  nextrs:     ${JSON.stringify(observed.candidate)}`}`);
  }
} finally {
  await browser.close();
}

const expectations = [
  ["same deployment: one document load only", results.sameDeployment.candidate.documents.length === 1],
  ["same deployment: RSC responses carry the ID", JSON.stringify(results.sameDeployment.candidate.rscDeployments) === JSON.stringify([id])],
  ["same deployment: the client sends its ID", JSON.stringify(results.sameDeployment.candidate.sentDeployments) === JSON.stringify([id])],
  ["skew: falls back to a full page load", JSON.stringify(results.skew.candidate.documents) === JSON.stringify(["/", "/dynamic"])],
  ["action: ran without a reload", results.action.candidate.actions.length >= 1 && results.action.candidate.documents.length === 1],
  ["action: sends the deployment ID", results.action.candidate.actions.every((a) => a.sentDeployment === id)],
];
for (const [name, ok] of expectations) {
  if (!ok) failed++;
  console.log(`${ok ? "ok  " : "FAIL"} expect: ${name}`);
}
if (args.json) writeFileSync(args.json, JSON.stringify({ deploymentId: id, results, expectations }, null, 2));
process.exit(failed ? 1 : 0);
