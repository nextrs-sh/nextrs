// assetPrefix and caseSensitiveRoutes on fixtures/next-proxy (builds
// NEXTRS_VARIANT=asset-prefix and case-sensitive), compared between next
// start and NextRS: where the prefixed asset URLs the HTML references are
// served, and how case variants of pages, config rules and proxy paths
// route. The proxy, routing-config and routing-vector suites run on the
// same builds.
//
//   node tests/compat/config-flags.mjs --baseline URL --candidate URL --variant asset-prefix|case-sensitive [--json out.json]
import { observe, parseArgs, runScenarios } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, variant } = args;
if (!baseline || !candidate || !["asset-prefix", "case-sensitive"].includes(variant)) {
  console.error("usage: config-flags.mjs --baseline URL --candidate URL --variant asset-prefix|case-sensitive [--json FILE]");
  process.exit(2);
}
const scenarios = {};
const get = (name, path, headers = {}, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (base) => observe(base, { path, headers, ...extra }) };
};

if (variant === "asset-prefix") {
  // The asset URLs the page references (with the prefix), and the same
  // files without it.
  const html = await (await fetch(`${baseline}/target`)).text();
  const assets = [...new Set(html.match(/\/static-cdn\/_next\/static\/[^"']+?\.(?:js|css)/g) ?? [])].slice(0, 3);
  scenarios["html-references-the-prefix"] = {
    kind: "compare",
    run: async (base) => {
      const page = await (await fetch(`${base}/target`)).text();
      return { prefixed: /\/static-cdn\/_next\/static\//.test(page), unprefixed: /"\/_next\/static\//.test(page) };
    },
  };
  assets.forEach((asset, i) => {
    get(`asset-with-prefix-${i}`, asset);
    get(`asset-without-prefix-${i}`, asset.replace("/static-cdn", ""));
    get(`asset-with-prefix-head-${i}`, asset, {}, { method: "HEAD" });
  });
  get("prefix-missing-asset", "/static-cdn/_next/static/missing.js");
  get("prefix-page-path", "/static-cdn/target");
  get("prefix-rsc", "/target", { rsc: "1" });
} else {
  for (const path of ["/target", "/Target", "/TARGET", "/blog/hello", "/Blog/hello", "/blog/Hello", "/old-blog/x", "/Old-Blog/x", "/af/x", "/AF/x", "/proxy/redirect", "/Proxy/redirect", "/protected", "/Protected", "/api/echo", "/API/echo", "/excluded", "/Excluded"]) {
    get(`case${path.replace(/\//g, "-")}`, path);
  }
}

const failures = await runScenarios(`config-flags (${variant})`, scenarios, { baseline, candidate, json: args.json });
process.exit(failures ? 1 : 0);
