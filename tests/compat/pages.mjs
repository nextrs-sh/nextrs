// Pages Router compatibility: fixtures/next-pages under next start (the
// oracle) and NextRS, compared response by response, with each critical
// scenario's outcome also asserted on its own (page identity from
// __NEXT_DATA__, props, statuses, ledgers), so a wrong answer both servers
// gave is still caught.
//
//   node tests/compat/pages.mjs --baseline URL --candidate URL --upstream URL
//        --dist fixtures/next-pages/.next [--variant default|base-path|trailing-slash]
//        [--json out.json] [--only a,b]
//
// Normalized away (beyond routing-lib's transport headers): per-request
// stamps in getServerSideProps output, the side name in keys each server
// gets its own of, and ETags of bodies that carry such stamps.
import { readFileSync } from "node:fs";
import path from "node:path";
import { parseArgs, runScenarios, sleep } from "./routing-lib.mjs";

const args = parseArgs();
const baseline = args.baseline;
const candidate = args.candidate;
const upstream = args.upstream;
const variant = args.variant ?? "default";
const dist = args.dist;
const buildId = readFileSync(path.join(dist, "BUILD_ID"), "utf8").trim();
const prefix = variant === "base-path" ? "/base" : "";
const slash = variant === "trailing-slash";
const ns = `n${Date.now().toString(36)}`;
const only = args.only ? new Set(args.only.split(",")) : null;

// A page URL in this build's spelling (basePath, trailing slash).
const page = (p) => {
  const [pathname, query] = p.split("?");
  let out = `${prefix}${pathname === "/" && prefix ? "" : pathname}`;
  if (slash && !out.endsWith("/")) out += "/";
  if (!out) out = "/";
  return query === undefined ? out : `${out}?${query}`;
};
const data = (p) => `${prefix}/_next/data/${buildId}${p}`;
const SKIP = new Set(["date", "connection", "keep-alive", "transfer-encoding", "content-encoding", "content-length", "last-modified", "accept-ranges"]);
const SELF = /\bhttps?:\/\/(?:localhost|0\.0\.0\.0|127\.0\.0\.1|\[::1\]):\d+/g;
const decode = (s) => s.replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
const volatile = (text) => text.replace(/"stamp":\d+/g, '"stamp":0').replace(/-(baseline|candidate)\b/g, "-SIDE");

function summarize(html) {
  const out = {};
  for (const id of ["title", "slug", "kind", "secret", "query", "info", "result"]) {
    const m = new RegExp(`id="${id}">([\\s\\S]*?)</`).exec(html);
    if (m) out[id] = volatile(decode(m[1].replace(/<!-- -->/g, "")));
  }
  // Which page rendered, with which props: Next's own record of it.
  const next = /<script id="__NEXT_DATA__" type="application\/json">([\s\S]*?)<\/script>/.exec(html);
  if (next) {
    const d = JSON.parse(next[1]);
    out.next = {
      page: d.page,
      query: JSON.parse(volatile(JSON.stringify(d.query ?? {}))),
      buildId: d.buildId === buildId ? "BUILD" : d.buildId,
      isFallback: d.isFallback ?? false,
      gssp: d.gssp ?? false,
      gsp: d.gsp ?? false,
      gip: d.gip ?? false,
      appGip: d.appGip ?? false,
      props: JSON.parse(volatile(JSON.stringify(d.props ?? {}))),
    };
  }
  out.document = html.includes('data-doc="nextrs-pages-doc"');
  out.app = html.includes('data-app="nextrs-pages-app"');
  out.css = /<link rel="stylesheet" href="[^"]*\/_next\/static\//.test(html);
  return out;
}

async function look(base, request) {
  const { method = "GET", path: p, headers = {}, body } = request;
  let response;
  try {
    response = await fetch(`${base}${p}`, { method, headers: { "accept-encoding": "identity", ...headers }, body, redirect: "manual" });
  } catch (error) {
    return { error: String(error?.cause?.code ?? error?.message ?? error) };
  }
  const text = await response.text().catch((error) => `<<body error ${error?.message}>>`);
  const h = {};
  const port = new URL(base).port;
  const stamped = /"stamp":\d+|stamp-(?:<!-- -->)?\d+/.test(text) || /-(baseline|candidate)\b/.test(text);
  for (const [name, value] of response.headers) {
    if (SKIP.has(name) || name === "set-cookie") continue;
    if (name === "etag" && stamped) continue;
    let v = value.replace(SELF, "http://SELF").replace(new RegExp(`^(127\\.0\\.0\\.1|localhost):${port}$`), "SELF-HOST");
    if (name === "vary") {
      v = v.split(/,\s*/).filter((t) => t.toLowerCase() !== "accept-encoding").join(", ");
      if (!v) continue;
    }
    h[name] = v;
  }
  const type = (response.headers.get("content-type") ?? "").split(";")[0];
  let shown;
  if (type === "text/html") shown = summarize(text);
  else if (type === "application/json") {
    try {
      shown = JSON.parse(volatile(text));
    } catch {
      shown = volatile(text);
    }
  } else if (/^\{[\s\S]*\}$/.test(text.trim())) {
    // Next sends some JSON bodies untyped ({"notFound":true}, {}).
    try {
      shown = JSON.parse(volatile(text));
    } catch {
      shown = volatile(text);
    }
  } else shown = volatile(text.replace(SELF, "http://SELF"));
  return {
    status: response.status,
    headers: h,
    setCookie: response.headers.getSetCookie().map((c) => c.replace(/Expires=[^;]+/i, "Expires=X").replace(/Max-Age=\d+/i, "Max-Age=X")),
    body: shown,
  };
}

// A page rendered at request time carries that time (`built`), so two
// servers' renders differ there and only there.
function unstamp(o) {
  if (!o || typeof o !== "object") return o;
  if (o.headers) delete o.headers.etag;
  if (o.body?.info?.startsWith?.("built-")) o.body.info = "built-REQUEST";
  for (const props of [o.body?.next?.props?.pageProps, o.body?.pageProps]) if (props && "built" in props) props.built = 0;
  return o;
}

// Assertions independent of the comparison.
const is = (cond, message) => (cond ? null : message);
const all = (...checks) => checks.find((c) => c !== null) ?? null;
const pageIs = (o, name) => is(o.body?.next?.page === name, `rendered ${o.body?.next?.page ?? "no page"}, expected ${name}`);
const status = (o, code) => is(o.status === code, `status ${o.status}, expected ${code}`);
const layout = (o) => is(o.body?.document && o.body?.app && o.body?.css, "custom _document, _app or global CSS missing");
const notFoundPage = (o) => all(status(o, 404), is(o.body?.title === "Custom 404 (pages)", `not the custom 404 page: ${o.body?.title}`));

const scenarios = {};
const add = (name, p, expect, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (base) => look(base, { path: p, ...extra }), expect };
};

// --- static pages, the App and Document ---------------------------------
add("static-index", page("/"), (o) => all(status(o, 200), pageIs(o, "/"), layout(o), is(o.body.title === "NextRS Pages Router", "wrong title")));
add("static-about", page("/about"), (o) => all(status(o, 200), pageIs(o, "/about"), is(!o.headers["cache-control"], "a static page has no Cache-Control in next start")));
add("static-nested", page("/nested/deep"), (o) => all(status(o, 200), pageIs(o, "/nested/deep")));
add("static-query", page("/about?x=1&y=2"), (o) => all(status(o, 200), pageIs(o, "/about")));
add("static-head", page("/about"), (o) => status(o, 200), { method: "HEAD" });
add("static-post-405", page("/about"), (o) => all(status(o, 405), is(o.headers.allow === "GET, HEAD", "Allow")), { method: "POST" });
// Conditional requests, judged by the fresh package in next start. A
// request with Cache-Control: no-cache (Node's fetch adds it to any
// conditional request) is never fresh; a plain one is, whatever the
// status (the 404 page too).
import http from "node:http";
function conditional(base, p, etag) {
  return new Promise((resolve, reject) => {
    const url = new URL(base);
    const req = http.request({ host: url.hostname, port: url.port, path: p, headers: { "if-none-match": etag, "accept-encoding": "identity" }, agent: false }, (res) => {
      res.resume();
      res.on("end", () => resolve({ status: res.statusCode, etag: res.headers.etag ?? null }));
    });
    req.on("error", reject);
    req.end();
  });
}
async function etagOf(base, p) {
  const r = await fetch(`${base}${p}`, { headers: { "accept-encoding": "identity" } });
  await r.text();
  return r.headers.get("etag") ?? "";
}
scenarios["conditional-no-cache"] = {
  kind: "compare",
  run: async (base) => look(base, { path: page("/about"), headers: { "if-none-match": await etagOf(base, page("/about")) } }),
  expect: (o) => status(o, 200),
};
for (const [name, p] of [["conditional-static-page", "/about"], ["conditional-ssg-page", "/ssg/static"], ["conditional-404-page", "/missing-page"]]) {
  scenarios[name] = {
    kind: "compare",
    run: async (base) => conditional(base, page(p), await etagOf(base, page(p))),
    expect: (o) => is(o.status === 304, `status ${o.status}, expected 304`),
  };
}
scenarios["conditional-static-asset"] = {
  kind: "compare",
  run: async (base) => {
    const html = await fetch(`${base}${page("/about")}`).then((r) => r.text());
    const asset = /src="([^"]*\/_next\/static\/chunks\/[^"]+\.js)"/.exec(html)?.[1];
    const first = await conditional(base, asset, '"none"');
    const second = await conditional(base, asset, first.etag ?? "");
    return { first: first.status, second: second.status };
  },
  expect: (o) => is(o.first === 200 && o.second === 304, `statuses ${o.first}, ${o.second}`),
};

// An RSC request (rsc: 1) to a Pages Router page without its _rsc
// parameter: base server's cache-busting redirect, the 404 page and API
// routes aside.
add("rsc-header-static-page", { path: page("/about"), headers: { rsc: "1" } }.path, (o) => status(o, 307), { headers: { rsc: "1" } });
add("rsc-header-ssr-page", page("/books/1"), (o) => status(o, 307), { headers: { rsc: "1" } });
add("rsc-header-ssg-post", page("/ssg/static"), (o) => status(o, 307), { method: "POST", headers: { rsc: "1" } });
add("rsc-header-api", page("/api/health"), (o) => status(o, 200), { headers: { rsc: "1" } });
add("rsc-header-404", page("/missing-page"), (o) => status(o, 404), { headers: { rsc: "1" } });

// --- not found and error pages -------------------------------------------
add("missing-page", page("/missing-page"), (o) => all(notFoundPage(o), pageIs(o, "/404")));
add("status-page-404", page("/404"), (o) => all(notFoundPage(o), is(!o.headers["cache-control"], "next start sends /404 without Cache-Control")));
add("status-page-500", page("/500"), (o) => all(status(o, 500), is(o.body?.title === "Custom 500 (pages)", "custom 500")));
add("blocked-app", page("/_app"), notFoundPage);
add("blocked-document", page("/_document"), notFoundPage);
add("blocked-error", page("/_error"), notFoundPage);
add("api-missing", page("/api/missing"), notFoundPage);
add("missing-post", page("/missing-page"), notFoundPage, { method: "POST" });

// --- getServerSideProps --------------------------------------------------
add("ssr-params-query", page("/books/42?ref=a&ref=b&x=1"), (o) =>
  all(status(o, 200), pageIs(o, "/books/[id]"), is(o.body.slug === "42", "param"), is(o.body.next?.gssp === true, "gssp"),
    is(JSON.stringify(o.body.next?.props?.pageProps?.query) === JSON.stringify({ ref: ["a", "b"], x: "1", id: "42" }), `query ${JSON.stringify(o.body.next?.props?.pageProps?.query)}`)));
add("ssr-header-cookie", page("/books/7"), (o) => all(status(o, 200), is(o.body.secret === "user=u-alice header=h-alice", `saw ${o.body.secret}`), is(o.headers["x-book-id"] === "7", "the page's own header")),
  { headers: { cookie: "user=u-alice; other=1", "x-test-user": "h-alice" } });
add("ssr-encoded-param", page("/books/caf%C3%A9%20au%20lait"), (o) => all(status(o, 200), is(o.body.slug === "café au lait", `param ${o.body.slug}`)));
add("ssr-trailing-slash-redirect", slash ? `${prefix}/books/42` : `${prefix}/books/42/`, (o) => status(o, 308));
add("ssr-options-400", page("/books/1"), (o) => status(o, 400), { method: "OPTIONS" });
add("ssr-delete-renders", page("/books/1"), (o) => all(status(o, 200), pageIs(o, "/books/[id]")), { method: "DELETE" });
add("ssr-redirect", page("/ssr/redirect"), (o) => all(status(o, 307), is(o.headers.location === `${prefix}/about?from=ssr`, `location ${o.headers.location}`)));
add("ssr-redirect-permanent", page("/ssr/redirect?permanent=1"), (o) => all(status(o, 308), is(o.headers.location === `${prefix}/about`, "location")));
add("ssr-redirect-external", page("/ssr/redirect?external=1"), (o) => all(status(o, 307), is(o.headers.location === "https://example.com/elsewhere", "location")));
add("ssr-redirect-303", page("/ssr/redirect?status=303"), (o) => all(status(o, 303), is(o.headers.location === `${prefix}/books/7`, "location")));
add("ssr-not-found", page("/ssr/not-found"), (o) => all(notFoundPage(o), is(o.headers["cache-control"] === "private, no-cache, no-store, max-age=0, must-revalidate", "cache-control")));
add("ssr-error", page("/ssr/error"), (o) => all(status(o, 500), is(o.body?.title === "Custom 500 (pages)", "custom 500")));
add("ssr-render-error", page("/ssr/render-error"), (o) => all(status(o, 500), is(o.body?.title === "Custom 500 (pages)", "custom 500")));
add("ssr-internal-headers", page("/ssr/headers"), (o) =>
  // router-server removes Next's internal headers (x-nextjs-data,
  // x-matched-path, x-now-route-matches); others reach the page in next
  // start too.
  all(status(o, 200), is(o.body.result === JSON.stringify({ "x-test-user": "visible", "x-middleware-subrequest": "middleware:middleware:middleware:middleware:middleware", "x-invoke-path": "/books/1" }), `the page saw ${o.body.result}`), is(o.setCookie.length === 2, "two cookies")),
  { headers: { "x-test-user": "visible", "x-middleware-subrequest": "middleware:middleware:middleware:middleware:middleware", "x-nextjs-data": "1", "x-matched-path": "/books/1", "x-now-route-matches": "id=1", "x-invoke-path": "/books/1" } });
add("gip-page", page("/gip?z=1"), (o) => all(status(o, 200), pageIs(o, "/gip"), is(o.body.kind === "server", "getInitialProps ran on the server"), is(o.body.next?.gip === true, "gip")));
scenarios["ssr-fetch"] = {
  kind: "compare",
  run: (base, side) => look(base, { path: page(`/ssr/fetch?key=pages-fetch-${ns}-${side}`), headers: { authorization: "Bearer t-1" } }),
  expect: (o) => all(status(o, 200), is(o.body.result === JSON.stringify({ key: `pages-fetch-${ns}-SIDE`, hit: 1, auth: "Bearer t-1" }), `fetched ${o.body.result}`)),
};

// --- getStaticProps / getStaticPaths -------------------------------------
add("ssg-static", page("/ssg/static"), (o) => all(status(o, 200), pageIs(o, "/ssg/static"), is(o.headers["x-nextjs-cache"] === "HIT", "served from the build"), is(o.body.next?.gsp === true, "gsp")));
add("ssg-paths-alpha", page("/posts/alpha"), (o) => all(status(o, 200), pageIs(o, "/posts/[slug]"), is(o.body.slug === "alpha", "slug")));
add("ssg-paths-beta", page("/posts/beta"), (o) => all(status(o, 200), is(o.body.slug === "beta", "slug")));
add("ssg-fallback-false-unknown", page("/posts/gamma"), notFoundPage);
add("ssg-not-found", page("/ssg/not-found"), notFoundPage);
add("ssg-post-405", page("/ssg/static"), (o) => status(o, 405), { method: "POST" });
add("fallback-true-known", page("/fallback-true/known"), (o) => all(status(o, 200), is(o.body.kind === "fallback-true", "rendered page"), is(o.body.next?.isFallback === false, "not a fallback")));
scenarios["fallback-true-new"] = {
  kind: "compare",
  run: async (base) => {
    const slug = `new-${ns}`;
    const shell = await look(base, { path: page(`/fallback-true/${slug}`) });
    const json = await look(base, { path: data(`/fallback-true/${slug}.json`) });
    const after = await look(base, { path: page(`/fallback-true/${slug}`) });
    return { shell, json: { status: json.status, slug: json.body?.pageProps?.slug, cache: json.headers["x-nextjs-cache"] }, after: unstamp(after) };
  },
  expect: (o) =>
    all(
      is(o.shell.body?.next?.isFallback === true && o.shell.body?.kind === "fallback-loading", "first request gets the fallback shell"),
      is(o.json.status === 200 && o.json.slug === `new-${ns}`, `data request generates the page: ${JSON.stringify(o.json)}`),
      is(o.after.body?.kind === "fallback-true" && o.after.body?.next?.isFallback === false, "then the page itself"),
    ),
};
add("fallback-true-missing-data", data(`/fallback-true/missing.json`), (o) => all(status(o, 404), is(o.body?.notFound === true, "notFound JSON")));
// Regenerated in the background once its 60 s pass: the stamp is the
// regeneration's time on each server.
scenarios["fallback-blocking-known"] = {
  kind: "compare",
  run: async (base) => unstamp(await look(base, { path: page("/fallback-blocking/known") })),
  expect: (o) => all(status(o, 200), is(o.body.kind === "fallback-blocking", "page")),
};
scenarios["fallback-blocking-new"] = {
  kind: "compare",
  run: async (base) => {
    const slug = `blk-${ns}`;
    const first = await look(base, { path: page(`/fallback-blocking/${slug}`) });
    const second = await look(base, { path: page(`/fallback-blocking/${slug}`) });
    const same = first.body?.info === second.body?.info;
    return { first: unstamp(first), second: unstamp(second), same };
  },
  expect: (o) =>
    all(
      is(o.first.status === 200 && o.first.body?.slug === `blk-${ns}` && o.first.body?.next?.isFallback === false, "rendered on the first request, no shell"),
      is(o.first.headers["x-nextjs-cache"] === "MISS" && o.second.headers["x-nextjs-cache"] === "HIT", `cache ${o.first.headers["x-nextjs-cache"]} then ${o.second.headers["x-nextjs-cache"]}`),
      is(o.same, "the second request is the stored page"),
    ),
};
add("fallback-blocking-missing", page("/fallback-blocking/missing"), notFoundPage);
add("fallback-blocking-redirect", page("/fallback-blocking/moved"), (o) => all(status(o, 307), is(o.headers.location === `${prefix}/posts/alpha`, "location")));

// --- /_next/data -----------------------------------------------------------
add("data-ssr", data("/books/42.json?x=1"), (o) =>
  all(status(o, 200), is(o.body?.__N_SSP === true && o.body?.pageProps?.id === "42" && o.body?.pageProps?.query?.x === "1", `props ${JSON.stringify(o.body)}`)));
add("data-ssr-cookie", data("/ssr/whoami.json"), (o) => all(status(o, 200), is(o.body?.pageProps?.user === "u-bob", "identity")), { headers: { cookie: "user=u-bob" } });
add("data-ssg", data("/ssg/static.json"), (o) => all(status(o, 200), is(o.body?.__N_SSG === true && o.body?.pageProps?.nested?.when === "build", "props"), is(o.headers["x-nextjs-cache"] === "HIT", "cache")));
add("data-ssg-dynamic", data("/posts/alpha.json"), (o) => all(status(o, 200), is(o.body?.pageProps?.slug === "alpha", "slug")));
add("data-fallback-false-unknown", data("/posts/gamma.json"), (o) => status(o, 404));
add("data-redirect", data("/ssr/redirect.json"), (o) => all(status(o, 200), is(o.body?.pageProps?.__N_REDIRECT === "/about?from=ssr", `redirect data ${JSON.stringify(o.body)}`)));
add("data-not-found", data("/ssr/not-found.json"), (o) => all(status(o, 404), is(o.body?.notFound === true, "notFound")));
add("data-ssg-not-found", data("/ssg/not-found.json"), (o) => all(status(o, 404), is(o.body?.notFound === true, "notFound")));
add("data-wrong-build", `${prefix}/_next/data/not-the-build/books/42.json`, (o) => status(o, 404));
add("data-missing", data("/no-such-page.json"), (o) => status(o, 404));
add("data-index", data("/index.json"), (o) => status(o, 200));
add("data-static-page", data("/about.json"), (o) => status(o, 200));
add("data-error", data("/ssr/error.json"), (o) => status(o, 500));
add("data-encoded-param", data("/books/a%20b.json"), (o) => all(status(o, 200), is(o.body?.pageProps?.id === "a b", `id ${o.body?.pageProps?.id}`)));
// trailingSlash: next start's trailing-slash redirect comes first.
add("data-no-json-suffix", data("/books/42"), (o) => status(o, slash ? 308 : 404));
add("data-through-proxy-rewrite", data("/mw/rewrite-book.json"), (o) => all(status(o, 200), is(o.body?.pageProps?.id === "99", "rewritten page's data")));

// --- API routes ------------------------------------------------------------
add("api-health", page("/api/health"), (o) => all(status(o, 200), is(o.body?.status === "ok", "body")));
add("api-head", page("/api/health"), (o) => status(o, 200), { method: "HEAD" });
add("api-get-query", page("/api/echo?a=1&a=2&b=x%20y"), (o) =>
  all(status(o, 200), is(JSON.stringify(o.body?.query) === JSON.stringify({ a: ["1", "2"], b: "x y" }), `query ${JSON.stringify(o.body?.query)}`), is(o.headers["x-api-route"] === "echo", "header")));
add("api-post-json", page("/api/echo"), (o) => all(status(o, 200), is(JSON.stringify(o.body?.body) === JSON.stringify({ n: [1, 2], s: "é" }), "parsed JSON")),
  { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ n: [1, 2], s: "é" }) });
add("api-post-form", page("/api/echo"), (o) => all(status(o, 200), is(JSON.stringify(o.body?.body) === JSON.stringify({ a: "1", b: "x y", c: ["1", "2"] }), `form ${JSON.stringify(o.body?.body)}`)),
  { method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" }, body: "a=1&b=x+y&c=1&c=2" });
add("api-post-text", page("/api/echo"), (o) => all(status(o, 200), is(o.body?.body === "plain body", "text")), { method: "POST", headers: { "content-type": "text/plain" }, body: "plain body" });
add("api-post-bad-json", page("/api/echo"), (o) => status(o, 400), { method: "POST", headers: { "content-type": "application/json" }, body: "{not json" });
add("api-cookies", page("/api/cookies?v=9"), (o) => all(status(o, 201), is(o.setCookie.length === 2 && o.setCookie[0].startsWith("api-a=9"), "Set-Cookie"), is(o.body?.seen?.c1 === "v1", "request cookie")),
  { headers: { cookie: "c1=v1" } });
add("api-redirect", page("/api/redirect"), (o) => all(status(o, 307), is(o.headers.location === "/books/1?via=api", "location")));
add("api-redirect-301", page("/api/redirect?status=301"), (o) => status(o, 301));
add("api-error-throw", page("/api/error"), (o) => all(status(o, 500), is(o.body === "Internal Server Error", "Next's error body")));
add("api-error-status", page("/api/error?kind=status"), (o) => all(status(o, 418), is(o.body?.error === "teapot", "body")));
add("api-stream", page("/api/stream?chunks=4&gap=30"), (o) => all(status(o, 200), is(o.body === "chunk-0\nchunk-1\nchunk-2\nchunk-3\ndone\n", "chunks")));
const rawBody = Buffer.from(Array.from({ length: 70_000 }, (_, i) => (i * 31) % 251));
const rawSum = rawBody.reduce((sum, byte) => (sum + byte) % 65521, 0);
add("api-raw-body", page("/api/raw"), (o) => all(status(o, 200), is(o.body?.length === 70_000 && o.body?.sum === rawSum, `raw ${JSON.stringify(o.body)}`)),
  { method: "POST", headers: { "content-type": "application/octet-stream" }, body: rawBody });
add("api-body-limit", page("/api/limit"), (o) => status(o, 413), { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ a: "x".repeat(2000) }) });
add("api-dynamic", page("/api/items/abc?q=1"), (o) => all(status(o, 200), is(o.body?.id === "abc" && o.body?.query?.q === "1", "params")));
add("api-mutate-get-405", page("/api/mutate"), (o) => status(o, 405));
scenarios["api-mutate-once"] = {
  kind: "compare",
  run: async (base, side) => {
    const key = `pages-mutate-${ns}-${side}`;
    const r = await look(base, { path: page(`/api/mutate?key=${key}&id=1`), method: "POST" });
    const ledger = await fetch(`${upstream}/mutations?key=${key}`).then((x) => x.json());
    return { status: r.status, recorded: r.body?.recorded, ledger: ledger.ids };
  },
  expect: (o) => all(status(o, 200), is(o.recorded === 1 && JSON.stringify(o.ledger) === '["1"]', `ledger ${JSON.stringify(o.ledger)}`)),
};

// --- proxy, authentication and next.config rules -------------------------
const SESSION = "session=synthetic-valid-session";
add("proxy-protected-anon", page("/protected/secret"), (o) => all(status(o, 307), is(String(o.headers.location).includes("login=required"), "to login")));
add("proxy-protected-session", page("/protected/secret"), (o) => all(status(o, 200), is(o.body.secret === "top-secret-for-u-carol", "secret for the session user")),
  { headers: { cookie: `${SESSION}; user=u-carol` } });
add("proxy-protected-data-anon", data("/protected/secret.json"), (o) => all(status(o, 307), is(!JSON.stringify(o.body).includes("top-secret"), "no secret in the data route")));
add("proxy-protected-data-session", data("/protected/secret.json"), (o) => all(status(o, 200), is(o.body?.pageProps?.secret === "top-secret-for-u-carol", "data")),
  { headers: { cookie: `${SESSION}; user=u-carol` } });
add("proxy-protected-ssg-anon", page("/protected/static-secret"), (o) => all(status(o, 307), is(!JSON.stringify(o.body).includes("static-top-secret"), "no secret")));
add("proxy-protected-ssg-data-anon", data("/protected/static-secret.json"), (o) => all(status(o, 307), is(!JSON.stringify(o.body).includes("static-top-secret"), "no secret")));
add("proxy-protected-encoded", `${prefix}/%70rotected/secret`, (o) => is(!JSON.stringify(o.body).includes("top-secret"), "an encoded path must not reach the page without the proxy"));
add("proxy-protected-data-encoded", `${prefix}/_next/data/${buildId}/%70rotected/secret.json`, (o) => is(!JSON.stringify(o.body).includes("top-secret"), "no secret"));
add("proxy-protected-data-wrong-build", `${prefix}/_next/data/other/protected/secret.json`, (o) => is(!JSON.stringify(o.body).includes("top-secret"), "no secret"));
add("proxy-header-on-page", page("/books/3"), (o) => all(status(o, 200), is(o.headers["x-proxy-seen"] === "/books/3", "proxy ran")));
add("proxy-rewrite-page", page("/mw/rewrite-book"), (o) => all(status(o, 200), pageIs(o, "/books/[id]"), is(o.body.slug === "99", "rewritten")));
add("proxy-rewrite-ssg", page("/mw/rewrite-post"), (o) => all(status(o, 200), is(o.body.slug === "beta", "rewritten")));
add("proxy-redirect", page("/mw/redirect"), (o) => status(o, 307));
add("proxy-json", page("/mw/json"), (o) => all(status(o, 202), is(o.body?.fromProxy === true, "direct response")));
add("proxy-api", page("/api/echo"), (o) => all(status(o, 200), is(o.headers["x-proxy-seen"] === "/api/echo", "proxy ran")));
add("config-redirect", page("/old-about"), (o) => status(o, 308));
add("config-redirect-dynamic", page("/old-book/5"), (o) => all(status(o, 307), is(String(o.headers.location).endsWith("/books/5") || String(o.headers.location).endsWith("/books/5/"), "location")));
add("config-before-files", page("/bf-about"), (o) => all(status(o, 200), pageIs(o, "/about")));
add("config-after-files-ssr", page("/rw-book/5"), (o) => all(status(o, 200), pageIs(o, "/books/[id]"), is(o.body.slug === "5", "param")));
add("config-after-files-ssg", page("/rw-post/beta"), (o) => all(status(o, 200), is(o.body.slug === "beta", "rewritten")));
add("config-after-files-api", page("/rw-api/x/y"), (o) => all(status(o, 200), is(o.body?.query?.alias !== undefined, "alias")));
add("config-fallback", page("/fb/77"), (o) => all(status(o, 200), is(o.body.slug === "77", "fallback rewrite")));
add("config-headers", page("/about"), (o) => is(o.headers["x-config-about"] === "1", "config header"));
// A config rewrite into a protected path: the proxy matches the request's
// own path (/rw-secret), so next start renders the page without it. Both
// servers must agree; the assertion records next start's behaviour.
add("config-rewrite-into-protected", page("/rw-secret"), (o) => status(o, 200));

// --- Preview Mode ------------------------------------------------------------
async function enablePreview(base, user) {
  const r = await fetch(`${base}${page(`/api/preview?secret=preview-secret&user=${user}`)}`, { redirect: "manual" });
  await r.text();
  return r.headers.getSetCookie().map((c) => c.split(";")[0]).join("; ");
}
add("preview-bad-secret", page("/api/preview?secret=wrong"), (o) => all(status(o, 401), is(o.setCookie.length === 0, "no cookie")));
scenarios["preview-enable"] = {
  kind: "compare",
  run: async (base) => {
    const r = await look(base, { path: page("/api/preview?secret=preview-secret&user=u-dana") });
    return { ...r, setCookie: r.setCookie.map((c) => c.replace(/=[^;]*/, "=VALUE")) };
  },
  expect: (o) => all(status(o, 200), is(o.setCookie.length === 2, "two cookies")),
};
scenarios["preview-page"] = {
  kind: "compare",
  run: async (base) => unstamp(await look(base, { path: page("/preview"), headers: { cookie: await enablePreview(base, "u-dana") } })),
  expect: (o) => all(status(o, 200), is(o.body.kind === "preview" && o.body.secret === JSON.stringify({ user: "u-dana" }), `preview ${o.body.kind} ${o.body.secret}`),
    is(o.headers["cache-control"] === "private, no-cache, no-store, max-age=0, must-revalidate", "never cached")),
};
add("preview-off", page("/preview"), (o) => all(status(o, 200), is(o.body.kind === "published" && o.body.secret === "none", "published")));
add("preview-forged-cookie", page("/preview"), (o) => all(status(o, 200), is(o.body.kind === "published", "a forged cookie is not preview"), is(o.setCookie.length === 2, "cleared")),
  { headers: { cookie: "__prerender_bypass=forged; __next_preview_data=garbage" } });
scenarios["preview-data-route"] = {
  kind: "compare",
  run: async (base) => unstamp(await look(base, { path: data("/preview.json"), headers: { cookie: await enablePreview(base, "u-erin") } })),
  expect: (o) => all(status(o, 200), is(o.body?.pageProps?.preview === true && o.body?.pageProps?.data === JSON.stringify({ user: "u-erin" }), "preview data")),
};
// The other server's cookie: both servers use the build's preview keys, so
// a cookie one minted must decrypt in the other (pbkdf2, AES-GCM, HS256).
scenarios["preview-cross-server-cookie"] = {
  kind: "compare",
  run: async (base, side) => {
    const other = side === "baseline" ? candidate : baseline;
    return unstamp(await look(base, { path: page("/preview"), headers: { cookie: await enablePreview(other, "u-frank") } }));
  },
  expect: (o) => is(o.body.kind === "preview" && o.body.secret === JSON.stringify({ user: "u-frank" }), "decrypted the other server's preview cookie"),
};
scenarios["preview-clear"] = {
  kind: "compare",
  run: async (base) => look(base, { path: page("/api/clear-preview"), headers: { cookie: await enablePreview(base, "u-dana") } }),
  expect: (o) => all(status(o, 200), is(o.setCookie.length === 2 && o.setCookie.every((c) => /^__(prerender_bypass|next_preview_data)=;/.test(c)), "cleared")),
};

// --- ISR (one server at a time: they share the upstream counter) ---------
const generation = (o) => Number(/generation-(\d+)/.exec(o.body?.result ?? "")?.[1] ?? NaN);
scenarios["isr-stale-then-regenerated"] = {
  kind: "compare",
  run: async (base) => {
    await look(base, { path: page("/isr") });
    await sleep(2300);
    const stale = await look(base, { path: page("/isr") });
    let fresh;
    for (let i = 0; i < 20; i++) {
      await sleep(150);
      fresh = await look(base, { path: page("/isr") });
      if (generation(fresh) !== generation(stale)) break;
    }
    return { stale: stale.headers["x-nextjs-cache"], advanced: generation(fresh) > generation(stale), fresh: fresh.headers["x-nextjs-cache"], cacheControl: fresh.headers["cache-control"] };
  },
  expect: (o) => all(is(o.stale === "STALE", `expired page served ${o.stale}`), is(o.advanced, "regenerated in the background"), is(o.fresh === "HIT", `then ${o.fresh}`)),
};
scenarios["isr-failed-regeneration-keeps-page"] = {
  kind: "compare",
  run: async (base) => {
    // Settle first: a regeneration the previous scenario started may still
    // be running (slowly, on a busy machine) and would make the page fresh.
    let before = await look(base, { path: page("/isr") });
    for (let i = 0; i < 40 && before.headers["x-nextjs-cache"] !== "HIT"; i++) {
      await sleep(100);
      before = await look(base, { path: page("/isr") });
    }
    await fetch(`${upstream}/flag?name=pages-isr-fail&on=1`, { method: "POST" });
    try {
      await sleep(2600);
      const during = await look(base, { path: page("/isr") });
      await sleep(400);
      const again = await look(base, { path: page("/isr") });
      return { status: [during.status, again.status], kept: generation(again) === generation(before) || generation(again) === generation(during), cache: during.headers["x-nextjs-cache"] };
    } finally {
      await fetch(`${upstream}/flag?name=pages-isr-fail&on=0`, { method: "POST" });
    }
  },
  expect: (o) => all(is(o.status.every((s) => s === 200), `statuses ${o.status}`), is(o.kept, "the stored page stays when regeneration fails"), is(o.cache === "STALE", `cache ${o.cache}`)),
};
scenarios["isr-on-demand"] = {
  kind: "compare",
  run: async (base) => {
    const before = await look(base, { path: page("/on-demand") });
    const again = await look(base, { path: page("/on-demand") });
    const r = await look(base, { path: page("/api/revalidate?path=/on-demand") });
    const after = await look(base, { path: page("/on-demand") });
    const json = await look(base, { path: data("/on-demand.json") });
    return {
      stable: generation(before) === generation(again),
      revalidated: r.body,
      advanced: generation(after) > generation(again),
      dataMatches: json.body?.pageProps?.generation === generation(after),
      cache: after.headers["x-nextjs-cache"],
    };
  },
  expect: (o) => all(is(o.stable, "no regeneration without revalidation"), is(o.revalidated?.revalidated === true, `revalidate ${JSON.stringify(o.revalidated)}`), is(o.advanced, "regenerated"), is(o.dataMatches, "data route serves the same generation")),
};
add("isr-on-demand-missing", page("/api/revalidate?path=/does-not-exist"), (o) => is(o.body?.revalidated === false, "a missing page cannot be revalidated"));

// Whichever way it is spelled, a scenario name is stable across variants.
const failures = await runScenarios(`pages-${variant}`, scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
