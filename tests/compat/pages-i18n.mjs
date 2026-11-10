// Next's native i18n routing (Pages Router): fixtures/next-pages-i18n
// under next start (the oracle) and NextRS, compared response by response.
// Every scenario also asserts its expected outcome on its own (locale props,
// __NEXT_DATA__, statuses, redirects, ledgers), so an answer both servers
// got wrong is still caught (ORACLE-FAIL).
//
//   node tests/compat/pages-i18n.mjs --baseline URL --candidate URL --upstream URL
//        --dist fixtures/next-pages-i18n/.next
//        [--variant default|no-detect|base-path|domains] [--json out.json] [--only a,b]
//
// Requests go out through node:http (fetch cannot set Host, which the
// domain variant needs). Normalized away: per-request stamps, the ISR
// generation (each server counts its own), ETags of stamped bodies.
import http from "node:http";
import { readFileSync } from "node:fs";
import path from "node:path";
import { parseArgs, runScenarios } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, upstream } = args;
const variant = args.variant ?? "default";
const buildId = readFileSync(path.join(args.dist, "BUILD_ID"), "utf8").trim();
const prefix = variant === "base-path" ? "/base" : "";
const slash = variant === "base-path";
const proxy = variant !== "no-detect";
const detection = variant !== "no-detect";
const domains = variant === "domains";
const only = args.only ? new Set(args.only.split(",")) : null;
const ns = `i${Date.now().toString(36)}`;
const SESSION = "session=synthetic-valid-session";

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
const volatile = (text) => text.replace(/"(stamp|built)":\d+/g, '"$1":0').replace(/built=(<!-- -->)?\d+/g, "built=N").replace(/generation=\d+/g, "generation=N").replace(/"generation":\d+/g, '"generation":0');

function summarize(html) {
  const out = {};
  for (const id of ["title", "kind", "locale", "locales", "default-locale", "secret", "query", "resolved", "slug", "info", "built", "fallback"]) {
    const m = new RegExp(`id="${id}">([\\s\\S]*?)</`).exec(html);
    if (m) out[id] = volatile(decode(m[1].replace(/<!-- -->/g, "")));
  }
  const next = /<script id="__NEXT_DATA__" type="application\/json">([\s\S]*?)<\/script>/.exec(html);
  if (next) {
    const d = JSON.parse(next[1]);
    out.next = {
      page: d.page,
      query: d.query,
      buildId: d.buildId === buildId ? "BUILD" : d.buildId,
      locale: d.locale,
      locales: d.locales,
      defaultLocale: d.defaultLocale,
      domainLocales: d.domainLocales,
      isFallback: d.isFallback ?? false,
      gssp: d.gssp ?? false,
      gsp: d.gsp ?? false,
      isPreview: d.isPreview ?? false,
      props: JSON.parse(volatile(JSON.stringify(d.props ?? {}))),
    };
  }
  out.app = html.includes('data-app="nextrs-i18n-app"');
  return out;
}

function request(base, { method = "GET", path: p, headers = {}, body }) {
  return new Promise((resolve, reject) => {
    const url = new URL(base);
    const req = http.request(
      { host: url.hostname, port: url.port, path: p, method, agent: false, headers: { "accept-encoding": "identity", ...headers } },
      (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, text: Buffer.concat(chunks).toString("utf8") }));
        res.on("error", reject);
      },
    );
    req.on("error", reject);
    if (body != null) req.write(body);
    req.end();
  });
}

async function look(base, req) {
  let r;
  try {
    r = await request(base, req);
  } catch (error) {
    return { error: String(error?.code ?? error?.message ?? error) };
  }
  const port = new URL(base).port;
  const stamped = /"(stamp|built)":\d+|generation=\d+|"generation":\d+/.test(r.text);
  const h = {};
  for (const [name, raw] of Object.entries(r.headers)) {
    if (SKIP.has(name) || name === "set-cookie") continue;
    if (name === "etag" && stamped) continue;
    let v = (Array.isArray(raw) ? raw.join(", ") : raw).replace(SELF, "http://SELF").replace(new RegExp(`^(127\\.0\\.0\\.1|localhost):${port}$`), "SELF-HOST");
    if (name === "vary") {
      // next start compresses (Vary: Accept-Encoding); NextRS does not.
      v = v.split(/,\s*/).filter((t) => t.toLowerCase() !== "accept-encoding").join(", ");
      if (!v) continue;
    }
    h[name] = v;
  }
  const type = String(r.headers["content-type"] ?? "").split(";")[0];
  let body;
  if (type === "text/html") body = summarize(r.text);
  else if (type === "application/json" || /^\{[\s\S]*\}$/.test(r.text.trim())) {
    try {
      body = JSON.parse(volatile(r.text));
    } catch {
      body = volatile(r.text);
    }
  } else body = volatile(r.text.replace(SELF, "http://SELF"));
  const cookies = r.headers["set-cookie"] ?? [];
  return {
    status: r.status,
    headers: h,
    setCookie: cookies.map((c) => c.replace(/Expires=[^;]+/i, "Expires=X").replace(/Max-Age=\d+/i, "Max-Age=X").replace(/=([^;]{20,})/, "=TOKEN")),
    body,
  };
}

// Assertions independent of the comparison.
const is = (cond, message) => (cond ? null : message);
const all = (...checks) => checks.find((c) => c !== null) ?? null;
const status = (o, code) => is(o.status === code, `status ${o.status}, expected ${code}`);
const pageIs = (o, name) => is(o.body?.next?.page === name, `rendered ${o.body?.next?.page ?? "no page"}, expected ${name}`);
// The locale the page rendered with, in its props, its __NEXT_DATA__ and
// (for pages with data functions) the locale context the function got.
const localeIs = (o, locale, defaultLocale = "en") =>
  all(
    is(o.body?.next?.locale === locale, `__NEXT_DATA__.locale ${o.body?.next?.locale}, expected ${locale}`),
    is(o.body?.locale === undefined || o.body.locale === locale, `page locale ${o.body?.locale}, expected ${locale}`),
    is(o.body?.next?.defaultLocale === defaultLocale, `defaultLocale ${o.body?.next?.defaultLocale}, expected ${defaultLocale}`),
    is(JSON.stringify(o.body?.next?.locales) === '["en","fr","ja"]', `locales ${JSON.stringify(o.body?.next?.locales)}`),
  );
const dataLocale = (o, locale) => is(o.body?.pageProps?.locale === locale, `data locale ${o.body?.pageProps?.locale}, expected ${locale}`);
const NOT_FOUND = { en: "Not found", fr: "Introuvable", ja: "見つかりません" };
const notFoundIn = (o, locale, defaultLocale = "en") =>
  all(status(o, 404), pageIs(o, "/404"), is(o.body?.title === NOT_FOUND[locale], `404 page "${o.body?.title}", expected the ${locale} one`), localeIs(o, locale, defaultLocale));
const location = (o, expected) => is(o.headers?.location === expected, `Location ${o.headers?.location}, expected ${expected}`);
const redirect = (o, code, expected) => all(status(o, code), location(o, expected));
const noSecret = (o) => is(!JSON.stringify(o.body ?? "").includes("secret-for"), "the protected page's data reached the client");

const scenarios = {};
const add = (name, p, expect, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (base) => look(base, { path: p, ...extra }), expect };
};
const lang = (value, more = {}) => ({ headers: { "accept-language": value, ...more } });

// --- locale detection at the root ----------------------------------------
const root = prefix ? `${prefix}/` : "/";
const rootRedirect = (locale) => `${prefix}/${locale}${slash ? "/" : ""}`;
if (domains) {
  // A domain's default locale wins over Accept-Language and the cookie;
  // a preference for another domain's locale goes to that domain.
  // Upstream: a page rendered per request reports the domain's default
  // locale as defaultLocale; a prerendered one the config's (it was
  // rendered at build time).
  add("domain-root-own-default", "/", (o) => all(status(o, 200), localeIs(o, "fr", "en")), lang("fr", { host: "example.fr" }));
  add("domain-root-other-domain", "/", (o) => redirect(o, 307, "https://example.fr/"), lang("fr", { host: "example.com" }));
  add("domain-root-http-domain", "/", (o) => redirect(o, 307, "http://example.jp/"), lang("ja", { host: "example.com:3000" }));
  add("domain-root-cookie-ignored", "/", (o) => all(status(o, 200), localeIs(o, "fr", "en")), { headers: { host: "example.fr", cookie: "NEXT_LOCALE=ja" } });
  add("domain-root-unknown-host", "/", (o) => redirect(o, 307, "/fr"), lang("fr", { host: "elsewhere.test" }));
  add("domain-page-default", "/about", (o) => all(status(o, 200), localeIs(o, "fr", "en")), { headers: { host: "example.fr" } });
  add("domain-page-prefixed", "/ja/about", (o) => all(status(o, 200), localeIs(o, "ja", "en")), { headers: { host: "example.fr" } });
  add("domain-ssr-own-default", "/ssr", (o) => all(status(o, 200), localeIs(o, "fr", "fr")), { headers: { host: "example.fr" } });
  add("domain-ssr-default", "/ssr", (o) => all(status(o, 200), localeIs(o, "ja", "ja"), is(o.headers["x-ssr-locale"] === "ja", "getServerSideProps locale")), { headers: { host: "EXAMPLE.JP:8080" } });
  add("domain-data-default", data("/ssr.json"), (o) => all(status(o, 200), dataLocale(o, "fr")), { headers: { host: "example.fr" } });
  add("domain-404", "/nope", (o) => notFoundIn(o, "fr", "en"), { headers: { host: "example.fr" } });
  add("domain-forwarded-host-ignored", "/ssr", (o) => all(status(o, 200), localeIs(o, "en")), { headers: { host: "elsewhere.test", "x-forwarded-host": "example.fr" } });
}
if (detection) {
  add("root-accept-language", root, (o) => redirect(o, 307, rootRedirect("fr")), lang("fr-FR,fr;q=0.9"));
  add("root-accept-language-q", root, (o) => redirect(o, 307, rootRedirect("ja")), lang("de, ja;q=0.5, fr;q=0.4"));
  add("root-accept-language-q0", root, (o) => redirect(o, 307, rootRedirect("fr")), lang("ja;q=0, fr;q=0.1"));
  add("root-accept-language-region", root, (o) => all(status(o, 200), localeIs(o, "en")), lang("fr-CA"));
  add("root-accept-language-malformed", root, (o) => all(status(o, 200), localeIs(o, "en")), lang("fr;x=1"));
  add("root-accept-language-default", root, (o) => all(status(o, 200), localeIs(o, "en")), lang("en-US,en;q=0.8"));
  add("root-cookie", root, (o) => redirect(o, 307, rootRedirect("ja")), { headers: { cookie: "NEXT_LOCALE=JA", "accept-language": "fr" } });
  add("root-cookie-default", root, (o) => all(status(o, 200), localeIs(o, "en")), { headers: { cookie: "NEXT_LOCALE=en", "accept-language": "fr" } });
  add("root-cookie-invalid", root, (o) => redirect(o, 307, rootRedirect("fr")), { headers: { cookie: "NEXT_LOCALE=de", "accept-language": "fr" } });
  add("root-query-kept", `${root}?a=1&b=%20x&c`, (o) => redirect(o, 307, `${rootRedirect("fr")}?a=1&b=%20x&c`), lang("fr"));
  add("root-index", page("/index"), (o) => redirect(o, 307, rootRedirect("fr")), lang("fr"));
  add("root-not-for-pages", page("/about"), (o) => all(status(o, 200), localeIs(o, "en")), lang("fr"));
  add("root-locale-path", page("/fr"), (o) => all(status(o, 200), localeIs(o, "fr")), lang("ja"));
  add("root-post", root, (o) => redirect(o, 307, rootRedirect("fr")), { method: "POST", ...lang("fr") });
  if (prefix) add("root-outside-base-path", "/", (o) => redirect(o, 307, rootRedirect("fr")), lang("fr"));
} else {
  add("no-detect-accept-language", root, (o) => all(status(o, 200), localeIs(o, "en")), lang("fr"));
  add("no-detect-cookie", root, (o) => all(status(o, 200), localeIs(o, "en")), { headers: { cookie: "NEXT_LOCALE=ja" } });
}

// --- locale prefixes -----------------------------------------------------
add("prefix-default", page("/en"), (o) => all(status(o, 200), localeIs(o, "en"), is(o.body.title === "Hello", "greeting")));
add("prefix-fr", page("/fr"), (o) => all(status(o, 200), localeIs(o, "fr"), is(o.body.title === "Bonjour", "greeting")));
add("prefix-ja", page("/ja"), (o) => all(status(o, 200), localeIs(o, "ja"), is(o.body.title === "こんにちは", "greeting")));
add("prefix-case", page("/FR/about"), (o) => all(status(o, 200), localeIs(o, "fr")));
add("prefix-encoded", page("/%66r/about"), (o) => notFoundIn(o, "en"));
add("prefix-unknown", page("/de/about"), (o) => notFoundIn(o, "en"));
if (!slash) {
  add("trailing-slash-locale", "/fr/", (o) => redirect(o, 308, "/fr"));
  add("trailing-slash-default", "/en/about/", (o) => redirect(o, 308, "/about"));
  add("trailing-slash-other", "/fr/about/", (o) => redirect(o, 308, "/fr/about"));
} else {
  add("trailing-slash-missing", `${prefix}/fr/about`, (o) => redirect(o, 308, `${prefix}/fr/about/`));
  add("trailing-slash-default", `${prefix}/en/about`, (o) => redirect(o, 308, `${prefix}/about/`));
}

// --- pages under each locale ---------------------------------------------
add("static-default", page("/about"), (o) => all(status(o, 200), pageIs(o, "/about"), localeIs(o, "en"), is(o.body.app, "custom App")));
add("static-fr", page("/fr/about"), (o) => all(status(o, 200), pageIs(o, "/about"), localeIs(o, "fr")));
add("static-ja-query", page("/ja/about?x=1"), (o) => all(status(o, 200), localeIs(o, "ja")));
add("static-post-405", page("/fr/about"), (o) => status(o, 405), { method: "POST" });
add("ssr-default", page("/ssr?x=1"), (o) => all(status(o, 200), localeIs(o, "en"), is(o.body.query === '{"x":"1"}', `query ${o.body.query}`), is(o.body.resolved === `/ssr${slash ? "/" : ""}?x=1`, `resolvedUrl ${o.body.resolved}`)));
add("ssr-fr", page("/fr/ssr?x=1&x=2"), (o) => all(status(o, 200), localeIs(o, "fr"), is(o.headers["x-ssr-locale"] === "fr", "context.locale"), is(o.body.query === '{"x":["1","2"]}', `query ${o.body.query}`)));
add("ssr-cookie", page("/ja/ssr"), (o) => all(status(o, 200), localeIs(o, "ja"), is(o.body.secret === "user=alice", `identity ${o.body.secret}`)), { headers: { cookie: "user=alice" } });
add("ssr-dynamic", page("/fr/items/a%20b"), (o) => all(status(o, 200), pageIs(o, "/items/[id]"), localeIs(o, "fr"), is(o.body.slug === "a b", `param ${o.body.slug}`)));
add("ssr-not-found", page("/ja/items/missing"), (o) => notFoundIn(o, "ja"));
add("ssr-redirect", page("/fr/items/moved"), (o) => all(status(o, 307), is(o.headers.location?.endsWith("/about"), `Location ${o.headers.location}`)));
add("ssg-paths-listed", page("/fr/posts/hello"), (o) => all(status(o, 200), pageIs(o, "/posts/[slug]"), localeIs(o, "fr"), is(o.headers["x-nextjs-cache"] === "HIT", `cache ${o.headers["x-nextjs-cache"]}`)));
add("ssg-paths-default-locale", page("/posts/bonjour"), (o) => all(status(o, 200), localeIs(o, "en"), is(o.headers["x-nextjs-cache"] === "HIT", "listed without a locale: the default's")));
add("ssg-paths-unlisted", page("/ja/posts/hello"), (o) => all(status(o, 200), localeIs(o, "ja"), is(o.body.slug === "hello", "slug"), is(o.body.fallback === "false", "blocking fallback renders the page")));
add("ssg-paths-not-found", page("/fr/posts/missing"), (o) => notFoundIn(o, "fr"));
add("ssg-paths-redirect", page("/ja/posts/gone"), (o) => all(status(o, 307), is(o.headers.location?.endsWith("/about"), `Location ${o.headers.location}`)));

// --- data routes ----------------------------------------------------------
add("data-index-fr", data("/fr.json"), (o) => all(status(o, 200), dataLocale(o, "fr"), is(o.body.pageProps.greeting === "Bonjour", "greeting")));
add("data-ssr-fr", data("/fr/ssr.json"), (o) => all(status(o, 200), dataLocale(o, "fr"), is(o.body.__N_SSP === true, "SSP data")), { headers: { "x-nextjs-data": "1" } });
add("data-ssg-fr", data("/fr/posts/hello.json"), (o) => all(status(o, 200), dataLocale(o, "fr"), is(o.headers["x-nextjs-matched-path"] === "/fr/posts/[slug]", `matched ${o.headers["x-nextjs-matched-path"]}`)));
add("data-ssg-ja-unlisted", data("/ja/posts/hello.json"), (o) => all(status(o, 200), dataLocale(o, "ja")));
add("data-dynamic-ssr", data("/ja/items/7.json"), (o) => all(status(o, 200), dataLocale(o, "ja"), is(o.body.pageProps.id === "7", "param")));
add("data-unknown-locale", data("/de/ssr.json"), (o) => status(o, 404));
add("data-wrong-build", `${prefix}/_next/data/not-this-build/fr/ssr.json`, (o) => status(o, 404));
if (proxy) {
  // With a proxy, a data path without a locale routes under the default.
  add("data-no-locale", data("/ssr.json"), (o) => all(status(o, 200), dataLocale(o, "en")));
  add("data-index-no-locale", data("/index.json"), (o) => all(status(o, 200), dataLocale(o, "en")));
} else {
  // Without one, base server renders the 404 page for it.
  add("data-no-locale", data("/ssr.json"), (o) => all(status(o, 404), is(o.body?.pageProps?.text === "Not found", "the 404 page's data")));
  add("data-index-no-locale", data("/index.json"), (o) => status(o, 404));
}

// --- localized 404 ---------------------------------------------------------
add("not-found-default", page("/nope"), (o) => notFoundIn(o, "en"));
add("not-found-fr", page("/fr/nope"), (o) => notFoundIn(o, "fr"));
add("not-found-ja-deep", page("/ja/a/b/c"), (o) => notFoundIn(o, "ja"));
add("not-found-unknown-locale", page("/de/nope"), (o) => notFoundIn(o, "en"));

// --- API routes (no locale) -------------------------------------------------
add("api-default", page("/api/whoami?q=1"), (o) => all(status(o, 200), is(o.body?.method === "GET", "handler ran"), is(o.body?.query?.q === "1", "query")));
add("api-under-locale", page("/fr/api/whoami"), (o) => notFoundIn(o, "fr"));
add("api-under-default-locale", page("/en/api/whoami"), (o) => notFoundIn(o, "en"));
add("api-private-unauthorized", page("/api/private"), (o) => all(status(o, 401), is(!JSON.stringify(o.body).includes("api-secret"), "no secret")));
add("api-private-under-locale", page("/fr/api/private"), (o) => all(status(o, 404), is(!JSON.stringify(o.body).includes("api-secret"), "no secret")), { headers: { cookie: SESSION } });
scenarios["api-mutation-once"] = {
  kind: "compare",
  run: async (base, side) => {
    const key = `${ns}-${side}-mutate`;
    const r = await look(base, { method: "POST", path: page(`/api/mutate?key=${key}&id=1`) });
    const under = await look(base, { method: "POST", path: page(`/fr/api/mutate?key=${key}&id=2`) });
    const ledger = await fetch(`${upstream}/mutations?key=${key}`).then((x) => x.json());
    return { status: r.status, count: r.body?.count, under: under.status, ids: ledger.ids };
  },
  expect: (o) => all(is(o.status === 200 && o.count === 1, `status ${o.status}, count ${o.count}`), is(o.under === 404, `under a locale: ${o.under}`), is(JSON.stringify(o.ids) === '["1"]', `ledger ${JSON.stringify(o.ids)}: the mutation ran once, the localized one never`)),
};

// --- next.config rules -------------------------------------------------------
add("config-header", page("/fr/about"), (o) => all(status(o, 200), is(o.headers["x-config-about"] === "1", "header rule"), is(o.headers["x-config-fr-about"] === "1", "locale: false header")));
add("config-header-default", page("/about"), (o) => all(is(o.headers["x-config-about"] === "1", "header rule"), is(!o.headers["x-config-fr-about"], "locale: false rule is fr only")));
// Next writes a redirect's destination as configured (basePath, no
// trailing slash). With domains it compiles one more rule per domain: a
// locale's paths redirect to its domain, without the locale.
const dest = (locale, p) => (domains ? `${{ en: "https://example.com", fr: "https://example.fr", ja: "http://example.jp" }[locale]}${p}` : `${prefix}${locale === "en" ? "" : `/${locale}`}${p}`);
add("config-redirect-default", page("/old-about"), (o) => redirect(o, 307, dest("en", "/about")));
add("config-redirect-locale", page("/fr/old-about"), (o) => redirect(o, 307, dest("fr", "/about")));
add("config-redirect-param", page("/ja/old-post/hello"), (o) => redirect(o, 308, dest("ja", "/posts/hello")));
add("config-redirect-no-locale", page("/fr/ancien"), (o) => status(o, 307));
add("config-redirect-no-locale-default", page("/ancien"), (o) => notFoundIn(o, "en"));
add("config-rewrite-before", page("/fr/bf-about"), (o) => all(status(o, 200), pageIs(o, "/about"), localeIs(o, "fr")));
add("config-rewrite-after", page("/ja/rw-ssr"), (o) => all(status(o, 200), localeIs(o, "ja"), is(o.body.query === '{"from":"rewrite"}', `query ${o.body.query}`)));
add("config-rewrite-ssg", page("/fr/rw-post/hello"), (o) => all(status(o, 200), pageIs(o, "/posts/[slug]"), localeIs(o, "fr")));
add("config-rewrite-fallback", page("/ja/fb/hello"), (o) => all(status(o, 200), pageIs(o, "/posts/[slug]"), localeIs(o, "ja")));
if (!prefix) {
  add("config-rewrite-changes-locale", "/en/to-fr", (o) => all(status(o, 200), localeIs(o, "fr"), is(o.headers["x-ssr-locale"] === "fr", "the destination's locale")));
  add("config-rewrite-changes-locale-default", "/to-fr", (o) => all(status(o, 200), localeIs(o, "fr")));
}

// --- the proxy and authorization ---------------------------------------------
// Upstream (Next 16.4.3): with basePath and i18n together, runMiddleware
// builds the proxy's URL as `/<locale><routed path>` and the routed path
// still starts with the basePath. Next's adapter recovers the pathname for
// the default locale only: for another locale the proxy sees `/base/fr/ssr`
// as its pathname, and a `/protected` check in it never matches. NextRS
// gives the proxy the same URL; the page's own session check protects it.
const proxyPath = (locale, p) => (prefix && locale !== "en" ? `${prefix}/${locale}${p}` : p);
if (proxy) {
  add("proxy-locale-seen", page("/fr/ssr"), (o) => all(status(o, 200), is(o.headers["x-proxy-seen"] === `fr:${proxyPath("fr", "/ssr")}`, `proxy saw ${o.headers["x-proxy-seen"]}`)));
  add("proxy-default-seen", page("/ssr"), (o) => is(o.headers["x-proxy-seen"] === `en:${proxyPath("en", "/ssr")}`, `proxy saw ${o.headers["x-proxy-seen"]}`));
  add("proxy-api-locale-seen", page("/fr/api/whoami"), (o) => is(o.headers["x-proxy-seen"] === `fr:${proxyPath("fr", "/api/whoami")}`, `proxy saw ${o.headers["x-proxy-seen"]}`));
  add("proxy-data-seen", data("/ja/ssr.json"), (o) => is(o.headers["x-proxy-seen"] === `ja:${proxyPath("ja", "/ssr")}`, `proxy saw ${o.headers["x-proxy-seen"]}`));
  add("proxy-protected-redirect", page("/fr/protected/secret"), (o) =>
    all(status(o, 307), prefix ? location(o, `${prefix}/about?login=page`) : location(o, "/fr/about?login=required"), noSecret(o)));
  add("proxy-protected-case", page("/FR/protected/secret"), (o) => all(status(o, 307), noSecret(o)));
  add("proxy-protected-session", page("/ja/protected/secret"), (o) => all(status(o, 200), is(o.body.secret === `secret-for-ja proxy=${prefix ? "-" : "verified"}`, `secret ${o.body.secret}`)), { headers: { cookie: SESSION } });
  add("proxy-protected-data", data("/fr/protected/secret.json"), (o) => all(is(o.status === 307 || o.status === 200, `status ${o.status}`), noSecret(o)));
  add("proxy-protected-data-session", data("/fr/protected/secret.json"), (o) => all(status(o, 200), is(o.body?.pageProps?.secret === "secret-for-fr", "secret with a session")), { headers: { cookie: SESSION } });
  if (!prefix) {
    add("proxy-rewrite-locale", page("/fr/mw/rewrite-ssr"), (o) => all(status(o, 200), localeIs(o, "fr"), is(o.body.query === '{"from":"proxy"}', `query ${o.body.query}`)));
    add("proxy-rewrite-to-ja", page("/mw/to-ja"), (o) => all(status(o, 200), localeIs(o, "ja")));
  } else {
    add("proxy-rewrite-unmatched-base-path", page("/fr/mw/rewrite-ssr"), (o) => notFoundIn(o, "fr"));
  }
  // next/link prefetches a link the proxy matches with x-middleware-prefetch:
  // next start answers {} for a page that is not statically generated,
  // without running getServerSideProps.
  add("prefetch-ssr-skipped", data("/fr/ssr.json"), (o) => all(status(o, 200), is(o.headers["x-middleware-skip"] === "1", "x-middleware-skip"), is(JSON.stringify(o.body) === "{}", `body ${JSON.stringify(o.body)}`), is(!o.headers["x-ssr-locale"], "getServerSideProps ran")), { headers: { "x-middleware-prefetch": "1", "x-nextjs-data": "1" } });
  add("prefetch-static-skipped", page("/ja/about"), (o) => all(status(o, 200), is(o.headers["x-middleware-skip"] === "1", "x-middleware-skip")), { headers: { "x-middleware-prefetch": "1" } });
  add("prefetch-ssg-served", data("/fr/posts/hello.json"), (o) => all(status(o, 200), dataLocale(o, "fr")), { headers: { "x-middleware-prefetch": "1", "x-nextjs-data": "1" } });
}
// Bypass attempts: encoded, unknown-locale and rewritten paths never get
// the page's data without a session (the page checks it itself: a config
// rewrite into it skips the proxy, in next start too).
add("bypass-encoded", page("/fr/%70rotected/secret"), (o) => all(status(o, 404), noSecret(o)));
add("bypass-unknown-locale", page("/de/protected/secret"), (o) => all(status(o, 404), noSecret(o)));
add("bypass-data-encoded", data("/fr/%70rotected/secret.json"), (o) => noSecret(o));
add("bypass-config-rewrite", page("/fr/rw-secret"), (o) => all(status(o, 307), noSecret(o), is(o.headers.location?.includes("login=page"), "the page's own check redirected")));
add("bypass-config-rewrite-session", page("/fr/rw-secret"), (o) => all(status(o, 200), is(o.body.secret?.startsWith("secret-for-fr proxy=-"), `the proxy did not run for the rewrite: ${o.body.secret}`)), { headers: { cookie: SESSION } });
add("bypass-internal-header", page("/fr/protected/secret"), (o) => all(noSecret(o), is(o.status !== 200, `status ${o.status}`)), { headers: { "x-proxy-user": "verified", "x-middleware-subrequest": "middleware:middleware:middleware:middleware:middleware", "x-invoke-path": "/fr/protected/secret" } });

// --- ISR per locale ----------------------------------------------------------
const generationOf = async (base, p) => {
  const r = await request(base, { path: p });
  return Number(/generation=(?:<!-- -->)?(\d+)/.exec(r.text)?.[1] ?? NaN);
};
const settle = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// When the page was rendered (changes with every regeneration).
const builtOf = async (base, p) => {
  const r = await request(base, { path: p });
  return Number(/built=(?:<!-- -->)?(\d+)/.exec(r.text)?.[1] ?? NaN);
};
const changed = (a, b) => Number.isFinite(a) && Number.isFinite(b) && a !== b;
scenarios["isr-per-locale"] = {
  kind: "compare",
  run: async (base) => {
    // Revalidate 2 s: after it, a request serves the stale page and
    // regenerates that locale's entry only.
    await request(base, { path: page("/fr/isr") });
    await request(base, { path: page("/ja/isr") });
    await settle(2300);
    const stale = await look(base, { path: page("/fr/isr") });
    await settle(800);
    const fr = await generationOf(base, page("/fr/isr"));
    const ja1 = await generationOf(base, page("/ja/isr"));
    const fr2 = await generationOf(base, page("/fr/isr"));
    const title = await look(base, { path: page("/ja/isr") });
    return { stale: stale.headers["x-nextjs-cache"], frStable: fr === fr2, frNumber: Number.isFinite(fr), jaNumber: Number.isFinite(ja1), jaTitle: title.body?.title, frLocale: stale.body?.next?.locale };
  },
  expect: (o) => all(is(o.stale === "STALE", `cache ${o.stale}`), is(o.frStable && o.frNumber && o.jaNumber, "generations"), is(o.jaTitle === "ISR ja", `ja page ${o.jaTitle}: locales share no entry`), is(o.frLocale === "fr", "fr page")),
};
scenarios["isr-on-demand-locale"] = {
  kind: "compare",
  run: async (base) => {
    const fr = await builtOf(base, page("/fr/on-demand"));
    const ja = await builtOf(base, page("/ja/on-demand"));
    const r = await look(base, { path: page("/api/revalidate?path=/fr/on-demand") });
    const frAfter = await builtOf(base, page("/fr/on-demand"));
    const jaAfter = await builtOf(base, page("/ja/on-demand"));
    const frData = await look(base, { path: data("/fr/on-demand.json") });
    return { revalidated: r.body?.revalidated, frAdvanced: changed(fr, frAfter), jaSame: Number.isFinite(ja) && ja === jaAfter, dataLocale: frData.body?.pageProps?.locale };
  },
  expect: (o) => all(is(o.revalidated === "/fr/on-demand", `revalidate ${o.revalidated}`), is(o.frAdvanced, "the fr page regenerated"), is(o.jaSame, "the ja page kept its render"), is(o.dataLocale === "fr", "fr data")),
};
scenarios["isr-on-demand-default"] = {
  kind: "compare",
  run: async (base) => {
    const en = await builtOf(base, page("/on-demand"));
    const fr = await builtOf(base, page("/fr/on-demand"));
    const r = await look(base, { path: page("/api/revalidate?path=/on-demand") });
    const enAfter = await builtOf(base, page("/en/on-demand"));
    const frAfter = await builtOf(base, page("/fr/on-demand"));
    return { revalidated: r.body?.revalidated, enAdvanced: changed(en, enAfter), frSame: Number.isFinite(fr) && fr === frAfter };
  },
  expect: (o) => all(is(o.revalidated === "/on-demand", `revalidate ${o.revalidated}`), is(o.enAdvanced, "a path without a locale revalidates the default locale"), is(o.frSame, "fr untouched")),
};

// --- Preview Mode -----------------------------------------------------------
scenarios["preview-per-locale"] = {
  kind: "compare",
  run: async (base) => {
    const enable = await request(base, { path: page("/api/preview?who=editor") });
    const cookies = (enable.headers["set-cookie"] ?? []).map((c) => c.split(";")[0]).join("; ");
    const fr = await look(base, { path: page("/fr/preview"), headers: { cookie: cookies } });
    const ja = await look(base, { path: page("/ja/preview"), headers: { cookie: cookies } });
    const plain = await look(base, { path: page("/fr/preview") });
    const forged = await look(base, { path: page("/fr/preview"), headers: { cookie: "__prerender_bypass=forged; __next_preview_data=forged" } });
    const frData = await look(base, { path: data("/fr/preview.json"), headers: { cookie: cookies } });
    return { cookies: cookies.includes("__prerender_bypass"), fr: fr.body?.info, frLocale: fr.body?.next?.locale, ja: ja.body?.info, plain: plain.body?.info, plainCache: plain.headers["x-nextjs-cache"], forged: forged.body?.info, data: frData.body?.pageProps?.preview };
  },
  expect: (o) =>
    all(
      is(o.cookies, "preview cookies set"),
      is(o.fr === 'preview=true data={"who":"editor"}' && o.frLocale === "fr", `fr preview ${o.fr}`),
      is(o.ja?.startsWith("preview=true"), `ja preview ${o.ja}`),
      is(o.plain === "preview=false data=-" && o.plainCache === "HIT", `without cookies: ${o.plain} (${o.plainCache}): preview output never reaches the cache`),
      is(o.forged === "preview=false data=-", `forged cookie: ${o.forged}`),
      is(o.data === true, "preview data route"),
    ),
};

// --- isolation between locales and identities -------------------------------
scenarios["concurrent-locales"] = {
  kind: "compare",
  run: async (base) => {
    const locales = ["en", "fr", "ja"];
    const jobs = [];
    for (let i = 0; i < 90; i++) {
      const locale = locales[i % 3];
      const user = `u${i}`;
      const p = locale === "en" && i % 2 ? page(`/ssr?i=${i}`) : page(`/${locale}/ssr?i=${i}`);
      jobs.push(request(base, { path: p, headers: { cookie: `user=${user}` } }).then((r) => ({ locale, user, i, text: r.text, status: r.status })));
    }
    const results = await Promise.all(jobs);
    const wrong = results.filter((r) => {
      const locale = /id="locale">([^<]*)</.exec(r.text)?.[1];
      const secret = /id="secret">([^<]*)</.exec(r.text)?.[1];
      const query = /id="query">([^<]*)</.exec(r.text)?.[1] ?? "";
      return r.status !== 200 || locale !== r.locale || secret !== `user=${r.user}` || !decode(query).includes(`"i":"${r.i}"`);
    });
    return { total: results.length, wrong: wrong.length };
  },
  expect: (o) => is(o.total === 90 && o.wrong === 0, `${o.wrong} of ${o.total} responses had another request's locale, identity or query`),
};
scenarios["cache-no-locale-leak"] = {
  kind: "compare",
  run: async (base) => {
    const out = {};
    for (const locale of ["en", "fr", "ja"]) {
      const r = await look(base, { path: data(`/${locale}/preview.json`) });
      out[locale] = r.body?.pageProps?.locale;
      const html = await look(base, { path: page(`/${locale}/posts/hello`) });
      out[`${locale}-post`] = html.body?.next?.locale;
    }
    return out;
  },
  expect: (o) => is(Object.entries(o).every(([k, v]) => k.startsWith(v)), `entries served for the wrong locale: ${JSON.stringify(o)}`),
};

const failures = await runScenarios(`pages-i18n-${variant}`, scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
