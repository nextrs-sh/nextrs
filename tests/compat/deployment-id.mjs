// Deployment ID behaviour, compared between next start (the baseline) and
// nextrs (the candidate) serving the same build of fixtures/next-app made
// with NEXT_DEPLOYMENT_ID (next start started with the same variable):
//
//   node tests/compat/deployment-id.mjs --baseline http://127.0.0.1:3000 \
//     --candidate http://127.0.0.1:3001 --deployment-id ID [--json out.json]
//
// What is compared, per path and request kind (document, RSC, RSC
// prefetch, HEAD, a client claiming another deployment):
// - status, Location, and x-nextjs-deployment-id (routes-manifest
//   onMatchHeaders, or Next's own handler for RSC requests);
// - for HTML, the <html data-dpl-id> attribute and the ?dpl= asset query;
// - for RSC payloads, the ?dpl= chunk references and the absence of the
//   build ID (`"b":`), which Next leaves out when a deployment ID is set.
// Static assets serve whatever ?dpl= they carry (next start validates it
// only in Next's own test mode), and an unknown Server Action from another
// deployment gets the same answer from both servers.
import { writeFileSync } from "node:fs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const { baseline, candidate } = args;
const id = args["deployment-id"];
if (!baseline || !candidate || !id) {
  console.error("usage: deployment-id.mjs --baseline URL --candidate URL --deployment-id ID [--json FILE]");
  process.exit(2);
}
const HEADER = "x-nextjs-deployment-id";

async function fetchOnce(base, path, { method = "GET", headers = {} } = {}) {
  const response = await fetch(new URL(path, base), { method, headers, redirect: "manual" });
  const body = method === "HEAD" ? "" : await response.text();
  return {
    status: response.status,
    location: response.headers.get("location"),
    deployment: response.headers.get(HEADER),
    type: (response.headers.get("content-type") ?? "").split(";")[0],
    body,
  };
}

// An RSC request as the client router makes it: follow the `_rsc`
// cache-busting redirect next start answers first (same origin only).
async function rsc(base, path, extra = {}) {
  const headers = { rsc: "1", ...extra };
  const first = await fetchOnce(base, path, { headers });
  if (first.status !== 307 || !first.location?.includes("_rsc")) return { first, final: first };
  const final = await fetchOnce(base, first.location, { headers });
  return { first, final };
}

function htmlFacts(body) {
  return {
    dplAttribute: (body.match(/<html[^>]*data-dpl-id="([^"]*)"/) ?? [])[1] ?? null,
    // Inline flight data escapes its quotes (`\"`): stop at a backslash.
    assetQueries: [...new Set([...body.matchAll(/\/_next\/static\/[^"'\s\\]*?\?dpl=([^"'&\s\\]*)/g)].map((m) => m[1]))],
  };
}

function rscFacts(body) {
  return {
    buildIdInPayload: /^0:\{[^\n]*"b":"/m.test(body),
    chunkQueries: [...new Set([...body.matchAll(/\/_next\/static\/[^"\s\\]*?\?dpl=([^"&\s\\]*)/g)].map((m) => m[1]))],
  };
}

const pages = ["/", "/dynamic", "/books/42", "/request-info", "/gsp/alpha", "/isr/timed", "/actions/form", "/data-fetch"];
const unmatched = ["/no-such-page", "/a/b/c"];

const checks = [];
function check(name, observe) {
  checks.push({ name, observe });
}

for (const path of [...pages, ...unmatched]) {
  check(`document ${path}`, async (base) => {
    const r = await fetchOnce(base, path);
    return { status: r.status, deployment: r.deployment, type: r.type, ...(r.type === "text/html" ? htmlFacts(r.body) : {}) };
  });
  check(`rsc ${path}`, async (base) => {
    const { first, final } = await rsc(base, path);
    return {
      firstStatus: first.status,
      firstDeployment: first.deployment,
      status: final.status,
      deployment: final.deployment,
      type: final.type,
      ...(final.type === "text/x-component" ? rscFacts(final.body) : {}),
    };
  });
  check(`rsc prefetch ${path}`, async (base) => {
    const { first, final } = await rsc(base, path, { "next-router-prefetch": "1" });
    return { firstStatus: first.status, firstDeployment: first.deployment, status: final.status, deployment: final.deployment };
  });
  check(`rsc from another deployment ${path}`, async (base) => {
    // The client sends its own deployment ID; next start does not act on it
    // (a platform's skew protection would route by it).
    const { final } = await rsc(base, path, { "x-deployment-id": "an-older-deployment" });
    return { status: final.status, deployment: final.deployment };
  });
  check(`head ${path}`, async (base) => {
    const r = await fetchOnce(base, path, { method: "HEAD" });
    return { status: r.status, deployment: r.deployment };
  });
}

check("rsc value must be exactly 1", async (base) => {
  const out = {};
  for (const value of ["0", "2", "true"]) {
    const r = await fetchOnce(base, "/dynamic", { headers: { rsc: value } });
    out[value] = { status: r.status, deployment: r.deployment, type: r.type };
  }
  return out;
});

check("static asset with any dpl", async (base) => {
  const home = await fetchOnce(base, "/");
  const asset = (home.body.match(/\/_next\/static\/chunks\/[^"?]*\.js/) ?? [])[0];
  if (!asset) throw new Error("no chunk reference on /");
  const out = {};
  for (const query of ["", `?dpl=${id}`, "?dpl=an-older-deployment"]) {
    const r = await fetchOnce(base, `${asset}${query}`);
    out[query || "none"] = { status: r.status, deployment: r.deployment, length: r.body.length };
  }
  const withRsc = await fetchOnce(base, asset, { headers: { rsc: "1" } });
  out.rsc = { status: withRsc.status, deployment: withRsc.deployment };
  return out;
});

check("next data path", async (base) => {
  const r = await fetchOnce(base, "/_next/data/whatever/index.json");
  return { status: r.status, deployment: r.deployment };
});

check("server action from another deployment", async (base) => {
  // An action ID this build does not have, as an older client would send.
  const r = await fetchOnce(base, "/actions/form", {
    method: "POST",
    headers: {
      "next-action": "0".repeat(42),
      "x-deployment-id": "an-older-deployment",
      accept: "text/x-component",
      "content-type": "text/plain;charset=UTF-8",
      origin: base,
    },
  });
  return { status: r.status, deployment: r.deployment, type: r.type };
});

const results = [];
let failed = 0;
for (const { name, observe } of checks) {
  let baselineResult, candidateResult, error = null;
  try {
    baselineResult = await observe(baseline);
    candidateResult = await observe(candidate);
  } catch (e) {
    error = String(e?.stack ?? e);
  }
  const same = !error && JSON.stringify(baselineResult) === JSON.stringify(candidateResult);
  if (!same) failed++;
  results.push({ name, same, baseline: baselineResult, candidate: candidateResult, error });
  console.log(`${same ? "ok  " : "DIFF"} ${name}${same ? "" : `\n  next start: ${JSON.stringify(baselineResult)}\n  nextrs:     ${JSON.stringify(candidateResult)}${error ? `\n  ${error}` : ""}`}`);
}

// Facts the comparison alone would not catch (both servers wrong alike).
const expectations = [];
const expect = (name, ok) => {
  expectations.push({ name, ok });
  if (!ok) failed++;
  console.log(`${ok ? "ok  " : "FAIL"} expect: ${name}`);
};
const byName = Object.fromEntries(results.map((r) => [r.name, r.candidate]));
expect("RSC responses carry the deployment ID", pages.every((p) => byName[`rsc ${p}`]?.deployment === id));
expect("RSC payloads have no build ID", pages.every((p) => byName[`rsc ${p}`]?.buildIdInPayload === false));
expect("documents carry data-dpl-id", pages.every((p) => byName[`document ${p}`]?.dplAttribute === id));
expect("documents have no deployment header", pages.every((p) => byName[`document ${p}`]?.deployment === null));
expect(
  "asset URLs carry ?dpl=<id>",
  pages.every((p) => JSON.stringify(byName[`document ${p}`]?.assetQueries) === JSON.stringify([id])),
);

console.log(`${checks.length - results.filter((r) => !r.same).length}/${checks.length} comparisons equal, ${expectations.filter((e) => e.ok).length}/${expectations.length} expectations met`);
if (args.json) writeFileSync(args.json, JSON.stringify({ deploymentId: id, results, expectations }, null, 2));
process.exit(failed ? 1 : 0);
