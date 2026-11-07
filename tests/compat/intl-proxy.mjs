// Locale routing with next-intl's middleware (the library's documented App
// Router setup: createMiddleware(routing) in proxy.ts, a [locale] segment,
// no Next.js i18n config), compared between next start and NextRS on
// fixtures/next-intl-proxy.
//
//   node tests/compat/intl-proxy.mjs --baseline URL --candidate URL --upstream URL --dist DIR \
//        [--only a,b] [--json out.json]
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { observe, parseArgs, runScenarios } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, dist } = args;
if (!baseline || !candidate || !dist) {
  console.error("usage: intl-proxy.mjs --baseline URL --candidate URL --dist DIR [--json FILE]");
  process.exit(2);
}
const only = args.only ? new Set(args.only.split(",")) : null;

const manifest = JSON.parse(readFileSync(join(dist, "server/server-reference-manifest.json"), "utf8"));
const greet = Object.entries(manifest.node).find(([, v]) => v.exportedName === "greet")?.[0];
if (!greet) throw new Error("no greet action in the build");

// next-intl answers alternate links per locale: the servers' own origins
// in them are normalized by observe() (self URLs).
const scenarios = {};
const get = (name, path, headers = {}, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (base) => observe(base, { path, headers, ...extra }) };
};

// --- Detection and redirects ------------------------------------------------
get("root-no-preference", "/");
get("root-accept-language-de", "/", { "accept-language": "de-DE,de;q=0.9,en;q=0.5" });
get("root-accept-language-unsupported", "/", { "accept-language": "ja-JP,ja;q=0.9" });
get("root-accept-language-weighted", "/", { "accept-language": "es;q=0.9, fr;q=0.8, de;q=0.1" });
get("root-cookie-fr", "/", { cookie: "NEXT_LOCALE=fr" });
get("root-cookie-overrides-header", "/", { cookie: "NEXT_LOCALE=de", "accept-language": "fr" });
get("root-cookie-unsupported", "/", { cookie: "NEXT_LOCALE=xx" });
get("unprefixed-static", "/about");
get("unprefixed-static-de", "/about", { "accept-language": "de" });
get("unprefixed-dynamic", "/products/7");
get("unprefixed-query-kept", "/search?q=caf%C3%A9&page=2&page=3");
get("unprefixed-rsc", "/about", { rsc: "1" });
get("unprefixed-head", "/about", {}, { method: "HEAD" });

// --- Prefixed paths --------------------------------------------------------------
get("prefixed-default", "/en");
get("prefixed-de", "/de");
get("prefixed-fr-static", "/fr/about");
get("prefixed-de-dynamic-param", "/de/products/42");
get("prefixed-de-dynamic-encoded-param", "/de/products/a%20b");
get("prefixed-search-query", "/de/search?q=k%C3%A4se");
get("prefixed-header-ignored", "/fr/about", { "accept-language": "de" });
get("prefixed-sets-no-cookie-on-match", "/de/about", { cookie: "NEXT_LOCALE=de" });
get("prefixed-switch-cookie", "/fr/about", { cookie: "NEXT_LOCALE=de" });
get("prefixed-trailing-slash", "/de/about/");
get("prefixed-upper-case-locale", "/DE/about");
get("unsupported-locale", "/xx/about");
get("unsupported-locale-root", "/xx");
get("not-found-under-locale", "/de/nope");

// --- RSC navigation and prefetch ----------------------------------------------------
get("rsc-static", "/de/about", { rsc: "1" });
get("rsc-prefetch", "/fr", { rsc: "1", "next-router-prefetch": "1" });
get("rsc-dynamic", "/de/products/9", { rsc: "1", "next-url": "/de" });
get("rsc-unprefixed-redirect", "/products/9", { rsc: "1", "next-url": "/en" });

// --- Not matched by the matcher -------------------------------------------------------
get("api-not-matched", "/api/health");
get("file-not-matched", "/robots.txt");

// --- Cached routes do not bypass the middleware ----------------------------------------
scenarios["prerendered-twice"] = {
  kind: "compare",
  run: async (base) => ({ first: await observe(base, { path: "/de/about" }), second: await observe(base, { path: "/de/about" }) }),
};

// --- A locale-aware Server Action ------------------------------------------------------
function action(base, path, headers = {}) {
  const form = new FormData();
  form.append("1_name", "Ada");
  form.append("0", '["$K1"]');
  return { method: "POST", path, body: form, headers: { origin: base, "next-action": greet, accept: "text/x-component", ...headers } };
}
for (const locale of ["en", "de", "fr"]) {
  scenarios[`action-${locale}`] = { kind: "compare", run: (base) => observe(base, action(base, `/${locale}/actions`)) };
}
scenarios["action-unprefixed"] = { kind: "compare", run: (base) => observe(base, action(base, "/actions", { "accept-language": "de" })) };

// --- Concurrent requests in different locales --------------------------------------------
scenarios["concurrent-locales"] = {
  kind: "compare",
  run: async (base) => {
    const locales = ["en", "de", "fr"];
    let wrong = 0;
    await Promise.all(
      Array.from({ length: 90 }, async (_, i) => {
        const locale = locales[i % 3];
        const r = await fetch(`${base}/${locale}/products/${i}`);
        const html = await r.text();
        const kind = /id="kind">([^<]*)</.exec(html)?.[1];
        const slug = /id="slug">([^<]*)</.exec(html)?.[1];
        const lang = /<html lang="([^"]*)"/.exec(html)?.[1];
        if (r.status !== 200 || kind !== locale || slug !== String(i) || lang !== locale) wrong++;
      }),
    );
    return { wrong };
  },
};

const failures = await runScenarios("intl-proxy (next-intl)", scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
