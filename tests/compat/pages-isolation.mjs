// Pages Router request isolation: many concurrent requests, each with its
// own identity (cookie), header, route parameter and query marker, through
// getServerSideProps pages, their /_next/data routes and API routes, with
// awaited delays so that requests overlap inside each isolate. Every
// response must carry exactly its own request's values; a mutation POST
// must be recorded exactly once.
//
//   node tests/compat/pages-isolation.mjs --target URL --upstream URL --dist DIR
//        [--requests 1200] [--concurrency 64] [--aborts 20] [--json out.json]
//        [--base-path /base]
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { parseArgs } from "./routing-lib.mjs";

const args = parseArgs();
const target = args.target;
const upstream = args.upstream;
const total = Number(args.requests ?? 1200);
const concurrency = Number(args.concurrency ?? 64);
const aborts = Number(args.aborts ?? 20);
const prefix = args["base-path"] ?? "";
const buildId = readFileSync(path.join(args.dist, "BUILD_ID"), "utf8").trim();
const ns = `iso${Date.now().toString(36)}`;
// Test-only corruption (see routing-lib.mjs): swaps identities in what was
// observed, which the checks below must report.
const control = process.env.NEXTRS_NEGATIVE_CONTROL ?? "";

const failures = [];
const counts = { ssr: 0, data: 0, api: 0, mutate: 0, aborted: 0 };

function check(kind, i, ok, detail) {
  counts[kind]++;
  if (!ok && failures.length < 50) failures.push({ kind, i, detail });
  else if (!ok) failures.push(null);
}

const delay = (i) => (i * 7) % 40;
const user = (i) => `u-${ns}-${i}`;

async function one(i) {
  const kind = ["ssr", "data", "api", "ssr-dyn"][i % 4];
  const headers = { cookie: `user=${user(i)}`, "x-test-user": `h-${i}` };
  if (kind === "ssr") {
    const r = await fetch(`${target}${prefix}/ssr/whoami?marker=m-${i}&ms=${delay(i)}`, { headers });
    let html = await r.text();
    if (control === "identity" && i % 7 === 0) html = html.replaceAll(user(i), user(i + 1));
    const ok = r.status === 200 && html.includes(`id="slug">${user(i)}<`) && html.includes(`secret-of-${user(i)}`) && html.includes(`id="info">m-${i}<`) && !html.includes(user(i + 1)) && !html.includes(user(i - 1));
    check("ssr", i, ok, ok ? null : { status: r.status, html: html.slice(0, 300) });
  } else if (kind === "data") {
    const r = await fetch(`${target}${prefix}/_next/data/${buildId}/ssr/whoami.json?marker=m-${i}&ms=${delay(i)}`, { headers });
    let body = await r.text();
    if (control === "identity" && i % 7 === 0) body = body.replaceAll(user(i), user(i + 1));
    let props = {};
    try {
      props = JSON.parse(body).pageProps ?? {};
    } catch {}
    const ok = r.status === 200 && props.user === user(i) && props.secret === `secret-of-${user(i)}` && props.marker === `m-${i}`;
    check("data", i, ok, ok ? null : { status: r.status, body: body.slice(0, 300) });
  } else if (kind === "api") {
    const r = await fetch(`${target}${prefix}/api/echo?i=${i}`, {
      method: "POST",
      headers: { ...headers, "content-type": "application/json" },
      body: JSON.stringify({ i, who: user(i) }),
    });
    let body = await r.text();
    if (control === "identity" && i % 7 === 0) body = body.replaceAll(user(i), user(i + 1));
    let echo = {};
    try {
      echo = JSON.parse(body);
    } catch {}
    const ok = r.status === 200 && echo.query?.i === String(i) && echo.body?.who === user(i) && echo.cookies?.user === user(i) && echo.user === `h-${i}`;
    check("api", i, ok, ok ? null : { status: r.status, body: body.slice(0, 300) });
  } else {
    const r = await fetch(`${target}${prefix}/books/${i}?q=${i}`, { headers });
    let html = await r.text();
    if (control === "identity" && i % 7 === 0) html = html.replaceAll(`user=${user(i)}`, `user=${user(i + 1)}`);
    const ok = r.status === 200 && html.includes(`id="slug">${i}<`) && html.includes(`user=${user(i)} header=h-${i}`);
    check("ssr", i, ok, ok ? null : { status: r.status, html: html.slice(0, 300) });
  }
}

// Clients that leave before getServerSideProps finishes: the isolate must
// keep serving the others.
async function abandoned(i) {
  const controller = new AbortController();
  const pending = fetch(`${target}${prefix}/ssr/slow?id=gone-${i}&ms=2000`, { signal: controller.signal }).catch(() => null);
  setTimeout(() => controller.abort(), 50 + (i % 5) * 20);
  await pending;
  counts.aborted++;
}

// Mutations: each POST must be recorded exactly once.
async function mutate(i) {
  const key = `${ns}-mut-${i}`;
  const r = await fetch(`${target}${prefix}/api/mutate?key=${key}&id=${i}&ms=${delay(i)}`, { method: "POST" });
  await r.text();
  const ledger = await fetch(`${upstream}/mutations?key=${key}`).then((x) => x.json());
  let ids = ledger.ids;
  if (control === "ledger-twice" && i % 10 === 0) ids = [...ids, ...ids];
  const ok = r.status === 200 && JSON.stringify(ids) === JSON.stringify([String(i)]);
  check("mutate", i, ok, ok ? null : { status: r.status, ids });
}

const started = Date.now();
let next = 0;
async function worker() {
  while (next < total) {
    const i = next++;
    try {
      if (i % 25 === 0) await mutate(i);
      else await one(i);
    } catch (error) {
      check("ssr", i, false, String(error));
    }
  }
}
await Promise.all([...Array.from({ length: concurrency }, worker), ...Array.from({ length: aborts }, (_, i) => abandoned(i))]);
// After the aborts: the server still answers.
const health = await fetch(`${target}${prefix}/api/health`).then((r) => r.status).catch(() => 0);

const failed = failures.length;
const summary = { total, concurrency, aborts, counts, failed, health, seconds: (Date.now() - started) / 1000, negativeControl: control || null };
console.log(JSON.stringify(summary));
for (const f of failures.filter(Boolean).slice(0, 5)) console.log("FAIL", JSON.stringify(f).slice(0, 400));
if (args.json) writeFileSync(args.json, JSON.stringify({ summary, failures: failures.filter(Boolean) }, null, 2));
process.exit(failed === 0 && health === 200 ? 0 : 1);
