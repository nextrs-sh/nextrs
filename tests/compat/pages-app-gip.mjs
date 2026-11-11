// A custom App with getInitialProps (fixtures/next-pages-app-gip) under
// next start (the oracle) and NextRS: the App's and the page's initial
// props, request headers and cookies in the App's context, async work,
// getServerSideProps and getStaticProps pages beside it, data routes, a
// getInitialProps redirect, the error page and concurrent identities.
// Every scenario also asserts its outcome on its own.
//
//   node tests/compat/pages-app-gip.mjs --baseline URL --candidate URL
//        --dist fixtures/next-pages-app-gip/.next [--json out.json] [--only a,b]
import { readFileSync } from "node:fs";
import path from "node:path";
import { parseArgs, runScenarios } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate } = args;
const buildId = readFileSync(path.join(args.dist, "BUILD_ID"), "utf8").trim();
const only = args.only ? new Set(args.only.split(",")) : null;
const data = (p) => `/_next/data/${buildId}${p}`;

const SKIP = new Set(["date", "connection", "keep-alive", "transfer-encoding", "content-encoding", "content-length", "last-modified", "accept-ranges", "etag"]);
const decode = (s) => s.replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
const volatile = (text) => text.replace(/"stamp":\d+/g, '"stamp":0').replace(/"data":\d+/g, '"data":0');

function summarize(html) {
  const out = {};
  for (const id of ["title", "app-user", "app-theme", "app-data", "app-side", "app-path", "page-query", "page-side", "page-theme", "page-when", "error-side"]) {
    const m = new RegExp(`id="${id}">([\\s\\S]*?)</`).exec(html);
    if (m) out[id] = decode(m[1].replace(/<!-- -->/g, ""));
  }
  const next = /<script id="__NEXT_DATA__" type="application\/json">([\s\S]*?)<\/script>/.exec(html);
  if (next) {
    const d = JSON.parse(next[1]);
    out.next = { page: d.page, query: d.query, appGip: d.appGip ?? false, gip: d.gip ?? false, gssp: d.gssp ?? false, gsp: d.gsp ?? false, err: d.err ? { name: d.err.name, statusCode: d.err.statusCode } : null, props: JSON.parse(volatile(JSON.stringify(d.props ?? {}))) };
  }
  out.app = html.includes('data-app="nextrs-app-gip"');
  return out;
}

async function look(base, { method = "GET", path: p, headers = {} }) {
  let r;
  try {
    r = await fetch(`${base}${p}`, { method, headers: { "accept-encoding": "identity", ...headers }, redirect: "manual" });
  } catch (error) {
    return { error: String(error?.cause?.code ?? error?.message ?? error) };
  }
  const text = await r.text();
  const h = {};
  for (const [name, value] of r.headers) {
    if (SKIP.has(name) || name === "set-cookie") continue;
    if (name === "vary") {
      const v = value.split(/,\s*/).filter((t) => t.toLowerCase() !== "accept-encoding").join(", ");
      if (v) h[name] = v;
      continue;
    }
    h[name] = value;
  }
  const type = (r.headers.get("content-type") ?? "").split(";")[0];
  let body;
  if (type === "text/html") body = summarize(text);
  else {
    try {
      body = JSON.parse(volatile(text));
    } catch {
      body = volatile(text).slice(0, 200);
    }
  }
  return { status: r.status, headers: h, body };
}

const is = (cond, message) => (cond ? null : message);
const all = (...checks) => checks.find((c) => c !== null) ?? null;
const status = (o, code) => is(o.status === code, `status ${o.status}, expected ${code}`);
const appRan = (o, { user = "-", theme = "-" } = {}) =>
  all(
    is(o.body?.app, "custom App missing"),
    is(o.body?.next?.appGip === true, "__NEXT_DATA__.appGip"),
    is(o.body?.["app-side"] === "server", `App getInitialProps ran on ${o.body?.["app-side"]}`),
    is(o.body?.["app-user"] === user, `App saw user ${o.body?.["app-user"]}, expected ${user}`),
    is(o.body?.["app-theme"] === theme, `App saw theme ${o.body?.["app-theme"]}, expected ${theme}`),
    is(o.body?.["app-data"] === "data-ok", `App async data ${o.body?.["app-data"]}`),
  );

const scenarios = {};
const add = (name, p, expect, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (base) => look(base, { path: p, ...extra }), expect };
};

add("home", "/", (o) => all(status(o, 200), appRan(o), is(o.body.next?.page === "/", "page"), is(o.body.title === "Home", "title")));
add("home-identity", "/", (o) => all(status(o, 200), appRan(o, { user: "alice", theme: "dark" })), { headers: { "x-test-user": "alice", cookie: "theme=dark" } });
add("page-gip", "/gip?q=1&q=2", (o) =>
  all(status(o, 200), appRan(o), is(o.body.next?.gip === true, "page gip"), is(o.body["page-side"] === "server", "page getInitialProps on the server"), is(o.body["page-query"] === '{"q":["1","2"]}', `page query ${o.body["page-query"]}`), is(o.body["app-path"] === "/gip?q=1&q=2", `asPath ${o.body["app-path"]}`)));
add("ssr-with-app-gip", "/ssr", (o) => all(status(o, 200), appRan(o, { theme: "light" }), is(o.body.next?.gssp === true, "gssp"), is(o.body["page-theme"] === "light", "page props")), { headers: { cookie: "theme=light" } });
add("ssg-with-app-gip", "/ssg", (o) => all(status(o, 200), is(o.body.next?.gsp === true, "gsp"), is(o.body["page-when"] === "build", "build-time props")));
// Upstream: with the App's getInitialProps, a data request runs it on the
// server too and its props travel beside pageProps (for getStaticProps,
// as computed at build time); client navigation renders with them.
add("data-ssr", data("/ssr.json"), (o) =>
  all(status(o, 200), is(o.body?.pageProps?.theme === "blue", "page props"), is(o.body?.app?.side === "server" && o.body.app.user === "bob" && o.body.app.theme === "blue", `App props in the data: ${JSON.stringify(o.body?.app)}`)), { headers: { cookie: "theme=blue", "x-test-user": "bob" } });
add("data-ssg", data("/ssg.json"), (o) => all(status(o, 200), is(o.body?.pageProps?.when === "build", "data"), is(o.body?.app?.side === "server" && o.body.app.path === "/ssg", `build-time App props: ${JSON.stringify(o.body?.app)}`)));
add("redirect", "/redirect", (o) => all(status(o, 307), is(o.headers.location === "/gip?from=redirect", `Location ${o.headers.location}`)));
add("page-throws", "/throws", (o) => all(status(o, 500), is(o.body?.title === "Error 500", `error page ${o.body?.title}`), is(o.body?.["error-side"] === "server", "error page on the server"), is(o.body?.app, "custom App around the error page")));
add("not-found", "/missing", (o) => all(status(o, 404), is(o.body?.title === "Error 404", `error page ${o.body?.title}`)));
add("api", "/api/health", (o) => all(status(o, 200), is(o.body?.ok === true, "api")));
add("post-page", "/gip", (o) => status(o, 200), { method: "POST" });

scenarios["concurrent-identities"] = {
  kind: "compare",
  run: async (base) => {
    const jobs = [];
    for (let i = 0; i < 80; i++) {
      const user = `u${i}`;
      const theme = `t${i % 7}`;
      jobs.push(
        fetch(`${base}/gip?i=${i}`, { headers: { "x-test-user": user, cookie: `theme=${theme}` } })
          .then((r) => r.text())
          .then((html) => {
            const s = summarize(html);
            return s["app-user"] === user && s["app-theme"] === theme && s["page-query"] === `{"i":"${i}"}`;
          }),
      );
    }
    const ok = await Promise.all(jobs);
    return { total: ok.length, wrong: ok.filter((x) => !x).length };
  },
  expect: (o) => is(o.total === 80 && o.wrong === 0, `${o.wrong} of ${o.total} responses had another request's identity`),
};

const failures = await runScenarios("pages-app-gip", scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
