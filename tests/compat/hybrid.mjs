// App Router and Pages Router in one application (fixtures/next-hybrid):
// each router's pages, route handlers and API routes, data and RSC
// requests across the routers, the App Router not-found page answering
// for the Pages Router, rewrites between the routers, next/image, and one
// proxy in front of both. Compared with next start and asserted.
//
//   node tests/compat/hybrid.mjs --baseline URL --candidate URL --dist DIR [--json out.json]
import { readFileSync } from "node:fs";
import path from "node:path";
import { observe, parseArgs, runScenarios } from "./routing-lib.mjs";

const args = parseArgs();
const buildId = readFileSync(path.join(args.dist, "BUILD_ID"), "utf8").trim();
const only = args.only ? new Set(args.only.split(",")) : null;
const data = (p) => `/_next/data/${buildId}${p}`;
const SESSION = "session=synthetic-valid-session";

const is = (cond, message) => (cond ? null : message);
const all = (...checks) => checks.find((c) => c !== null) ?? null;
const status = (o, code) => is(o.status === code, `status ${o.status}, expected ${code}`);
const title = (o, t) => is(o.body?.title === t, `title ${o.body?.title}, expected ${t}`);
const json = (o) => (typeof o.body === "string" ? (() => { try { return JSON.parse(o.body); } catch { return null; } })() : o.body);

const scenarios = {};
const add = (name, request, expect) => {
  scenarios[name] = { kind: "compare", run: (base) => observe(base, typeof request === "string" ? { path: request } : request), expect };
};

add("app-static", "/app-page", (o) => all(status(o, 200), title(o, "App Router page")));
add("app-dynamic", { path: "/app-items/3", headers: { cookie: "user=u-alice" } }, (o) => all(status(o, 200), title(o, "App item 3"), is(o.body.secret === "user=u-alice", "cookie")));
add("app-route-handler", "/app-api?q=1", (o) => all(status(o, 200), is(json(o)?.router === "app" && json(o)?.q === "1", "route handler")));
// An RSC request without its _rsc cache-busting parameter is redirected
// to one (next start's check, for both routers' paths).
add("app-rsc", { path: "/app-page", headers: { rsc: "1" } }, (o) => all(status(o, 307), is(String(o.headers.location).includes("_rsc"), "to the cache-busting URL")));
add("pages-static", "/pages-page", (o) => all(status(o, 200), title(o, "Pages Router page")));
add("pages-ssr", { path: "/pages-items/5", headers: { cookie: "user=u-bob" } }, (o) => all(status(o, 200), title(o, "Pages item 5"), is(o.body.secret === "user=u-bob", "cookie")));
add("pages-ssg", "/pages-ssg", (o) => all(status(o, 200), is(o.body.kind === "pages-ssg-build", "props")));
add("pages-api", "/api/pages-health?q=2", (o) => all(status(o, 200), is(json(o)?.router === "pages" && json(o)?.q === "2", "API route")));
add("app-api-health", "/api/health", (o) => status(o, 200));
add("pages-rsc-header", { path: "/pages-page", headers: { rsc: "1" } }, (o) => status(o, 307));
add("pages-ssr-rsc-header", { path: "/pages-items/1", headers: { rsc: "1" } }, (o) => status(o, 307));
add("pages-api-rsc-header", { path: "/api/pages-health", headers: { rsc: "1" } }, (o) => status(o, 200));
add("pages-post-405", { path: "/pages-page", method: "POST" }, (o) => status(o, 405));
add("pages-data-ssr", data("/pages-items/5.json"), (o) => all(status(o, 200), is(json(o)?.pageProps?.id === "5", "data")));
add("pages-data-ssg", data("/pages-ssg.json"), (o) => all(status(o, 200), is(json(o)?.pageProps?.built === "build", "data")));
// With a proxy, a data path routes as the page it names (here an App
// Router page, served as such).
add("data-for-app-page", data("/app-page.json"), (o) => all(status(o, 200), title(o, "App Router page")));
add("data-missing", data("/missing.json"), (o) => status(o, 404));
// The App Router not-found page answers for both routers.
add("not-found-unmatched", "/no-such-page", (o) => all(status(o, 404), title(o, "App Router not found")));
add("not-found-from-pages-gssp", "/pages-items/missing", (o) => all(status(o, 404), title(o, "App Router not found")));
add("not-found-pages-data", data("/pages-items/missing.json"), (o) => all(status(o, 404), is(json(o)?.notFound === true, "notFound")));
add("rewrite-into-pages", "/to-pages/8", (o) => all(status(o, 200), title(o, "Pages item 8")));
// NextRS encodes images with its own engine (compared pixel by pixel in
// tests/compat/image.mjs): here, the status, type and dimensions.
scenarios["image-optimizer"] = {
  kind: "compare",
  run: async (base) => {
    const r = await fetch(`${base}/_next/image?url=%2Fswatch.png&w=64&q=75`, { headers: { accept: "image/png" } });
    const bytes = Buffer.from(await r.arrayBuffer());
    const png = bytes.subarray(1, 4).toString() === "PNG";
    return { status: r.status, type: r.headers.get("content-type"), width: png ? bytes.readUInt32BE(16) : null, height: png ? bytes.readUInt32BE(20) : null };
  },
  expect: (o) => all(status(o, 200), is(o.type === "image/png" && o.width === 64 && o.height === 48, `image ${JSON.stringify(o)}`)),
};
add("proxy-app-anon", "/secure/app-secret", (o) => all(status(o, 307), is(!JSON.stringify(o.body).includes("top-secret"), "no secret")));
add("proxy-pages-anon", "/secure/pages-secret", (o) => all(status(o, 307), is(!JSON.stringify(o.body).includes("top-secret"), "no secret")));
add("proxy-pages-data-anon", data("/secure/pages-secret.json"), (o) => all(status(o, 307), is(!JSON.stringify(o.body).includes("top-secret"), "no secret")));
add("proxy-app-rsc-anon", { path: "/secure/app-secret", headers: { rsc: "1" } }, (o) => is(!JSON.stringify(o.body).includes("top-secret"), "no secret"));
add("proxy-app-session", { path: "/secure/app-secret", headers: { cookie: SESSION } }, (o) => all(status(o, 200), is(o.body.secret === "app-top-secret", "secret")));
add("proxy-pages-session", { path: "/secure/pages-secret", headers: { cookie: SESSION } }, (o) => all(status(o, 200), is(o.body.secret === "pages-top-secret", "secret")));
add("proxy-pages-data-session", { path: data("/secure/pages-secret.json"), headers: { cookie: SESSION } }, (o) => all(status(o, 200), is(json(o)?.pageProps?.secret === "pages-top-secret", "data")));

const failures = await runScenarios("hybrid", scenarios, { baseline: args.baseline, candidate: args.candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
