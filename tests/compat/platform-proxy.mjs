// A data-driven proxy (no config.matcher; it fetches before deciding),
// external rewrites from next.config and from NextResponse.rewrite(),
// instrumentation.ts, waitUntil, Draft Mode, dynamicParams = false and ISR
// fallback, compared between next start and NextRS on
// fixtures/next-platform-proxy.
//
//   DRAFT_SECRET=<test secret> node tests/compat/platform-proxy.mjs \
//        --baseline URL --candidate URL --upstream URL --closed-port N --dist DIR \
//        [--isolates N] [--only a,b] [--json out.json]
//
// NextRS runs with --net-allow for the upstream and the closed port only:
// the fixture's "unlisted" origin, the metadata address and destinations
// taken from the path are outside the policy, and those scenarios are
// documented differences (NextRS refuses with 502, next start proxies).
import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { observe, parseArgs, runScenarios, sleep } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, upstream, dist } = args;
const DRAFT_SECRET = process.env.DRAFT_SECRET;
if (!baseline || !candidate || !upstream || !dist || !DRAFT_SECRET) {
  console.error("usage: DRAFT_SECRET=... platform-proxy.mjs --baseline URL --candidate URL --upstream URL --dist DIR [--json FILE]");
  process.exit(2);
}
const isolates = Number(args.isolates ?? 1);
const only = args.only ? new Set(args.only.split(",")) : null;
const run = `${Date.now().toString(36)}${randomBytes(2).toString("hex")}`;
const ledger = async (key) => (await (await fetch(`${upstream}/mutations?key=${encodeURIComponent(key)}`)).json()).ids;
const hits = async (key) => (await (await fetch(`${upstream}/hits?key=${encodeURIComponent(key)}`)).json());

// An upstream /echo body: what the external rewrite forwarded. Header order
// and the servers' own ports are normalized; `connection` is transport
// (Node's agent sends keep-alive, hyper sends none for HTTP/1.1).
function echo(text, base) {
  let body;
  try {
    body = JSON.parse(text);
  } catch {
    return text;
  }
  if (!body || typeof body !== "object" || !body.headers) return body;
  const port = new URL(base).port;
  const headers = {};
  for (const name of Object.keys(body.headers).sort()) {
    if (name === "connection") continue;
    // next start forwards routing's x-middleware-set-cookie (the proxy's
    // cookies) upstream; NextRS never sends Next's internal headers out.
    // Recorded by proxy-ext-rewrite-internal-header.
    if (name === "x-middleware-set-cookie") continue;
    let value = body.headers[name];
    if (typeof value === "string") value = value.replace(new RegExp(`(127\\.0\\.0\\.1|localhost):${port}\\b`, "g"), "SELF-HOST");
    if (name === "x-forwarded-port" && value === port) value = "SELF-PORT";
    headers[name] = value;
  }
  return { ...body, headers };
}
async function look(base, request) {
  const o = await observe(base, request);
  if (typeof o.body === "string" && o.body.startsWith("{")) o.body = echo(o.body, base);
  return o;
}

const scenarios = {};
const get = (name, path, headers = {}, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (base) => look(base, { path, headers, ...extra }) };
};
const knownDiff = (name, reason, path, headers = {}, extra = {}) => {
  scenarios[name] = { kind: "known-diff", reason, run: (base) => look(base, { path, headers, ...extra }) };
};

// --- A proxy without config.matcher runs for every path ------------------------
get("no-matcher-home", "/");
get("no-matcher-not-found", "/nope");
get("no-matcher-route-handler", "/api/health");
const chunk = (() => {
  try {
    const manifest = JSON.parse(readFileSync(join(dist, "build-manifest.json"), "utf8"));
    return Object.values(manifest).flat().find((f) => typeof f === "string" && f.endsWith(".js")) ?? null;
  } catch {
    return null;
  }
})();
if (chunk) get("no-matcher-static-asset", `/_next/${chunk}`);
get("no-matcher-static-missing", "/_next/static/missing.js");

// --- The proxy fetches, then decides -----------------------------------------------
get("fetch-continue", "/gate/open");
get("fetch-redirect", "/gate/closed");
get("fetch-direct-response", "/gate/direct");
get("fetch-rewrite", "/gate/variant");
get("fetch-status-200", "/gate/status/200");
get("fetch-status-401-redirect", "/gate/status/401");
get("fetch-status-404-rewrite", "/gate/status/404");
get("fetch-status-500-direct", "/gate/status/500");
get("fetch-timeout", "/gate/timeout");
get("fetch-abort", "/gate/abort");
get("fetch-refused", "/gate/refused");
get("fetch-large-response", "/gate/large?bytes=4194304");
knownDiff(
  "fetch-unlisted-destination",
  "the proxy's fetch to an origin outside --net-allow is refused by NextRS (TypeError, cause ERR_NEXTRS_NET_DENIED); next start has no policy",
  "/gate/unlisted",
);
get("fetch-head", "/gate/open", {}, { method: "HEAD" });
get("fetch-rsc", "/gate/variant", { rsc: "1" });

scenarios["fetch-concurrent-identities"] = {
  kind: "compare",
  run: async (base) => {
    const total = 160;
    const concurrency = 32;
    let next = 0;
    const wrong = [];
    let failed = 0;
    async function worker() {
      while (next < total) {
        const i = next++;
        const id = `id${i}-${run}`;
        try {
          const r = await fetch(`${base}/gate/identity?id=${id}&ms=${(i * 7) % 23}`);
          const html = await r.text();
          const info = JSON.parse((/id="info">([^<]*)</.exec(html)?.[1] ?? "{}").replace(/&quot;/g, '"'));
          const cookie = r.headers.getSetCookie().some((c) => c.startsWith(`identity=${id};`));
          if (r.status !== 200 || info.identity !== id || info.seen !== id || info.identityCookie !== id || !cookie) {
            wrong.push({ i, status: r.status, info, cookie });
          }
        } catch {
          failed++;
        }
      }
    }
    await Promise.all(Array.from({ length: concurrency }, worker));
    return { total, wrong: wrong.length, sample: wrong.slice(0, 3), failed };
  },
};

// --- waitUntil ----------------------------------------------------------------------
scenarios["wait-until-runs-after-response"] = {
  kind: "compare",
  run: async (base, side) => {
    const key = `wu-${run}-${side}`;
    const started = Date.now();
    const response = await look(base, { path: `/gate/wait-until?key=${key}&ms=400` });
    const elapsed = Date.now() - started;
    const before = (await ledger(key)).length;
    await sleep(1500);
    response.body = typeof response.body === "object" ? { scheduled: Boolean(response.body?.scheduled) } : response.body;
    return { response, answeredBeforeWork: elapsed < 400 && before === 0, after: await ledger(key) };
  },
};
scenarios["wait-until-failure-isolated"] = {
  kind: "compare",
  run: async (base) => ({ failing: await look(base, { path: "/gate/wait-until-fail" }), next: await look(base, { path: "/gate/direct" }) }),
};
scenarios["wait-until-never-settles"] = {
  kind: "candidate",
  run: async (base) => {
    const r = await look(base, { path: "/gate/wait-until-forever" });
    // The response is not held back, and the pool keeps serving; the work
    // is ended by the request deadline (--request-timeout).
    const after = await Promise.all(Array.from({ length: 8 }, () => look(base, { path: "/gate/direct" })));
    const ok = r.status === 200 && after.every((a) => a.status === 202);
    return { ok, detail: { first: r.status, after: after.map((a) => a.status) } };
  },
};

// --- instrumentation.ts ----------------------------------------------------------------
const instrumentation = (o) => {
  if (o.body?.state) delete o.body.state.registeredAt;
  return o;
};
scenarios["instrumentation-registered-once"] = {
  kind: isolates === 1 ? "compare" : "candidate",
  run: async (base) => {
    const state = instrumentation(await look(base, { path: "/api/instrumentation" }));
    if (isolates === 1) return state;
    const registrations = state.body?.state?.registrations;
    return { ok: registrations === 1, detail: state.body };
  },
};
get("instrumentation-before-proxy", "/gate/instrumentation");
scenarios["instrumentation-on-request-error"] = {
  kind: isolates === 1 ? "compare" : "candidate",
  run: async (base) => {
    const boom = await look(base, { path: "/api/boom" });
    await sleep(100);
    const state = instrumentation(await look(base, { path: "/api/instrumentation" }));
    const errors = state.body?.state?.errors ?? [];
    const seen = errors.some((e) => e.path === "/api/boom" && e.routeType === "route" && /synthetic/.test(e.message));
    if (isolates === 1) return { boom, seen, errors: errors.length > 0 };
    return { ok: boom.status === 500 && (seen || errors.length === 0), detail: { boom: boom.status, errors } };
  },
};

// --- External rewrites from next.config ----------------------------------------------
get("ext-get", "/ext/a/b?x=1&y=%20z&x=2");
get("ext-query-merge", "/ext-query?extra=1&fixed=client");
get("ext-post-json", "/ext/post", { "content-type": "application/json" }, { method: "POST", body: JSON.stringify({ a: 1, b: "two" }) });
get("ext-put-text", "/ext/put", { "content-type": "text/plain" }, { method: "PUT", body: "plain text body" });
get("ext-delete", "/ext/delete", {}, { method: "DELETE" });
get("ext-head", "/ext/head", {}, { method: "HEAD" });
get("ext-cookies-and-auth", "/ext/creds", { cookie: "session=abc; theme=dark", authorization: "Bearer test-token" });
get("ext-redirect-not-followed", "/ext-redirect");
get("ext-status-418", "/ext-status/418");
get("ext-status-204", "/ext-status/204");
get("ext-rsc", "/ext/rsc", { rsc: "1", "next-router-state-tree": "%5B%22%22%5D" });
get("ext-refused", "/ext-closed/x");
get("ext-forwarded-headers", "/ext/fwd", { "x-forwarded-host": "client.example", "x-forwarded-proto": "https", "x-forwarded-for": "198.51.100.7" });
get("ext-internal-headers", "/ext/internal", {
  "x-middleware-rewrite": "/x",
  "x-middleware-next": "1",
  "x-middleware-set-cookie": "a=1",
  "x-matched-path": "/x",
  "x-nextjs-data": "1",
  "next-resume": "1",
});
knownDiff(
  "ext-hop-by-hop-headers",
  "NextRS does not forward hop-by-hop request headers (RFC 9110 7.6.1: Connection and the headers it names, TE, Keep-Alive, Proxy-*); next start's httpxy forwards them",
  "/ext/hop",
  { connection: "x-drop-me", "x-drop-me": "1", te: "trailers", "keep-alive": "timeout=5", "proxy-authorization": "Basic eA==" },
  // Sent raw: fetch (undici) refuses to send hop-by-hop headers itself.
  { raw: true },
);
scenarios["ext-large-response"] = {
  kind: "compare",
  run: async (base) => {
    const r = await fetch(`${base}/ext-large/8388608`);
    const bytes = (await r.arrayBuffer()).byteLength;
    return { status: r.status, length: r.headers.get("content-length"), bytes };
  },
};
// experimental.proxyTimeout (5 s in the fixture): an upstream that does
// not answer in time gets next start's proxy error (500), and one that goes
// quiet mid-body is cut off.
scenarios["ext-proxy-timeout"] = {
  kind: "compare",
  expect: (o) => (o.status === 500 && o.body === "Internal Server Error" && o.seconds >= 4 && o.seconds < 7 ? null : JSON.stringify(o)),
  run: async (base) => {
    const started = Date.now();
    const r = await fetch(`${base}/ext-too-slow`);
    const body = await r.text();
    return { status: r.status, body, seconds: Math.round((Date.now() - started) / 1000) };
  },
};
scenarios["ext-proxy-timeout-mid-body"] = {
  kind: "compare",
  // Whether the first line reached the client before the cut is not
  // compared: next start's compression holds it back, NextRS streams it
  // (ext-streamed-response-incremental).
  expect: (o) => (o.status === 200 && o.cut && o.seconds >= 4 && o.seconds < 6 ? null : JSON.stringify(o)),
  run: async (base) => {
    const started = Date.now();
    const r = await fetch(`${base}/ext-stall`);
    const reader = r.body.getReader();
    const decoder = new TextDecoder();
    let text = "";
    let cut = false;
    for (;;) {
      try {
        const { done, value } = await reader.read();
        if (done) break;
        text += decoder.decode(value, { stream: true });
      } catch {
        cut = true;
        break;
      }
    }
    return { status: r.status, cut: cut || !text.includes("line 2"), seconds: Math.round((Date.now() - started) / 1000) };
  },
};
scenarios["ext-streamed-response"] = {
  kind: "compare",
  run: async (base) => {
    const started = Date.now();
    const r = await fetch(`${base}/ext-stream`);
    const reader = r.body.getReader();
    let first = null;
    let text = "";
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      first ??= Date.now() - started;
      text += Buffer.from(value).toString();
    }
    // 5 lines 100 ms apart. next start compresses this response for a
    // client that accepts gzip, which buffers it; NextRS passes it through
    // as it arrives (checked by ext-streamed-response-incremental).
    return { status: r.status, text };
  },
};
scenarios["ext-streamed-response-incremental"] = {
  kind: "candidate",
  run: async (base) => {
    const started = Date.now();
    const r = await fetch(`${base}/ext-stream`);
    const reader = r.body.getReader();
    let first = null;
    for (;;) {
      const { done } = await reader.read();
      if (done) break;
      first ??= Date.now() - started;
    }
    const total = Date.now() - started;
    return { ok: first !== null && first < total - 200, detail: { firstChunkMs: first, totalMs: total } };
  },
};
scenarios["ext-large-upload"] = {
  kind: "compare",
  run: async (base) => {
    const body = "y".repeat(3 * 1024 * 1024);
    const r = await fetch(`${base}/ext/upload`, { method: "POST", body, headers: { "content-type": "text/plain" } });
    const echoed = await r.json();
    return { status: r.status, received: echoed.body?.length, length: echoed.headers?.["content-length"] ?? echoed.headers?.["transfer-encoding"] };
  },
};
scenarios["ext-concurrent"] = {
  kind: "compare",
  run: async (base) => {
    let wrong = 0;
    await Promise.all(
      Array.from({ length: 64 }, async (_, i) => {
        const r = await fetch(`${base}/ext/c${i}?n=${i}`);
        const e = await r.json();
        if (e.url !== `/echo/c${i}?n=${i}`) wrong++;
      }),
    );
    return { wrong };
  },
};
scenarios["ext-action-post-forwarded"] = {
  kind: "compare",
  run: async (base, side) => {
    const form = new FormData();
    form.append("_1_key", `ext-action-${run}`);
    form.append("0", '["$K1"]');
    const r = await look(base, { path: "/ext/action", method: "POST", body: form, headers: { "next-action": "0".repeat(42), origin: base } });
    if (r.body?.headers) r.body.headers["content-type"] = r.body.headers["content-type"]?.replace(/boundary=.*/, "boundary=…");
    if (typeof r.body?.body === "string") r.body.body = r.body.body.replace(/-{2,}[\w-]+/g, "--BOUNDARY");
    return r;
  },
};
scenarios["ext-client-abort"] = {
  kind: "candidate",
  run: async (base) => {
    const controller = new AbortController();
    const pending = fetch(`${base}/ext-slow`, { signal: controller.signal }).catch((e) => e.name);
    await sleep(300);
    controller.abort();
    const outcome = await pending;
    await sleep(3200);
    const counted = await hits("ext-slow");
    const after = await look(base, { path: "/ext/after-abort" });
    return { ok: outcome === "AbortError" && counted.aborted >= 1 && after.status === 200, detail: { outcome, counted, after: after.status } };
  },
};
knownDiff(
  "ext-unlisted-destination",
  "a config rewrite to an origin outside --net-allow: NextRS 502 external-rewrite-denied, nothing sent; next start proxies",
  "/ext-unlisted/x",
);
knownDiff(
  "ext-metadata-destination",
  "a config rewrite to 169.254.169.254: NextRS 502 external-rewrite-denied, nothing sent; next start tries to connect",
  "/ext-metadata",
);
get("ext-host-from-path-allowed", `/ext-host/${new URL(upstream).host}`);
knownDiff(
  "ext-host-from-path-metadata",
  "a destination host taken from the path (169.254.169.254): NextRS refuses it, next start tries to connect",
  "/ext-host/169.254.169.254",
);
knownDiff(
  "ext-host-from-path-encoded-loopback",
  "a destination host taken from the path (2130706433, loopback in decimal, port 80 not in --net-allow): NextRS refuses it, next start connects (refused here)",
  "/ext-host/2130706433",
);

// --- External rewrites from the proxy --------------------------------------------------
get("proxy-ext-rewrite", "/gate/external?client=1");
get("proxy-ext-rewrite-post", "/gate/external", { "content-type": "text/plain" }, { method: "POST", body: "via proxy" });
scenarios["proxy-ext-rewrite-internal-header"] = {
  kind: "known-diff",
  reason: "next start forwards routing's x-middleware-set-cookie (the proxy's own cookies) to the upstream; NextRS does not send Next's internal headers to other origins",
  run: async (base) => {
    const r = await fetch(`${base}/gate/external`);
    return { forwarded: "x-middleware-set-cookie" in ((await r.json()).headers ?? {}) };
  },
};
knownDiff(
  "proxy-ext-rewrite-unlisted",
  "NextResponse.rewrite() to an origin outside --net-allow: NextRS 502, next start proxies",
  "/gate/external-unlisted",
);

// --- Draft Mode, dynamicParams = false, ISR fallback ------------------------------------
get("posts-listed", "/posts/a");
get("posts-unlisted-dynamic-params-false", "/posts/c");
get("posts-unlisted-rsc", "/posts/c", { rsc: "1" });
get("proxy-rewrite-to-listed-post", "/gate/post/b");
get("proxy-rewrite-to-unlisted-post", "/gate/post/z");
get("draft-wrong-secret", "/api/draft?secret=wrong&slug=a");
get("draft-forged-cookie", "/posts/a", { cookie: "__prerender_bypass=forged" });
const stampOf = (html) => /stamp-(?:<!-- -->)?(\d+)/.exec(html)?.[1];
scenarios["draft-mode-round-trip"] = {
  kind: "compare",
  run: async (base) => {
    const enable = await fetch(`${base}/api/draft?secret=${encodeURIComponent(DRAFT_SECRET)}&slug=a`, { redirect: "manual" });
    const bypass = enable.headers.getSetCookie().find((c) => c.startsWith("__prerender_bypass="));
    const cookie = bypass?.split(";")[0] ?? "";
    const attrs = bypass?.split(";").slice(1).map((s) => s.trim().split("=")[0].toLowerCase()).sort() ?? [];
    const page = async () => {
      const r = await fetch(`${base}/posts/a`, { headers: { cookie } });
      const html = await r.text();
      return { status: r.status, kind: /id="kind">([^<]*)</.exec(html)?.[1], stamp: stampOf(html), cache: r.headers.get("cache-control") };
    };
    const one = await page();
    await sleep(20);
    const two = await page();
    const unlisted = await fetch(`${base}/posts/zz`, { headers: { cookie } });
    const disable = await fetch(`${base}/api/draft/disable`, { headers: { cookie } });
    const cleared = disable.headers.getSetCookie().find((c) => c.startsWith("__prerender_bypass=")) ?? null;
    const published = await fetch(`${base}/posts/a`);
    return {
      enable: { status: enable.status, location: enable.headers.get("location"), cookieSet: Boolean(bypass), attrs },
      draft: { status: one.status, kind: one.kind, kind2: two.kind, rendered: one.stamp !== two.stamp, cache: one.cache },
      unlistedInDraft: unlisted.status,
      disabled: { status: disable.status, cleared: Boolean(cleared && /Expires=Thu, 01 Jan 1970|Max-Age=0/i.test(cleared)) },
      published: { status: published.status, kind: /id="kind">([^<]*)</.exec(await published.text())?.[1] },
    };
  },
};
get("isr-listed", "/isr-posts/x");
scenarios["isr-fallback-generated-then-cached"] = {
  kind: "compare",
  run: async (base, side) => {
    const slug = `f-${run}-${side}`;
    const first = await fetch(`${base}/isr-posts/${slug}`);
    const a = stampOf(await first.text());
    await sleep(50);
    const second = await fetch(`${base}/isr-posts/${slug}`);
    const b = stampOf(await second.text());
    return { first: first.status, second: second.status, cache: second.headers.get("x-nextjs-cache"), cached: a === b };
  },
};

const failures = await runScenarios(`platform-proxy (${isolates} isolate${isolates === 1 ? "" : "s"})`, scenarios, {
  baseline,
  candidate,
  json: args.json,
  only,
});
process.exit(failures ? 1 : 0);
