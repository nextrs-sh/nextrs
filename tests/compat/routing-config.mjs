// next.config headers, redirects and rewrites (applied by NextRS in Rust
// from routes-manifest.json), compared with next start on
// fixtures/next-proxy, in each of its builds (default, basePath "/base",
// trailingSlash: true). The fixture also has a proxy, so these scenarios
// cover where config rules sit around it.
//
//   node tests/compat/routing-config.mjs --baseline URL --candidate URL \
//        [--variant default|base-path|trailing-slash] [--only a,b] [--json out.json]
import { observe, parseArgs, runScenarios } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate } = args;
if (!baseline || !candidate) {
  console.error("usage: routing-config.mjs --baseline URL --candidate URL [--variant V] [--json FILE]");
  process.exit(2);
}
const variant = args.variant ?? "default";
const only = args.only ? new Set(args.only.split(",")) : null;
const base = variant.startsWith("base-path") ? "/base" : "";
// With basePath, every rule source is under it.
const b = (path) => (path === "/" && base ? base : `${base}${path}`);

const scenarios = {};
const get = (name, path, headers = {}, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (url) => observe(url, { path, headers, ...extra }) };
};
const knownDiff = (name, reason, request) => {
  scenarios[name] = { kind: "known-diff", reason, run: (url) => observe(url, request) };
};

// --- headers ------------------------------------------------------------------
get("header-all-paths", b("/target"));
get("header-param", b("/blog/hello"));
get("header-param-on-demand", b("/blog/zz-config"));
get("header-has-query-absent", b("/static"));
get("header-has-query-present", b("/static?h=1"));
get("header-missing-header-present", b("/target"), { "x-no-header": "1" });
get("header-set-cookie", b("/target"));
get("header-cookie-capture", b("/cookie-header"), { cookie: "theme=dark" });
get("header-cookie-capture-nomatch", b("/cookie-header"), { cookie: "theme=blue" });
get("header-vs-proxy-conflict", b("/proxy/json"));
get("header-on-404", b("/nope-config"));
get("header-on-public-file", b("/asset.txt"));
get("header-on-static-asset-miss", b("/_next/static/nope.js"));

// --- redirects ------------------------------------------------------------------
get("redirect-permanent-param", b("/old-blog/x"));
get("redirect-temporary", b("/temp-redirect"));
get("redirect-case-insensitive", b("/TEMP-Redirect"));
get("redirect-status-301", b("/status-redirect"));
get("redirect-has-query-capture", b("/query-redirect?to=abc"));
get("redirect-has-query-nomatch", b("/query-redirect?to=ABC1"));
get("redirect-keeps-query", b("/keep-query?a=1&a=2&b=%20"));
get("redirect-external", b("/external-redirect/x/y?q=1"));
get("redirect-before-proxy", b("/proxy/config-wins"));
get("redirect-cookie-present", b("/cookie-redirect"), { cookie: "redir=1" });
get("redirect-cookie-absent", b("/cookie-redirect"));
get("redirect-host", b("/host-redirect"));
get("redirect-host-other", b("/host-redirect"), { host: "example.com" });
get("redirect-wildcard", b("/wildcard/a/b/c"));
get("redirect-wildcard-empty", b("/wildcard"));
get("redirect-regex-param", b("/regex/42"));
get("redirect-regex-nomatch", b("/regex/abc"));
get("redirect-missing", b("/missing-redirect"));
get("redirect-missing-present", b("/missing-redirect"), { "x-stay": "1" });
get("redirect-post", b("/temp-redirect"), { "content-type": "text/plain" }, { method: "POST", body: "x" });
get("redirect-head", b("/old-blog/x"), {}, { method: "HEAD" });
get("redirect-encoded-param", b("/old-blog/a%20b"));

// --- rewrites ------------------------------------------------------------------
get("before-files", b("/bf-static"));
get("before-files-overrides-page", b("/static?bf=1"));
get("after-files-does-not-override-page", b("/static"));
get("after-files-param", b("/af/hello"));
get("after-files-on-demand", b("/af/zz-af"));
get("after-files-chain", b("/af-chain"));
get("after-files-isr", b("/af-isr"));
get("after-files-route-handler", b("/af-api/a/b?z=1"));
get("after-files-fixed-query", b("/af-query?x=1&fixed=2"));
get("after-files-to-protected", b("/af-protected"));
get("after-files-rsc", b("/af/hello"), { rsc: "1" });
get("after-files-head", b("/af/hello"), {}, { method: "HEAD" });
get("fallback-param", b("/fb/a/b"));
get("fallback-not-before-dynamic", b("/blog/zz-fb"));
get("no-rule-404", b("/nothing/here"));
knownDiff(
  "external-rewrite",
  "the destination (a closed port) is outside this run's --net-allow: NextRS answers 502 external-rewrite-denied and sends nothing, next start proxies (connection refused: 500). Allowed destinations are compared in tests/compat/platform-proxy.mjs",
  { path: b("/external-rewrite/x") },
);

// --- normalization, trailing slash and basePath -------------------------------------
get("trailing-slash-page", b("/target/"));
get("trailing-slash-dynamic", b("/blog/hello/"));
get("no-trailing-slash-page", b("/target"));
get("trailing-slash-file", b("/asset.txt/"));
get("well-known", b("/.well-known/x"));
get("double-slash", `${base}//target`, {}, { raw: true });
get("dot-segment", `${base}/x/../target`, {}, { raw: true });
get("root", b("/"));
get("root-slash", `${base}/`, {}, { raw: true });
if (base) {
  get("outside-base-path", "/target");
  get("base-path-prefix-only", "/basex/target");
  get("base-path-static-asset-outside", "/_next/static/nope.js");
}

const failures = await runScenarios(`routing-config (${variant})`, scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
