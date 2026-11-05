// Cache Components ("use cache", cacheLife, cacheTag) compared between
// `next start` and NextRS, on fixtures/next-cache-components.
//
//   node tests/compat/cache-components.mjs --baseline URL --candidate URL \
//        --upstream URL --project DIR [--isolates N] [--phase run|prepare|verify]
//        [--prepared FILE] [--json FILE] [--only a,b]
//
// The cached functions (app/data.ts) make one upstream request per
// execution, keyed by their argument; every scenario uses arguments unique
// to the server and run, so upstream hit counts are each server's own
// executions. `at` (Date.now() inside the cached function) identifies one
// cached result; it is only compared within a server.
//
// --phase prepare/verify: an entry created before a restart, read after it
// (reported: next start's default handler keeps entries in memory only).
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
const isolates = Number(args.isolates ?? 1);
const phase = args.phase ?? "run";
const only = args.only ? new Set(args.only.split(",")) : null;
const run = args.ns ?? `${Date.now().toString(36)}${randomBytes(2).toString("hex")}`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const manifest = JSON.parse(readFileSync(join(project, ".next/server/server-reference-manifest.json"), "utf8"));
const invalidateId = Object.entries(manifest.node).find(([, v]) => v.exportedName === "invalidateBooks")?.[0];

async function read(base, path) {
  const response = await fetch(`${base}${path}`);
  const body = await response.text();
  // The streamed value appears in HTML and in the Flight data; take the
  // last occurrence (the resolved one).
  const all = [...body.matchAll(/(?:shelf|key)=([\w-]+) hit=(\d+) at=(\d+)/g)];
  const m = all.at(-1);
  return { status: response.status, hit: m ? Number(m[2]) : null, at: m?.[3] ?? null };
}
const books = (base, shelf) => read(base, `/cached?shelf=${shelf}`);
const short = (base, key) => read(base, `/profile?key=${key}`);
const hits = async (key) => (await (await fetch(`${upstream}/hits?key=${encodeURIComponent(key)}`)).json()).hits;
function generations(values) {
  const seen = [];
  return values.map((v) => {
    if (!seen.includes(v)) seen.push(v);
    return seen.indexOf(v);
  });
}
async function invalidate(base, how, tag) {
  const form = new FormData();
  form.append("_1_how", how);
  form.append("_1_tag", tag);
  form.append("0", '["$K1"]');
  const r = await fetch(`${base}/action`, { method: "POST", body: form, headers: { origin: base, "next-action": invalidateId, accept: "text/x-component" } });
  await r.text();
  return { status: r.status, revalidated: r.headers.get("x-action-revalidated") };
}

const scenarios = {};
const compare = (name, fn) => (scenarios[name] = { kind: "compare", fn });
const candidateOnly = (name, fn) => (scenarios[name] = { kind: "candidate", fn });

compare("first-call-then-cached", async (base, tag) => {
  const shelf = `${run}-${tag}-a`;
  const r = [await books(base, shelf), await books(base, shelf), await books(base, shelf)];
  return { statuses: r.map((x) => x.status), generations: generations(r.map((x) => x.at)), executions: await hits(`cc-books-${shelf}`) };
});

compare("arguments-are-keys", async (base, tag) => {
  const a = await books(base, `${run}-${tag}-k1`);
  const b = await books(base, `${run}-${tag}-k2`);
  const a2 = await books(base, `${run}-${tag}-k1`);
  return { distinct: a.at !== b.at, aCached: a.at === a2.at, executions: (await hits(`cc-books-${run}-${tag}-k1`)) + (await hits(`cc-books-${run}-${tag}-k2`)) };
});

compare("cache-life-expiry", async (base, tag) => {
  const key = `${run}-${tag}-s`;
  const a = await short(base, key);
  const b = await short(base, key);
  await sleep(2400); // past revalidate (2 s): production drops the entry
  const c = await short(base, key);
  const d = await short(base, key);
  return { generations: generations([a.at, b.at, c.at, d.at]), executions: await hits(`cc-short-${key}`) };
});

compare("updateTag-from-action", async (base, tag) => {
  const shelf = `${run}-${tag}-u`;
  const a = await books(base, shelf);
  const action = await invalidate(base, "updateTag", `shelf-${shelf}`);
  const b = await books(base, shelf);
  const c = await books(base, shelf);
  return { action, generations: generations([a.at, b.at, c.at]), executions: await hits(`cc-books-${shelf}`) };
});

compare("revalidateTag-max-from-action", async (base, tag) => {
  const shelf = `${run}-${tag}-r`;
  const a = await books(base, shelf);
  const action = await invalidate(base, "revalidateTag", `shelf-${shelf}`);
  const b = await books(base, shelf);
  await sleep(500);
  const c = await books(base, shelf);
  const d = await books(base, shelf);
  return { action, generations: generations([a.at, b.at, c.at, d.at]), executions: await hits(`cc-books-${shelf}`) };
});

compare("unrelated-tag-untouched", async (base, tag) => {
  const shelf = `${run}-${tag}-n`;
  const a = await books(base, shelf);
  await invalidate(base, "updateTag", `shelf-${run}-${tag}-other`);
  const b = await books(base, shelf);
  return { same: a.at === b.at, executions: await hits(`cc-books-${shelf}`) };
});

compare("concurrent-first-calls-agree-afterwards", async (base, tag) => {
  const shelf = `${run}-${tag}-c`;
  const all = await Promise.all(Array.from({ length: 24 }, () => books(base, shelf)));
  await sleep(300);
  const after = await Promise.all(Array.from({ length: 8 }, () => books(base, shelf)));
  return {
    allOk: all.every((r) => r.status === 200 && r.at !== null),
    afterwardOneValue: new Set(after.map((r) => r.at)).size === 1,
  };
});

candidateOnly("shared-by-isolates", async (base, tag) => {
  const shelf = `${run}-${tag}-x`;
  const first = await books(base, shelf);
  const all = await Promise.all(Array.from({ length: 48 }, () => books(base, shelf)));
  const executions = await hits(`cc-books-${shelf}`);
  return { pass: all.every((r) => r.at === first.at) && executions === 1, executions };
});

candidateOnly("tag-revalidation-reaches-every-isolate", async (base, tag) => {
  const shelf = `${run}-${tag}-y`;
  await books(base, shelf);
  await invalidate(base, "updateTag", `shelf-${shelf}`);
  const fresh = await books(base, shelf);
  const all = await Promise.all(Array.from({ length: 48 }, () => books(base, shelf)));
  return { pass: all.every((r) => r.at === fresh.at), distinct: new Set(all.map((r) => r.at)).size };
});

if (phase !== "run") {
  const shelf = (which) => `${run}-${which}-restart`;
  const observed = {};
  for (const [which, base] of [["b", baseline], ["c", candidate]]) observed[which] = await books(base, shelf(which));
  if (phase === "prepare") {
    if (args.json) writeFileSync(args.json, JSON.stringify(observed));
    console.log(JSON.stringify(observed));
    process.exit(0);
  }
  const prepared = JSON.parse(readFileSync(args.prepared, "utf8"));
  const kept = (w) => observed[w].at === prepared[w].at;
  console.log(`REPORT use-cache entry across restart: next start kept=${kept("b")} nextrs kept=${kept("c")} (next start's default handler is memory-only; NextRS keeps it with --cache-dir)`);
  if (args.json) writeFileSync(args.json, JSON.stringify({ prepared, observed, next: kept("b"), nextrs: kept("c") }, null, 2));
  process.exit(0);
}

const results = [];
let failures = 0;
for (const [name, { kind, fn }] of Object.entries(scenarios)) {
  if (only && !only.has(name)) continue;
  let verdict;
  let detail;
  try {
    if (kind === "candidate") {
      detail = await fn(candidate, "c");
      verdict = detail.pass ? "PASS" : "FAIL";
    } else {
      const b = await fn(baseline, "b");
      const c = await fn(candidate, "c");
      verdict = JSON.stringify(b) === JSON.stringify(c) ? "MATCH" : "DIFF";
      detail = verdict === "MATCH" ? b : { baseline: b, candidate: c };
    }
  } catch (error) {
    verdict = "ERROR";
    detail = String(error?.stack ?? error);
  }
  if (!["MATCH", "PASS"].includes(verdict)) failures++;
  results.push({ name, verdict, detail });
  console.log(`${verdict.padEnd(6)} ${name.padEnd(42)} ${JSON.stringify(detail)}`);
}
console.log(`${results.length - failures}/${results.length} pass, ${failures} failing (isolates=${isolates})`);
if (args.json) writeFileSync(args.json, JSON.stringify({ run, isolates, results }, null, 2));
process.exit(failures ? 1 : 0);
