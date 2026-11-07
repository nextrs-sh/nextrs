// next-intl's other routing modes, on builds of fixtures/next-intl-proxy
// (NEXTRS_VARIANT=as-needed | pathnames | domains, see i18n/routing.ts),
// compared between next start and NextRS. Each scenario also asserts what
// next-intl's documentation says the mode does (served locale, redirect
// target), so both servers agreeing on another answer fails.
//
//   node tests/compat/intl-variants.mjs --variant as-needed --baseline URL --candidate URL [--json out.json]
import { observe, parseArgs, runScenarios } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, variant } = args;
if (!baseline || !candidate || !["as-needed", "pathnames", "domains"].includes(variant)) {
  console.error("usage: intl-variants.mjs --variant as-needed|pathnames|domains --baseline URL --candidate URL [--json FILE]");
  process.exit(2);
}
const only = args.only ? new Set(args.only.split(",")) : null;

// Expectations.
const served = (locale, extra = {}) => (o) => {
  if (o.status !== 200) return `status ${o.status} location ${o.headers?.location}, expected 200 in ${locale}`;
  if (o.body?.kind !== locale) return `locale ${o.body?.kind}, expected ${locale}`;
  if (extra.slug !== undefined && o.body?.slug !== extra.slug) return `slug ${o.body?.slug}, expected ${extra.slug}`;
  return null;
};
const searched = (query) => (o) => (o.status === 200 && o.body?.query === JSON.stringify(query) ? null : `status ${o.status} query ${o.body?.query}`);
const redirects = (location) => (o) =>
  [307, 308].includes(o.status) && o.headers.location === location ? null : `status ${o.status} location ${o.headers?.location}, expected a redirect to ${location}`;
const status = (...codes) => (o) => (codes.includes(o.status) ? null : `status ${o.status}, expected ${codes.join(" or ")}`);
const alternates = (...hrefs) => (o) => {
  const link = o.headers?.link ?? "";
  const missing = hrefs.filter((h) => !link.includes(`<${h}>`));
  return missing.length ? `link header lacks ${missing.join(", ")}: ${link}` : null;
};
const all = (...checks) => (o) => checks.map((c) => c(o)).find(Boolean) ?? null;

// A domain's URLs carry the port the server listens on (next-intl keeps
// the request's port): each server's own, normalized.
async function look(base, request) {
  const o = await observe(base, request);
  const port = new URL(base).port;
  const own = (v) => (typeof v === "string" ? v.replaceAll(`.example.de:${port}`, ".example.de:PORT").replaceAll(`.example.com:${port}`, ".example.com:PORT") : v);
  if (o.headers) for (const k of Object.keys(o.headers)) o.headers[k] = own(o.headers[k]);
  o.body = own(o.body);
  return o;
}
const scenarios = {};
const get = (name, path, headers, expect, extra = {}) => {
  scenarios[name] = { kind: "compare", expect, run: (base) => look(base, { path, headers, ...extra }) };
};

if (variant === "as-needed") {
  get("root-default-unprefixed", "/", {}, served("en"));
  get("root-detects-de", "/", { "accept-language": "de-DE,de;q=0.9" }, redirects("/de"));
  get("root-cookie-fr", "/", { cookie: "NEXT_LOCALE=fr" }, redirects("/fr"));
  get("default-unprefixed-static", "/about", {}, served("en"));
  get("default-unprefixed-dynamic", "/products/9", {}, served("en", { slug: "9" }));
  get("default-prefix-removed", "/en/about", {}, redirects("/about"));
  get("default-prefix-removed-root", "/en", {}, redirects("/"));
  get("default-prefix-removed-query", "/en/search?q=a&page=2", {}, redirects("/search?q=a&page=2"));
  get("other-locale-prefixed", "/de/about", {}, served("de"));
  get("other-locale-dynamic", "/fr/products/a%20b", {}, served("fr", { slug: "a%20b" }));
  get("other-locale-search", "/de/search?q=k%C3%A4se", {}, searched({ q: "käse" }));
  get("unprefixed-cookie-redirects", "/about", { cookie: "NEXT_LOCALE=de" }, redirects("/de/about"));
  get("unprefixed-header-redirects", "/about", { "accept-language": "fr" }, redirects("/fr/about"));
  get("prefix-wins-over-cookie", "/de/about", { cookie: "NEXT_LOCALE=fr" }, served("de"));
  get("unsupported-locale", "/xx/about", {}, status(404));
  // A flight request for an unprefixed path is redirected too.
  get("rsc-unprefixed", "/about", { rsc: "1" }, status(307));
  get("head-unprefixed", "/about", {}, status(200), { method: "HEAD" });
  get("alternate-links", "/about", {}, alternates("http://SELF/about", "http://SELF/de/about", "http://SELF/fr/about"));
}

if (variant === "pathnames") {
  get("localized-de-about", "/de/ueber-uns", {}, served("de"));
  get("localized-fr-about", "/fr/a-propos", {}, served("fr"));
  get("canonical-en-about", "/en/about", {}, served("en"));
  get("internal-path-redirects-de", "/de/about", {}, redirects("/de/ueber-uns"));
  get("internal-path-redirects-fr", "/fr/about", {}, redirects("/fr/a-propos"));
  get("localized-dynamic-de", "/de/produkte/5", {}, served("de", { slug: "5" }));
  get("localized-dynamic-fr-encoded", "/fr/produits/a%20b", {}, served("fr", { slug: "a%20b" }));
  get("internal-dynamic-redirects", "/de/products/5", {}, redirects("/de/produkte/5"));
  get("localized-search-query", "/de/suche?q=k%C3%A4se", {}, searched({ q: "käse" }));
  get("internal-search-redirects-query", "/fr/search?q=x&p=2", {}, redirects("/fr/recherche?q=x&p=2"));
  get("unprefixed-detects-de", "/about", { "accept-language": "de" }, redirects("/de/ueber-uns"));
  get("unprefixed-cookie-fr", "/products/3", { cookie: "NEXT_LOCALE=fr" }, redirects("/fr/produits/3"));
  get("root-detects-fr", "/", { "accept-language": "fr" }, redirects("/fr"));
  // Another locale's pathname under this locale: not a route of it.
  get("foreign-pathname", "/de/a-propos", {}, status(307, 404));
  get("unprefixed-localized-path", "/ueber-uns", {}, status(307, 404));
  get("alternate-links-localized", "/de/ueber-uns", {}, alternates("http://SELF/en/about", "http://SELF/de/ueber-uns", "http://SELF/fr/a-propos"));
  get("rsc-localized", "/de/ueber-uns", { rsc: "1" }, status(200, 307));
}

if (variant === "domains") {
  // next-intl reads the host from X-Forwarded-Host or Host.
  const host = (h) => ({ host: h });
  get("de-domain-root", "/", host("shop.example.de"), served("de"), { raw: true });
  get("de-domain-unprefixed", "/about", host("shop.example.de"), served("de"), { raw: true });
  get("de-domain-other-locale", "/en/about", host("shop.example.de"), served("en"), { raw: true });
  get("de-domain-default-prefix-removed", "/de/about", host("shop.example.de"), redirects("http://shop.example.de:PORT/about"), { raw: true });
  get("com-domain-unprefixed", "/about", host("shop.example.com"), served("en"), { raw: true });
  get("com-domain-fr", "/fr/products/2", host("shop.example.com"), served("fr", { slug: "2" }), { raw: true });
  // A locale the domain does not serve: next-intl's answer, compared.
  get("com-domain-foreign-locale", "/de/about", host("shop.example.com"), status(200, 307, 404), { raw: true });
  get("forwarded-host-de", "/about", { "x-forwarded-host": "shop.example.de" }, served("de"));
  get("unknown-host-default", "/about", {}, served("en"));
  get("de-domain-detects-en", "/", { host: "shop.example.de", "accept-language": "en" }, status(200, 307), { raw: true });
}

const failures = await runScenarios(`intl-variants (next-intl ${variant})`, scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
