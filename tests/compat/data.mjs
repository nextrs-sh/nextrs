// Next.js server-side fetch behaviour, compared between `next start` (the
// oracle) and NextRS. Both fetch from tests/compat/upstream.mjs, which
// counts hits per key; each server uses its own key namespace. A scenario
// passes when both servers produce the same observable sequence (statuses,
// upstream hit numbers, error codes); a few NextRS-only checks (identity
// after fetch, upstream aborts) are reported separately.
//
//   node tests/compat/data.mjs --baseline http://127.0.0.1:3000 \
//        --candidate http://127.0.0.1:3001 --upstream http://127.0.0.1:4999 \
//        --closed-port 4998 [--only name,name] [--json out.json]
//
// --closed-port: a port on 127.0.0.1 nothing listens on (and that NextRS's
// network policy allows), for the connection-refused scenario.
import http from "node:http";
import { writeFileSync } from "node:fs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const servers = { baseline: args.baseline, candidate: args.candidate };
const upstreamBase = args.upstream;
const closedPort = args["closed-port"];
const only = args.only ? new Set(args.only.split(",")) : null;
const run = Date.now().toString(36);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const agent = new http.Agent({ keepAlive: true, maxSockets: 64 });

function get(base, path, { headers = {}, abortAfterMs } = {}) {
  return new Promise((resolve) => {
    const url = new URL(path, base);
    const req = http.get(url, { headers, agent }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString() }));
      res.on("error", () => resolve({ status: res.statusCode, error: "aborted" }));
    });
    req.on("error", (e) => resolve({ error: e.code ?? String(e) }));
    if (abortAfterMs) setTimeout(() => req.destroy(), abortAfterMs);
  });
}

const json = (r) => {
  try {
    return JSON.parse(r.body);
  } catch {
    return null;
  }
};
const api = async (base, query, headers) => {
  const r = await get(base, `/api/data?${query}`, { headers });
  const body = json(r);
  return { status: r.status, upstreamStatus: body?.status, hit: body?.body?.hit ?? null, auth: body?.body?.auth, code: body?.code ?? null };
};
const pageHit = (r) => {
  const m = /hit=(?:<!-- -->)?(\d+)/.exec(r.body ?? "");
  return m ? Number(m[1]) : null;
};
const hits = async (key) => json(await get(upstreamBase, `/hits?key=${encodeURIComponent(key)}`));

// Each scenario gets (base, ns) and returns an observation to compare.
const scenarios = {
  // 1. No cache options: Next 15+ does not cache; every request fetches.
  async "no-cache"(base, ns) {
    const out = [];
    for (let i = 0; i < 3; i++) out.push((await api(base, `key=${ns}-plain`)).hit);
    return out;
  },
  // 2. cache: 'force-cache' is served from the data cache after the first.
  async "force-cache"(base, ns) {
    const out = [];
    for (let i = 0; i < 3; i++) out.push((await api(base, `key=${ns}-forced&cache=force-cache`)).hit);
    return { responses: out, upstreamHits: (await hits(`${ns}-forced`)).hits };
  },
  // 3. Distinct URLs are distinct cache entries.
  async "distinct-keys"(base, ns) {
    const out = [];
    for (let round = 0; round < 2; round++) {
      for (const k of ["a", "b", "c"]) out.push((await api(base, `key=${ns}-k${k}&cache=force-cache`)).hit);
    }
    return out;
  },
  // 4. cache: 'no-store' bypasses the data cache.
  async "no-store"(base, ns) {
    const out = [];
    for (let i = 0; i < 3; i++) out.push((await api(base, `key=${ns}-nostore&cache=no-store`)).hit);
    return out;
  },
  // 5. Time-based revalidation: stale-while-revalidate after 2 s.
  async expiration(base, ns) {
    const q = `key=${ns}-exp&revalidate=2`;
    const out = [(await api(base, q)).hit, (await api(base, q)).hit];
    await sleep(2600);
    out.push((await api(base, q)).hit); // stale, refreshes in the background
    await sleep(800);
    out.push((await api(base, q)).hit);
    return out;
  },
  // 6. Tag revalidation expires the entry at once.
  async "tag-revalidation"(base, ns) {
    const q = `key=${ns}-tag&revalidate=3600&tag=${ns}-t`;
    const out = [(await api(base, q)).hit, (await api(base, q)).hit];
    await get(base, `/api/revalidate?tag=${ns}-t`);
    out.push((await api(base, q)).hit, (await api(base, q)).hit);
    // A different tag leaves it alone.
    await get(base, `/api/revalidate?tag=${ns}-other`);
    out.push((await api(base, q)).hit);
    return out;
  },
  // 7. Failed upstream responses (non-200) are not cached.
  async "upstream-error"(base, ns) {
    const out = [];
    for (let i = 0; i < 2; i++) {
      const r = await api(base, `key=${ns}-err&cache=force-cache&status=500`);
      out.push([r.status, r.upstreamStatus, r.hit]);
    }
    return out;
  },
  // 8. An unreachable upstream is a fetch failure with ECONNREFUSED.
  async "connection-refused"(base) {
    const r = await api(base, `target=${encodeURIComponent(`http://127.0.0.1:${closedPort}/data`)}`);
    return { status: r.status, code: r.code };
  },
  // 9. Request headers are part of the cache key: users never share.
  async "header-identity"(base, ns) {
    const q = `key=${ns}-auth&cache=force-cache`;
    const a1 = await api(base, q, { "x-upstream-auth": "user-a" });
    const b1 = await api(base, q, { "x-upstream-auth": "user-b" });
    const a2 = await api(base, q, { "x-upstream-auth": "user-a" });
    const b2 = await api(base, q, { "x-upstream-auth": "user-b" });
    return [
      [a1.auth, a1.hit],
      [b1.auth, b1.hit],
      [a2.auth, a2.hit],
      [b2.auth, b2.hit],
    ];
  },
  // 10. Without explicit caching, an authorization header means no cache.
  async "auth-auto-no-cache"(base, ns) {
    const out = [];
    for (let i = 0; i < 2; i++) out.push((await api(base, `key=${ns}-autonc&revalidate=60`, { "x-upstream-auth": "u" })).hit);
    for (let i = 0; i < 2; i++) out.push((await api(base, `key=${ns}-autonc2`, { "x-upstream-auth": "u" })).hit);
    return out;
  },
  // 11. One render, three fetches of one URL: deduplicated upstream.
  async "render-dedupe"(base, ns) {
    const out = [];
    for (let i = 0; i < 2; i++) {
      const r = await get(base, `/data-concurrent?key=${ns}-dd`);
      const sections = [...(r.body ?? "").replace(/<!-- -->/g, "").matchAll(/([abcxy]):([^:<]+):(\d+)/g)].map((m) => `${m[1]}=${m[3]}`);
      out.push(sections.sort().join(","));
    }
    return { renders: out, upstreamHits: (await hits(`${ns}-dd`)).hits };
  },
  // 12. A page reading the data cache.
  async "page-force-cache"(base, ns) {
    const out = [];
    for (let i = 0; i < 3; i++) out.push(pageHit(await get(base, `/data-cache?key=${ns}-page&cache=force-cache`)));
    return out;
  },
  // 13. A client that leaves during an upstream fetch: the server stays
  // healthy and serves the next request. (Whether the upstream fetch is
  // aborted differs by design and is reported, not compared.)
  async cancellation(base, ns) {
    const r = await get(base, `/api/data?key=${ns}-cancel&delay=1500`, { abortAfterMs: 200 });
    await sleep(1800);
    const after = await api(base, `key=${ns}-after`);
    const h = await hits(`${ns}-cancel`);
    return { compare: { clientSaw: r.error ?? r.status, next: after.hit }, report: { upstreamAborted: h.aborted } };
  },
};

// NextRS only: concurrent renders keep their identity across an upstream
// fetch (AsyncLocalStorage through the host network op).
async function identityAfterFetch(base, ns, n = 60) {
  let ok = 0;
  const bad = [];
  await Promise.all(
    Array.from({ length: n }, async (_, i) => {
      const id = `${ns}-id-${i}`;
      const r = await get(base, "/data-fetch", { headers: { "x-nextrs-id": id } });
      const text = (r.body ?? "").replace(/<!-- -->/g, "");
      const m = /data">id=(\S+) key=(\S+) hit=(\d+) after=([^<\s]+)/.exec(text);
      if (r.status === 200 && m && m[1] === id && m[2] === id && m[4] === id) ok++;
      else bad.push({ id, status: r.status, seen: m?.[0] });
    }),
  );
  return { ok, n, bad: bad.slice(0, 5) };
}

const results = [];
let failed = 0;
for (const [name, scenario] of Object.entries(scenarios)) {
  if (only && !only.has(name)) continue;
  const observed = {};
  await Promise.all(
    Object.entries(servers).map(async ([which, base]) => {
      observed[which] = await scenario(base, `${run}-${which[0]}`);
    }),
  );
  const pick = (o) => (o && typeof o === "object" && "compare" in o ? o.compare : o);
  const same = JSON.stringify(pick(observed.baseline)) === JSON.stringify(pick(observed.candidate));
  if (!same) failed++;
  results.push({ name, verdict: same ? "MATCH" : "DIFF", ...observed });
  console.log(`${same ? "MATCH" : "DIFF "}  ${name.padEnd(20)} next=${JSON.stringify(observed.baseline)} nextrs=${JSON.stringify(observed.candidate)}`);
}
if (!only || only.has("identity")) {
  const identity = await identityAfterFetch(servers.candidate, `${run}-c`);
  const pass = identity.ok === identity.n;
  if (!pass) failed++;
  results.push({ name: "identity-after-fetch", verdict: pass ? "PASS" : "FAIL", ...identity });
  console.log(`${pass ? "PASS " : "FAIL "}  identity-after-fetch ${identity.ok}/${identity.n}`);
}
if (args.json) writeFileSync(args.json, JSON.stringify(results, null, 2));
agent.destroy();
process.exitCode = failed ? 1 : 0;
