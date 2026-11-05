// Shared by tests/compat/proxy.mjs and tests/compat/routing-config.mjs:
// one request sent to next start (the oracle) and to NextRS, and what of
// the answers is compared.
//
// Normalized away, because they differ for reasons unrelated to routing:
// transport headers (date, connection, transfer/content encoding and
// length: next start compresses), validators (etag, last-modified),
// `Accept-Encoding` in `Vary` (compression again), next start's duplicate
// `x-nextjs-prerender`, the servers' own origin in URLs (they listen on
// different ports, and next start for these suites on 0.0.0.0), Date.now()
// stamps and the build id inside RSC payloads. Never normalized: status,
// Location, Set-Cookie, every other header name and value, and the
// request headers and cookies the route reports it saw.
import { writeFileSync } from "node:fs";
import http from "node:http";

export function parseArgs() {
  return Object.fromEntries(
    process.argv.slice(2).reduce((pairs, arg, i, all) => {
      if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]?.startsWith("--") ? "1" : all[i + 1]]);
      return pairs;
    }, []),
  );
}

const SKIP = new Set([
  "date", "connection", "keep-alive", "transfer-encoding", "content-encoding",
  "content-length", "etag", "last-modified", "accept-ranges",
]);
const SELF = /\bhttps?:\/\/(?:localhost|0\.0\.0\.0|127\.0\.0\.1|\[::1\]):\d+/g;
const self = (text) => text.replace(SELF, "http://SELF");

const decodeEntities = (s) =>
  s.replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");

// What a page shows: the fixture's marked elements (React may insert
// `<!-- -->` between text parts).
function htmlSummary(html) {
  const out = {};
  for (const id of ["title", "slug", "kind", "secret", "query", "info", "result"]) {
    const m = new RegExp(`id="${id}">([\\s\\S]*?)</`).exec(html);
    if (m) out[id] = decodeEntities(m[1].replace(/<!-- -->/g, ""));
  }
  out.stamp = /stamp-(?:<!-- -->)?\d+/.test(html) ? "present" : "absent";
  return out;
}

// A request whose target is sent verbatim (fetch would normalize dot
// segments and encodings before sending).
function rawFetch(base, { method = "GET", path, headers = {} }) {
  return new Promise((resolve, reject) => {
    const url = new URL(base);
    const req = http.request({ host: url.hostname, port: url.port, method, path, headers, agent: false }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => {
        const h = new Headers();
        for (let i = 0; i < res.rawHeaders.length; i += 2) h.append(res.rawHeaders[i], res.rawHeaders[i + 1]);
        resolve(new Response(res.statusCode === 204 || res.statusCode === 304 ? null : Buffer.concat(chunks), { status: res.statusCode, headers: h }));
      });
    });
    req.on("error", reject);
    req.end();
  });
}

export async function observe(base, request, { redirect = "manual", signal } = {}) {
  const { method = "GET", path, headers = {}, body } = request;
  let response;
  try {
    response = request.raw
      ? await rawFetch(base, request)
      : await fetch(`${base}${path}`, { method, headers, body, redirect, signal });
  } catch (error) {
    return { error: String(error?.cause?.code ?? error?.message ?? error) };
  }
  const text = await response.text().catch((error) => `<<body error ${error?.message}>>`);
  const h = {};
  const port = new URL(base).port;
  for (const [name, value] of response.headers) {
    if (SKIP.has(name) || name === "set-cookie") continue;
    // The server's own host:port (a reflected Host, X-Forwarded-*).
    let v = self(value).replace(new RegExp(`^(127\\.0\\.0\\.1|localhost):${port}$`), "SELF-HOST");
    if (name === "x-forwarded-port" && v === port) v = "SELF-PORT";
    if (name === "vary") {
      v = v.split(/,\s*/).filter((t) => t.toLowerCase() !== "accept-encoding").join(", ");
      if (!v) continue;
    }
    if (name === "x-nextjs-prerender") v = [...new Set(v.split(/,\s*/))].join(", ");
    h[name] = v;
  }
  const type = (response.headers.get("content-type") ?? "").split(";")[0];
  let shown;
  if (type === "text/html") shown = htmlSummary(text);
  else if (type === "text/x-component") {
    // An RSC payload: the action's returned object, or whether it rendered.
    const value = /\{"proxyRequest"[^}]*\}|\{"recorded"[^}]*\}|\{"revalidated"[^}]*\}/.exec(text)?.[0] ?? null;
    shown = { rsc: true, value, stamp: /stamp-\d+/.test(text) ? "present" : "absent" };
  } else shown = self(text);
  return {
    status: response.status,
    headers: h,
    setCookie: response.headers.getSetCookie().map(self),
    body: shown,
  };
}

// Field-by-field differences between two observations.
export function differences(a, b) {
  const out = [];
  const walk = (x, y, at) => {
    if (JSON.stringify(x) === JSON.stringify(y)) return;
    if (x && y && typeof x === "object" && typeof y === "object" && !Array.isArray(x)) {
      for (const k of new Set([...Object.keys(x), ...Object.keys(y)])) walk(x[k], y[k], `${at}.${k}`);
      return;
    }
    out.push(`${at}: ${JSON.stringify(x)?.slice(0, 300)} | ${JSON.stringify(y)?.slice(0, 300)}`);
  };
  walk(a, b, "$");
  return out;
}

// Runs `scenarios` ({ name: { kind, run(base) -> observation, reason } }).
// kind "compare": both servers must observe the same. "known-diff": the
// difference is documented (`reason`) and only reported. "candidate": a
// NextRS-only check whose `run` returns { ok, detail }.
export async function runScenarios(title, scenarios, { baseline, candidate, json, only }) {
  const results = [];
  let failures = 0;
  for (const [name, scenario] of Object.entries(scenarios)) {
    if (only && !only.has(name)) continue;
    let verdict;
    let detail;
    try {
      if (scenario.kind === "candidate") {
        const r = await scenario.run(candidate);
        verdict = r.ok ? "PASS" : "FAIL";
        detail = r.detail;
      } else {
        const a = await scenario.run(baseline, "baseline");
        const b = await scenario.run(candidate, "candidate");
        const diffs = differences(a, b);
        if (scenario.kind === "known-diff") {
          verdict = diffs.length ? "KNOWN-DIFF" : "MATCH";
          detail = { reason: scenario.reason, diffs, baseline: a, candidate: b };
        } else {
          verdict = diffs.length ? "DIFF" : "MATCH";
          detail = diffs.length ? { diffs, baseline: a, candidate: b } : { observed: a };
        }
      }
    } catch (error) {
      verdict = "ERROR";
      detail = String(error?.stack ?? error);
    }
    if (verdict === "DIFF" || verdict === "FAIL" || verdict === "ERROR") failures++;
    results.push({ name, verdict, detail });
    console.log(`${verdict.padEnd(10)} ${name}`);
    if (verdict === "DIFF") for (const d of detail.diffs) console.log(`    ${d}`);
    if (verdict === "FAIL" || verdict === "ERROR") console.log(`    ${JSON.stringify(detail).slice(0, 1200)}`);
    if (verdict === "KNOWN-DIFF") console.log(`    (${scenario.reason})`);
  }
  const count = (v) => results.filter((r) => r.verdict === v).length;
  const summary = {
    title,
    total: results.length,
    match: count("MATCH"),
    pass: count("PASS"),
    knownDiff: count("KNOWN-DIFF"),
    diff: count("DIFF"),
    fail: count("FAIL"),
    error: count("ERROR"),
  };
  console.log(
    `${title}: ${summary.match} MATCH, ${summary.pass} PASS, ${summary.knownDiff} KNOWN-DIFF, ${summary.diff} DIFF, ${summary.fail} FAIL, ${summary.error} ERROR`,
  );
  if (json) writeFileSync(json, JSON.stringify({ summary, results }, null, 2));
  return failures;
}

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
