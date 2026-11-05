// ISR, regenerated page storage and Server Actions on ISR pages, compared
// between `next start` (the oracle) and NextRS.
//
//   node tests/compat/isr.mjs --baseline URL --candidate URL --upstream URL \
//        --project DIR [--isolates N] [--only a,b] [--json out.json]
//
// The ISR fixtures (fixtures/next-app/app/isr, app/api/isr-route) make one
// uncached upstream request per render, so the upstream's hit count for
// their key is the number of times a page was generated. The two servers
// share one upstream, so each scenario runs on the baseline first, waits
// for its background work to settle, then on the candidate, and compares
// what each server did (x-nextjs-cache sequences, whether the served
// generation changed, render counts). Stamps (Date.now() at render) are
// never compared across servers, only within one.
//
// Time-based ISR cannot be tested without waiting: the fixtures revalidate
// after 1-2 s and the scenarios sleep past that, with margins. Everything
// else is ordered by awaiting responses, not by sleeping.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { randomBytes } from "node:crypto";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const { baseline, candidate, upstream, project } = args;
if (!baseline || !candidate || !upstream || !project) {
  console.error("usage: isr.mjs --baseline URL --candidate URL --upstream URL --project DIR [--isolates N] [--only a,b] [--json FILE]");
  process.exit(2);
}
const isolates = Number(args.isolates ?? 1);
const only = args.only ? new Set(args.only.split(",")) : null;
const run = `${Date.now().toString(36)}${randomBytes(2).toString("hex")}`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const manifest = JSON.parse(readFileSync(join(project, ".next/server/server-reference-manifest.json"), "utf8"));
const actionId = (name) => {
  const found = Object.entries(manifest.node).find(([, v]) => v.exportedName === name && v.filename === "app/isr/actions.ts");
  if (!found) throw new Error(`no action ${name}`);
  return found[0];
};
const invalidateId = actionId("isrInvalidate");

async function page(base, path, headers = {}) {
  const response = await fetch(`${base}${path}`, { headers, redirect: "manual" });
  const body = await response.text();
  const stamp = /stamp-(?:<!-- -->)?(\d+)/.exec(body)?.[1] ?? /"stamp":(\d+)/.exec(body)?.[1] ?? null;
  return {
    status: response.status,
    cache: response.headers.get("x-nextjs-cache"),
    cacheControl: response.headers.get("cache-control"),
    type: (response.headers.get("content-type") ?? "").split(";")[0],
    stamp,
    body,
  };
}
const hits = async (key) => (await (await fetch(`${upstream}/hits?key=${encodeURIComponent(key)}`)).json()).hits;
const ledger = async (key) => (await (await fetch(`${upstream}/mutations?key=${encodeURIComponent(key)}`)).json()).ids;
const setFlag = (name, on) => fetch(`${upstream}/flag?name=${name}&on=${on ? 1 : 0}`, { method: "POST" });

// Distinct generations among a list of stamps, in order of appearance:
// "a,a,b" -> [0,0,1]. Comparable across servers.
function generations(stamps) {
  const seen = [];
  return stamps.map((s) => {
    if (s === null) return null;
    if (!seen.includes(s)) seen.push(s);
    return seen.indexOf(s);
  });
}

// Waits until `path` was generated at least `ms` ago (by requesting it
// until a HIT shows a stable stamp), so the next request finds it stale.
async function settle(base, path, ms) {
  let last;
  for (let i = 0; i < 20; i++) {
    const r = await page(base, path);
    if (r.cache === "HIT" && r.stamp === last?.stamp) break;
    last = r;
    await sleep(150);
  }
  await sleep(ms);
}

// The action call React sends for isrInvalidate(formData): multipart,
// the form's fields as argument 1.
async function invalidate(base, path, how, id) {
  const form = new FormData();
  form.append("_1_how", how);
  form.append("_1_id", id);
  form.append("0", '["$K1"]');
  const response = await fetch(`${base}${path}`, {
    method: "POST",
    body: form,
    headers: { origin: base, "next-action": invalidateId, accept: "text/x-component" },
  });
  const text = await response.text();
  return {
    status: response.status,
    type: (response.headers.get("content-type") ?? "").split(";")[0],
    revalidated: response.headers.get("x-action-revalidated"),
    cacheControl: response.headers.get("cache-control"),
    rendered: /stamp-/.test(text),
  };
}

const decodeEntities = (s) => s.replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
function formFields(html, buttonId) {
  const at = html.indexOf(`id="${buttonId}"`);
  const start = html.lastIndexOf("<form", at);
  const end = html.indexOf("</form>", at);
  const fields = [];
  for (const m of html.slice(start, end).matchAll(/<input([^>]*)>/g)) {
    const attrs = Object.fromEntries([...m[1].matchAll(/([a-zA-Z-]+)="([^"]*)"/g)].map(([, k, v]) => [k, decodeEntities(v)]));
    if (attrs.name) fields.push([attrs.name, attrs.value ?? ""]);
  }
  return fields;
}
async function mpa(base, path, fields) {
  const form = new FormData();
  for (const [name, value] of fields) form.append(name, value);
  const response = await fetch(`${base}${path}`, { method: "POST", body: form, headers: { origin: base }, redirect: "manual" });
  const text = await response.text();
  return { status: response.status, type: (response.headers.get("content-type") ?? "").split(";")[0], location: response.headers.get("location"), rendered: /stamp-/.test(text) };
}

// Each scenario: (base, tag) -> observation. Compared unless candidate-only.
const scenarios = {};
const compare = (name, fn) => (scenarios[name] = { kind: "compare", fn });
const candidateOnly = (name, fn) => (scenarios[name] = { kind: "candidate", fn });

// --- Serving and regeneration ------------------------------------------------
compare("fresh-prerender", async (base) => {
  const before = await hits("isr-slug-two");
  const a = await page(base, "/isr/two");
  const b = await page(base, "/isr/two");
  return { statuses: [a.status, b.status], cache: [a.cache, b.cache], cacheControl: a.cacheControl, sameGeneration: a.stamp === b.stamp, renders: (await hits("isr-slug-two")) - before };
});

compare("expired-regenerates-in-background", async (base) => {
  await settle(base, "/isr/timed", 2300);
  const before = await hits("isr-timed");
  const stale = await page(base, "/isr/timed");
  // The background regeneration runs after the stale response.
  await sleep(700);
  const a = await page(base, "/isr/timed");
  const b = await page(base, "/isr/timed");
  return {
    cache: [stale.cache, a.cache, b.cache],
    generations: generations([stale.stamp, a.stamp, b.stamp]),
    cacheControl: stale.cacheControl,
    renders: (await hits("isr-timed")) - before,
  };
});

compare("expired-rsc-request", async (base) => {
  await settle(base, "/isr/timed", 2300);
  // A client navigation's RSC request (no cache-busting check on HEADless
  // fetches of the payload: the router sends _rsc; use the redirect).
  const first = await page(base, "/isr/timed", { rsc: "1" });
  return { status: first.status, location: first.status === 307 };
});

compare("on-demand-params-cached", async (base, tag) => {
  const slug = `p-${run}-${tag}`;
  const a = await page(base, `/isr/${slug}`);
  const b = await page(base, `/isr/${slug}`);
  const c = await page(base, `/isr/${slug}`);
  return { statuses: [a.status, b.status, c.status], cache: [a.cache, b.cache, c.cache], generations: generations([a.stamp, b.stamp, c.stamp]), renders: await hits(`isr-slug-${slug}`) };
});

compare("route-handler-isr", async (base) => {
  await settle(base, "/api/isr-route", 2300);
  const before = await hits("isr-route");
  const stale = await page(base, "/api/isr-route");
  await sleep(700);
  const a = await page(base, "/api/isr-route");
  const b = await page(base, "/api/isr-route");
  return { type: a.type, cache: [stale.cache, a.cache, b.cache], generations: generations([stale.stamp, a.stamp, b.stamp]), cacheControl: a.cacheControl, renders: (await hits("isr-route")) - before };
});

compare("concurrent-requests-on-stale-page", async (base) => {
  await settle(base, "/isr/timed", 2300);
  const before = await hits("isr-timed");
  const responses = await Promise.all(Array.from({ length: 100 }, () => page(base, "/isr/timed")));
  await sleep(1000);
  const after = await page(base, "/isr/timed");
  const renders = (await hits("isr-timed")) - before;
  const stamps = new Set(responses.map((r) => r.stamp));
  return {
    allOk: responses.every((r) => r.status === 200 && r.stamp !== null && r.body.includes("</html>")),
    staleServed: responses.some((r) => r.cache === "STALE"),
    // Concurrent requests share one regeneration per process (next start)
    // and at most one per isolate (NextRS); the count is compared below.
    rendersWithinIsolates: renders >= 1 && renders <= (base === candidate ? isolates : 1),
    distinctGenerationsServed: stamps.size <= 1 + (base === candidate ? isolates : 1),
    settledHit: after.cache,
  };
});

compare("failed-regeneration-keeps-last-good-page", async (base) => {
  // A freshly generated page (its own request must not start a
  // regeneration that would race the switch below).
  await settle(base, "/isr/flaky", 1300);
  await page(base, "/isr/flaky");
  await sleep(700);
  const good = await page(base, "/isr/flaky");
  await setFlag("isr-flaky", true);
  try {
    await sleep(1300);
    const a = await page(base, "/isr/flaky"); // stale; regeneration fails
    await sleep(500);
    const b = await page(base, "/isr/flaky");
    await sleep(1300);
    const c = await page(base, "/isr/flaky");
    await setFlag("isr-flaky", false);
    // Next retries a failed regeneration after max(revalidate, 3) s.
    await sleep(3300);
    const d = await page(base, "/isr/flaky");
    await sleep(700);
    const e = await page(base, "/isr/flaky");
    return {
      statuses: [a.status, b.status, c.status, d.status, e.status],
      generations: generations([good.stamp, a.stamp, b.stamp, c.stamp, d.stamp, e.stamp]),
      recovered: e.stamp !== good.stamp,
    };
  } finally {
    await setFlag("isr-flaky", false);
  }
});

// --- Revalidation from Server Actions ---------------------------------------
for (const how of ["revalidatePath", "updateTag", "revalidateTag", "none"]) {
  compare(`action-${how}-on-isr-page`, async (base, tag) => {
    const id = `${run}-${tag}-${how}`;
    await page(base, "/isr/tagged");
    const s0 = await page(base, "/isr/tagged");
    const before = await hits("isr-tagged");
    const action = await invalidate(base, "/isr/tagged", how, id);
    const a = await page(base, "/isr/tagged");
    await sleep(400);
    const b = await page(base, "/isr/tagged");
    const c = await page(base, "/isr/tagged");
    return {
      action,
      cache: [s0.cache, a.cache, b.cache, c.cache],
      generations: generations([s0.stamp, a.stamp, b.stamp, c.stamp]),
      renders: (await hits("isr-tagged")) - before,
      mutations: (await ledger("isr-mutations")).filter((x) => x === id).length,
    };
  });
}

compare("mpa-form-on-isr-page", async (base, tag) => {
  const html = (await page(base, "/isr/tagged")).body;
  const fields = formFields(html, "isr-revalidatePath").map(([n, v]) => [n, v]);
  fields.push(["id", `${run}-${tag}-mpa`]);
  const before = (await page(base, "/isr/tagged")).stamp;
  const r = await mpa(base, "/isr/tagged", fields);
  const after = await page(base, "/isr/tagged");
  return { ...r, regenerated: after.stamp !== before, mutations: (await ledger("isr-mutations")).filter((x) => x === `${run}-${tag}-mpa`).length };
});

compare("bound-action-on-isr-page", async (base, tag) => {
  const html = (await page(base, "/isr/tagged")).body;
  const fields = formFields(html, "isr-bound");
  fields.push(["id", `${run}-${tag}-bound`]);
  const before = (await page(base, "/isr/tagged")).stamp;
  const r = await mpa(base, "/isr/tagged", fields);
  const after = await page(base, "/isr/tagged");
  return { ...r, regenerated: after.stamp !== before, mutations: (await ledger("isr-mutations")).filter((x) => x === `bound:bound-label:${run}-${tag}-bound`).length };
});

compare("concurrent-mutations-on-isr-page", async (base, tag) => {
  const ids = Array.from({ length: 16 }, (_, i) => `${run}-${tag}-c${i}`);
  const results = await Promise.all(ids.map((id) => invalidate(base, "/isr/tagged", "revalidatePath", id)));
  const recorded = await ledger("isr-mutations");
  const after = await Promise.all(Array.from({ length: 8 }, () => page(base, "/isr/tagged")));
  return {
    statuses: [...new Set(results.map((r) => r.status))],
    eachRecordedOnce: ids.every((id) => recorded.filter((x) => x === id).length === 1),
    afterwardOneGeneration: new Set(after.map((r) => r.stamp)).size === 1,
  };
});

// --- Cross-isolate visibility (NextRS only; next start is one process) -----
candidateOnly("revalidation-visible-in-every-isolate", async (base, tag) => {
  await invalidate(base, "/isr/tagged", "revalidatePath", `${run}-${tag}-x`);
  const fresh = await page(base, "/isr/tagged");
  // Concurrent requests spread over the isolates.
  const all = await Promise.all(Array.from({ length: 48 }, () => page(base, "/isr/tagged")));
  const stamps = new Set(all.map((r) => r.stamp));
  return { pass: stamps.size === 1 && stamps.has(fresh.stamp), distinct: stamps.size };
});

candidateOnly("on-demand-page-shared-by-isolates", async (base, tag) => {
  const slug = `x-${run}-${tag}`;
  const first = await page(base, `/isr/${slug}`);
  const all = await Promise.all(Array.from({ length: 48 }, () => page(base, `/isr/${slug}`)));
  const renders = await hits(`isr-slug-${slug}`);
  return { pass: all.every((r) => r.stamp === first.stamp && r.cache === "HIT") && renders === 1, renders };
});

// --- Privacy: ISR declared, but the route is dynamic ------------------------
compare("cookie-dependent-page-not-shared", async (base, tag) => {
  const users = Array.from({ length: 12 }, (_, i) => `u${i}-${tag}`);
  const responses = await Promise.all(users.map((u) => page(base, "/isr/private", { cookie: `isr-user=${u}` })));
  return {
    cacheControl: responses[0].cacheControl,
    cache: responses[0].cache,
    eachOwn: responses.every((r, i) => r.body.includes(`>${users[i]}<`)),
    anonymous: (await page(base, "/isr/private")).body.includes(">anonymous<"),
  };
});

compare("authorization-dependent-route-not-shared", async (base, tag) => {
  const users = Array.from({ length: 12 }, (_, i) => `Bearer t${i}-${tag}`);
  const responses = await Promise.all(users.map((u) => fetch(`${base}/api/isr-private`, { headers: { authorization: u } }).then(async (r) => ({ cc: r.headers.get("cache-control"), body: await r.json() }))));
  return { cacheControl: responses[0].cc, eachOwn: responses.every((r, i) => r.body.auth === users[i]) };
});

// --- Cache poisoning ---------------------------------------------------------
// Request data that must not select, create or replace a cached entry.
compare("poisoning-query-and-headers", async (base, tag) => {
  await page(base, "/isr/two");
  const reference = await page(base, "/isr/two");
  const before = await hits("isr-slug-two");
  const attempts = [
    ["/isr/two?evil=<script>", {}],
    ["/isr/two", { "x-prerender-revalidate": "guessed-preview-id" }],
    ["/isr/two", { "x-next-revalidated-tags": "isr-slug-two", "x-next-revalidate-tag-token": "guess" }],
    ["/isr/two", { "x-forwarded-host": "evil.example", host: "evil.example" }],
    ["/isr/two", { cookie: `session=${tag}`, authorization: "Bearer x" }],
    ["/isr/two", { "x-nextjs-data": "1", "next-router-prefetch": "1" }],
  ];
  const out = [];
  for (const [path, headers] of attempts) {
    const r = await page(base, path, headers);
    out.push({ status: r.status, cache: r.cache, sameGeneration: r.stamp === reference.stamp, setCookie: r.body.includes("Set-Cookie") });
  }
  const after = await page(base, "/isr/two");
  return { attempts: out, renders: (await hits("isr-slug-two")) - before, unchanged: after.stamp === reference.stamp && !after.body.includes("evil") };
});

compare("rsc-and-html-entries-not-confused", async (base) => {
  const html = await page(base, "/isr/one");
  const rsc = await fetch(`${base}/isr/one`, { headers: { rsc: "1" }, redirect: "manual" });
  const location = rsc.headers.get("location");
  const flight = location ? await fetch(new URL(location, base), { headers: { rsc: "1" } }) : rsc;
  const flightType = (flight.headers.get("content-type") ?? "").split(";")[0];
  const after = await page(base, "/isr/one");
  return { htmlType: html.type, flightType, // next start compresses and adds Accept-Encoding; NextRS does not compress.
    vary: (flight.headers.get("vary") ?? "").replace(/, Accept-Encoding$/, ""), afterType: after.type, afterIsHtml: after.body.startsWith("<!DOCTYPE html>") };
});

// ---------------------------------------------------------------------------
const results = [];
let failures = 0;
const isEqual = (a, b) => JSON.stringify(a) === JSON.stringify(b);
for (const [name, { kind, fn }] of Object.entries(scenarios)) {
  if (only && !only.has(name)) continue;
  let verdict;
  let detail;
  try {
    if (kind === "candidate") {
      const out = await fn(candidate, "c");
      verdict = out.pass ? "PASS" : "FAIL";
      detail = out;
    } else {
      const b = await fn(baseline, "b");
      await sleep(300);
      const c = await fn(candidate, "c");
      verdict = isEqual(b, c) ? "MATCH" : "DIFF";
      detail = verdict === "MATCH" ? b : { baseline: b, candidate: c };
    }
  } catch (error) {
    verdict = "ERROR";
    detail = String(error?.stack ?? error);
  }
  if (verdict === "DIFF" || verdict === "FAIL" || verdict === "ERROR") failures++;
  results.push({ name, verdict, detail });
  console.log(`${verdict.padEnd(6)} ${name.padEnd(44)} ${JSON.stringify(detail)}`);
}
console.log(`${results.filter((r) => r.verdict === "MATCH" || r.verdict === "PASS").length}/${results.length} pass, ${failures} failing (isolates=${isolates})`);
if (args.json) writeFileSync(args.json, JSON.stringify({ run, isolates, results }, null, 2));
process.exit(failures ? 1 : 0);
