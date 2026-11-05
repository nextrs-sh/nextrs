// Generates crates/nextrs-server/tests/data/routing-vectors.json: what
// Next's own routing functions answer for a corpus of rules and requests.
// tests/routing_vectors.rs checks NextRS's Rust port against it.
//
//   node scripts/routing-vectors.mjs [--next DIR]   (default: fixtures/next-proxy/node_modules/next)
//
// The functions are the ones next start's router calls at request time:
// getPathMatch with buildCustomRoute's options, matchHas, prepareDestination,
// stringifyQuery and Node's url.format (redirect Location), and
// getMiddlewareRouteMatcher (proxy matchers).
import { createRequire } from "node:module";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import url from "node:url";
import querystring from "node:querystring";

const root = resolve(dirname(url.fileURLToPath(import.meta.url)), "..");
const argv = process.argv.slice(2);
const nextDir = resolve(argv[argv.indexOf("--next") + 1] && argv.includes("--next") ? argv[argv.indexOf("--next") + 1] : join(root, "fixtures/next-proxy/node_modules/next"));
const require = createRequire(join(nextDir, "package.json"));
const nextVersion = require("./package.json").version;
const dist = (p) => require(join(nextDir, "dist", p));
const { getPathMatch } = dist("shared/lib/router/utils/path-match.js");
const { modifyRouteRegex } = dist("lib/redirect-status.js");
const { matchHas, prepareDestination, compileNonPath } = dist("shared/lib/router/utils/prepare-destination.js");
const { normalizeRepeatedSlashes } = dist("shared/lib/utils.js");
const { getMiddlewareRouteMatcher } = dist("shared/lib/router/utils/middleware-route-matcher.js");
const { getRelativeURL } = dist("shared/lib/router/utils/relativize-url.js");

// buildCustomRoute (filesystem.js): the regex as built at runtime.
function build(kind, item) {
  let regex = "";
  const match = getPathMatch(item.source, {
    strict: true,
    removeUnnamedParams: true,
    regexModifier: (r) => {
      if (!item.internal) r = modifyRouteRegex(r, kind === "redirect" ? ["/_next"] : undefined);
      regex = r;
      return r;
    },
    sensitive: false,
  });
  return { regex, match };
}

// stringifyQuery(req, query) from server-route-utils.js.
function stringifyQuery(initialQuery, query) {
  const values = Object.values(initialQuery);
  return querystring.stringify(query, undefined, undefined, {
    encodeURIComponent(value) {
      if (value in initialQuery || values.some((v) => (Array.isArray(v) ? v.includes(value) : v === value))) {
        return encodeURIComponent(value);
      }
      return value;
    },
  });
}

function parseQuery(search) {
  const query = {};
  for (const [k, v] of new URLSearchParams(search)) {
    if (query[k] === undefined) query[k] = v;
    else if (Array.isArray(query[k])) query[k].push(v);
    else query[k] = [query[k], v];
  }
  return query;
}

const rules = [
  { kind: "redirect", source: "/old-blog/:slug", destination: "/blog/:slug" },
  { kind: "redirect", source: "/wildcard/:path*", destination: "/blog/:path*" },
  { kind: "redirect", source: "/plus/:path+", destination: "/to/:path+?x=:path+" },
  { kind: "redirect", source: "/regex/:id(\\d{1,})", destination: "/blog/num-:id" },
  { kind: "redirect", source: "/opt/:a?", destination: "/o/:a?" },
  { kind: "redirect", source: "/(.*)/unnamed", destination: "/u" },
  { kind: "redirect", source: "/keep-query", destination: "/target?fixed=1&k=:k" },
  { kind: "redirect", source: "/q", has: [{ type: "query", key: "to", value: "(?<to>[a-z]+)" }], destination: "/blog/:to" },
  { kind: "redirect", source: "/h", has: [{ type: "header", key: "X-Who" }], destination: "/who/:xwho" },
  { kind: "redirect", source: "/host", has: [{ type: "host", value: "(?<sub>[a-z]+)\\.example\\.com" }], destination: "https://:sub.example.org/p" },
  { kind: "redirect", source: "/hostplain", has: [{ type: "host", value: "example.com" }], destination: "/hp/:host" },
  { kind: "redirect", source: "/c", has: [{ type: "cookie", key: "theme", value: "dark|light" }], missing: [{ type: "header", key: "x-skip" }], destination: "/c/:theme" },
  { kind: "redirect", source: "/ext/:path*", destination: "https://example.com/:path*?from=nextrs" },
  { kind: "redirect", source: "/hash/:slug", destination: "/h/:slug#top" },
  { kind: "redirect", source: "/:path+/", destination: "/:path+", internal: true },
  { kind: "redirect", source: "/:file((?!\\.well-known(?:/.*)?)(?:[^/]+/)*[^/]+\\.\\w+)/", destination: "/:file", internal: true },
  { kind: "redirect", source: "/:notfile((?!\\.well-known(?:/.*)?)(?:[^/]+/)*[^/\\.]+)", destination: "/:notfile/", internal: true },
  { kind: "redirect", source: "/_next/:path*", destination: "/blocked" },
  { kind: "rewrite", source: "/af/:slug", destination: "/blog/:slug" },
  { kind: "rewrite", source: "/af-api/:path*", destination: "/api/echo?alias=:path*" },
  { kind: "rewrite", source: "/af-query", destination: "/rewritten/q?fixed=1" },
  { kind: "rewrite", source: "/params/:a/:b", destination: "/p" },
  { kind: "rewrite", source: "/mixed/:a/:b", destination: "/m/:a" },
  { kind: "rewrite", source: "/any/(.*)", destination: "/any-target" },
  { kind: "rewrite", source: "/ext-rw/:path*", destination: "http://127.0.0.1:9/:path*" },
  { kind: "header", source: "/:path*", headers: [{ key: "x-a", value: "1" }] },
  { kind: "header", source: "/blog/:slug", headers: [{ key: "x-slug", value: ":slug" }, { key: "x-:slug-key", value: "v-:slug*" }] },
];

const paths = [
  "/old-blog/x", "/OLD-BLOG/x", "/old-blog/a%20b", "/old-blog/x/", "/old-blog/x/y", "/wildcard", "/wildcard/a/b/c",
  "/plus/a/b", "/plus", "/regex/42", "/regex/abc", "/opt", "/opt/z", "/zz/unnamed", "/keep-query", "/q", "/h",
  "/host", "/hostplain", "/c", "/ext/a/b", "/hash/s", "/a/b/", "/file.txt/", "/dir/sub", "/.well-known/x",
  "/_next/static/x", "/af/hello", "/af-api/a/b", "/af-query", "/params/1/2", "/mixed/1/2", "/any/x/y", "/ext-rw/x",
  "/blog/hello", "/", "/x%2Fy",
];
const requests = [
  { query: "", headers: {} },
  { query: "?to=abc&a=1&a=2", headers: { "x-who": "me", cookie: "theme=dark", host: "app.example.com" } },
  { query: "?k=v%20w&to=ABC", headers: { cookie: "theme=blue", host: "example.com:3000", "x-skip": "1" } },
  { query: "?slug=from-query&x=%2F", headers: { host: "127.0.0.1:5000" } },
];

const builtRules = rules.map((rule) => {
  const built = build(rule.kind, rule);
  return { ...rule, regex: built.regex, internal: !!rule.internal, has: rule.has ?? [], missing: rule.missing ?? [], destination: rule.destination ?? null, headers: rule.headers ?? [], match: built.match };
});
const vectors = [];
for (const [ruleIndex, rule] of builtRules.entries()) {
  for (const path of paths) {
    for (const [requestIndex, req] of requests.entries()) {
      const query = parseQuery(req.query);
      let params = rule.match(path);
      if (params && (rule.has.length || rule.missing.length)) {
        const extra = matchHas({ headers: req.headers }, query, rule.has, rule.missing);
        params = extra ? Object.assign(params, extra) : false;
      }
      const vector = { rule: ruleIndex, path, request: requestIndex, params: params || null };
      if (params && rule.headers.length) {
        const hasParams = Object.keys(params).length > 0;
        vector.headers = rule.headers.map(({ key, value }) =>
          hasParams ? [compileNonPath(key, params), compileNonPath(value, params)] : [key, value],
        );
      }
      if (params && rule.destination) {
        try {
          const appendParamsToQuery = rule.kind === "rewrite";
          const { parsedDestination } = prepareDestination({ appendParamsToQuery, destination: rule.destination, params: { ...params }, query: { ...query } });
          vector.dest = { pathname: parsedDestination.pathname, hostname: parsedDestination.hostname ?? null, hash: parsedDestination.hash ?? "", search: parsedDestination.search ?? "", query: parsedDestination.query, protocol: parsedDestination.protocol ?? null };
          if (rule.kind === "redirect") {
            const d = { ...parsedDestination };
            const q = d.query;
            delete d.query;
            d.search = stringifyQuery(query, q);
            d.pathname = normalizeRepeatedSlashes(d.pathname);
            vector.location = url.format(d);
          }
        } catch (error) {
          vector.destError = String(error.message);
        }
      }
      vectors.push(vector);
    }
  }
}

// Proxy matchers, as functions-config-manifest.json carries them.
const matcherCases = [
  { matchers: [{ regexp: "^(?:\\/(_next\\/data\\/[^/]{1,}))?(?:\\/((?!_next\\/static|_next\\/image|excluded|api\\/health).*))(\\.json|\\.rsc|\\.segments\\/.+\\.segment\\.rsc)?[\\/#\\?]?$" }] },
  { matchers: [{ regexp: "^(?:\\/(_next\\/data\\/[^/]{1,}))?\\/has-gate(?:\\/((?:[^\\/#\\?]+?)(?:\\/(?:[^\\/#\\?]+?))*))?(\\.json|\\.rsc|\\.segments\\/.+\\.segment\\.rsc)?[\\/#\\?]?$", has: [{ type: "header", key: "x-gate", value: "open" }], missing: [{ type: "cookie", key: "skip-proxy" }] }] },
];
const matcherPaths = ["/", "/x", "/excluded", "/excluded/a", "/api/health", "/api/healthz", "/_next/static/a.js", "/_next/image", "/has-gate", "/has-gate/a/b", "/HAS-GATE/a", "/_next/data/b1/has-gate/a.json", "/%70rotected", "/a.rsc"];
const matcherRequests = [{ headers: {}, query: {} }, { headers: { "x-gate": "open" }, query: {} }, { headers: { "x-gate": "open", cookie: "skip-proxy=1" }, query: {} }];
const matchers = [];
for (const [i, c] of matcherCases.entries()) {
  const fn = getMiddlewareRouteMatcher(c.matchers);
  for (const path of matcherPaths) {
    for (const req of matcherRequests) {
      matchers.push({ set: i, matchers: c.matchers, path, requestHeaders: req.headers, matches: fn(path, { headers: req.headers }, req.query) });
    }
  }
}

const relatives = [
  ["http://localhost:3000/target?x=1#h", "http://localhost:3000/proxy/rewrite"],
  ["http://127.0.0.1:3000/target", "http://localhost:3000/a"],
  ["/rel/path", "http://localhost:3000/a/b"],
  ["https://example.com/landing", "http://localhost:3000/a"],
  ["http://localhost:3000/a%20b?q=%2F", "http://localhost:3000/"],
].map(([u, b]) => ({ url: u, base: b, relative: getRelativeURL(u, b) }));

const out = join(root, "crates/nextrs-server/tests/data/routing-vectors.json");
mkdirSync(dirname(out), { recursive: true });
const ruleList = builtRules.map(({ match, ...rule }) => rule);
const lines = (items) => items.map((item) => `  ${JSON.stringify(item)}`).join(",\n");
writeFileSync(
  out,
  `{\"next\": ${JSON.stringify(nextVersion)},\n\"rules\": [\n${lines(ruleList)}\n],\n\"requests\": ${JSON.stringify(requests)},\n\"vectors\": [\n${lines(vectors)}\n],\n\"matchers\": [\n${lines(matchers)}\n],\n\"relatives\": ${JSON.stringify(relatives)}\n}\n`,
);
console.log(`${vectors.length} rule vectors, ${matchers.length} matcher vectors, ${relatives.length} relative URLs (Next.js ${nextVersion}) -> ${out}`);
