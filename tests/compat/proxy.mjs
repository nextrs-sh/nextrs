// proxy.ts (Next's compiled proxy, run by NextRS in embedded V8) compared
// with next start, on fixtures/next-proxy.
//
//   node tests/compat/proxy.mjs --baseline URL --candidate URL --upstream URL \
//        --dist DIR [--variant default|base-path|trailing-slash] \
//        [--request-timeout-ms N] [--stall-ms N] [--only a,b] [--json out.json]
//
// --dist is the build both servers serve (for Server Action ids). next
// start must listen on 0.0.0.0 (or without -H): bound to 127.0.0.1 it
// relativizes proxy rewrites against a different origin than its proxy's
// URLs (NextURL names loopback hosts `localhost`) and proxies them to
// itself over HTTP. NextRS names a loopback bind `localhost` for both.
//
// --request-timeout-ms / --stall-ms are NextRS's --request-timeout and
// --js-stall-timeout; the scenarios that need them run only when given.
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { randomBytes } from "node:crypto";
import net from "node:net";
import { observe, parseArgs, runScenarios, sleep } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, upstream, dist } = args;
if (!baseline || !candidate || !upstream || !dist) {
  console.error("usage: proxy.mjs --baseline URL --candidate URL --upstream URL --dist DIR [--variant V] [--json FILE]");
  process.exit(2);
}
const variant = args.variant ?? "default";
const only = args.only ? new Set(args.only.split(",")) : null;
const run = `${Date.now().toString(36)}${randomBytes(2).toString("hex")}`;
const SESSION = "session=synthetic-valid-session";

// A page path in this build: basePath prefixed, trailing slash added
// where next start would redirect to one (not for files or queries' keys).
function p(path) {
  const [pathname, query] = path.split("?");
  let out = variant === "base-path" ? (pathname === "/" ? "/base" : `/base${pathname}`) : pathname;
  if (variant === "trailing-slash" && !out.endsWith("/") && !/\.[a-z0-9]+$/i.test(out)) out += "/";
  return query === undefined ? out : `${out}?${query}`;
}

const manifest = JSON.parse(readFileSync(join(dist, "server/server-reference-manifest.json"), "utf8"));
const actionId = (name, file = "app/actions/actions.ts") => {
  const found = Object.entries(manifest.node).find(([, v]) => v.exportedName === name && v.filename === file);
  if (!found) throw new Error(`no action ${name} in ${file}`);
  return found[0];
};
const ids = { inspect: actionId("inspect"), record: actionId("record"), refresh: actionId("refreshIsr") };
const ledger = async (key) => (await (await fetch(`${upstream}/mutations?key=${encodeURIComponent(key)}`)).json()).ids;

// A Server Action call as React sends one for a FormData argument.
function actionRequest(base, path, id, fields, headers = {}) {
  const form = new FormData();
  for (const [name, value] of Object.entries(fields)) form.append(`1_${name}`, value);
  form.append("0", '["$K1"]');
  return {
    method: "POST",
    path,
    body: form,
    headers: { origin: base, "next-action": id, accept: "text/x-component", ...headers },
  };
}

const scenarios = {};
const get = (name, path, headers = {}, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (base) => observe(base, { path, headers, ...extra }) };
};
const knownDiff = (name, reason, request) => {
  scenarios[name] = { kind: "known-diff", reason, run: (base) => observe(base, request) };
};

// --- NextResponse.next(): headers and cookies on the way to the route ------
get("next-static", p("/target"));
get("next-home", p("/"));
get("next-dynamic", p("/dynamic"));
get("next-dynamic-query", p("/dynamic?a=1&a=2&b=x%20y"));
get("next-route-handler", p("/api/echo?x=1"));
get("next-public-file", p("/asset.txt"));
get("next-prerendered-dynamic-param", p("/blog/hello"));
// Rendered on its first request, cached after (compared on the second, so
// a page next start kept on disk from an earlier run does not differ).
scenarios["next-on-demand-param"] = {
  kind: "compare",
  run: async (base) => {
    await observe(base, { path: p(`/blog/other-${run}`) });
    return observe(base, { path: p(`/blog/other-${run}`) });
  },
};
get("request-header-removed", p("/dynamic"), { "x-remove-me": "1", "x-user": "spoof" });
get("request-cookie-read", p("/dynamic"), { cookie: "theme=dark; other=1" });
get("response-cookie-write", p("/dynamic?setcookie=v-1"));
get("async-proxy", p("/proxy/async"));
get("head-static", p("/target"), {}, { method: "HEAD" });
get("head-dynamic", p("/dynamic"), {}, { method: "HEAD" });

// --- Matchers -----------------------------------------------------------------
get("matcher-excluded-page", p("/excluded"));
get("matcher-excluded-api", p("/api/health"));
get("matcher-excluded-static-asset", p("/_next/static/missing.js"));
get("matcher-has-absent", p("/has-gate/a"));
get("matcher-has-present", p("/has-gate/a"), { "x-gate": "open" });
get("matcher-has-wrong-value", p("/has-gate/a"), { "x-gate": "closed" });
get("matcher-missing-present", p("/has-gate/a"), { "x-gate": "open", cookie: "skip-proxy=1" });
get("matcher-not-found", p("/nope"));

// --- Redirects -----------------------------------------------------------------
get("redirect-307", p("/proxy/redirect"));
get("redirect-308-query", p("/proxy/redirect-308"));
get("redirect-external", p("/proxy/redirect-external"));
get("redirect-response-303", p("/proxy/response-redirect"));
get("redirect-auth-static", p("/protected"));
get("redirect-auth-isr", p("/protected/isr"));
get("redirect-auth-rsc", p("/protected"), { rsc: "1" });
get("auth-session-static", p("/protected"), { cookie: SESSION });
get("auth-session-isr", p("/protected/isr"), { cookie: SESSION });
get("auth-session-wrong", p("/protected"), { cookie: "session=forged" });
get("auth-spoofed-user-header", p("/protected"), { "x-user": "synthetic-user" });

// --- Rewrites -----------------------------------------------------------------
get("rewrite-static", p("/proxy/rewrite"));
get("rewrite-query", p("/proxy/rewrite-query?orig=2"));
get("rewrite-dynamic", p("/proxy/rewrite-dynamic/abc?q=1"));
get("rewrite-dynamic-encoded", p("/proxy/rewrite-dynamic/a%20b"));
get("rewrite-isr", p("/proxy/rewrite-isr"));
get("rewrite-prerendered", p("/proxy/rewrite-static"));
get("rewrite-route-handler", p("/proxy/rewrite-api"));
get("rewrite-missing", p("/proxy/rewrite-missing"));
get("rewrite-to-itself", p("/proxy/rewrite-loop"));
get("rewrite-to-protected", p("/proxy/rewrite-protected"));
get("rewrite-rsc", p("/proxy/rewrite"), { rsc: "1" });
get("rewrite-dynamic-rsc", p("/proxy/rewrite-dynamic/abc"), { rsc: "1" });
get("rewrite-invalid-url", p("/proxy/rewrite-invalid"));
knownDiff(
  "rewrite-external",
  "the destination (a closed port) is outside this run's --net-allow: NextRS answers 502 external-rewrite-denied and sends nothing, next start proxies (connection refused: 500). Allowed destinations are compared in tests/compat/platform-proxy.mjs",
  { path: p("/proxy/rewrite-external") },
);

// --- Direct responses and errors ------------------------------------------------
get("direct-json", p("/proxy/json?k=v"));
get("direct-json-forwarded-https", p("/proxy/json"), { "x-forwarded-proto": "https" });
get("direct-text-418", p("/proxy/text"));
get("direct-204", p("/proxy/empty"));
get("direct-location-not-redirect", p("/proxy/location-200"));
get("proxy-exception", p("/proxy/throw"));
get("proxy-exception-rsc", p("/proxy/throw"), { rsc: "1" });
get("body-read", p("/proxy/body"), { "content-type": "text/plain" }, { method: "POST", body: "hello body" });
get("body-read-then-route", p("/proxy/body-pass"), { "content-type": "text/plain" }, { method: "POST", body: "pass body" });

// --- Bypass classes and internal headers ----------------------------------------
get("internal-headers-from-client", p("/dynamic"), {
  "x-middleware-rewrite": "/target",
  "x-middleware-next": "1",
  "x-middleware-override-headers": "x-user",
  "x-middleware-request-x-user": "evil",
  "x-middleware-set-cookie": "session=x",
  "x-middleware-skip": "1",
  "x-matched-path": "/target",
  "x-now-route-matches": "1",
  "next-resume": "1",
});
get("internal-rewrite-header-on-protected", p("/protected"), { "x-middleware-rewrite": p("/target"), "x-middleware-next": "1" });
get("subrequest-header", p("/protected"), { "x-middleware-subrequest": "proxy:proxy:proxy:proxy:proxy:middleware:middleware:middleware:middleware:middleware" });
get("matched-path-header", p("/protected"), { "x-matched-path": p("/login"), "x-nextjs-data": "1" });
get("encoded-pathname", p("/%70rotected"), {}, { raw: true });
get("encoded-isr-pathname", p("/protecte%64/isr"), {}, { raw: true });
get("dot-segments", p("/login/../protected"), {}, { raw: true });
get("double-slash", "//protected", {}, { raw: true });
get("backslash", "/\\protected", {}, { raw: true });
get("upper-case", p("/PROTECTED"));
get("rsc-suffix", p("/protected.rsc"));
get("data-request-unknown-build", `${variant === "base-path" ? "/base" : ""}/_next/data/unknown/protected.json`);
get("location-header-reflected", p("/proxy/reflect"), { location: "http://127.0.0.1:9/evil" });
get("query-data-req", p("/protected?__nextDataReq=1"));
get("forwarded-host-redirect", p("/protected"), { "x-forwarded-host": "evil.example", "x-forwarded-proto": "https" });

// --- ISR behind the proxy ----------------------------------------------------------
scenarios["isr-cached-through-proxy"] = {
  kind: "compare",
  run: async (base) => {
    const a = await observe(base, { path: p("/isr") });
    const b = await observe(base, { path: p("/isr") });
    return { first: a, second: b };
  },
};

// --- Server Actions through the proxy -----------------------------------------------
scenarios["action-sees-proxy-headers"] = {
  kind: "compare",
  run: (base) => observe(base, actionRequest(base, p("/actions"), ids.inspect, { value: "x" })),
};
scenarios["action-executes-once"] = {
  kind: "compare",
  run: async (base, side) => {
    const key = `proxy-once-${run}-${side}`;
    const response = await observe(base, actionRequest(base, p("/actions"), ids.record, { key, id: "1" }));
    await sleep(100);
    return { response, executions: (await ledger(key)).length };
  },
};
scenarios["action-protected-anonymous"] = {
  kind: "compare",
  run: async (base, side) => {
    const key = `proxy-anon-${run}-${side}`;
    const response = await observe(base, actionRequest(base, p("/protected/actions"), ids.record, { key, id: "1" }));
    await sleep(100);
    return { response, executions: (await ledger(key)).length };
  },
};
scenarios["action-protected-session"] = {
  kind: "compare",
  run: async (base, side) => {
    const key = `proxy-session-${run}-${side}`;
    const response = await observe(base, actionRequest(base, p("/protected/actions"), ids.record, { key, id: "1" }, { cookie: SESSION }));
    await sleep(100);
    return { response, executions: (await ledger(key)).length };
  },
};
scenarios["action-revalidates-isr"] = {
  kind: "compare",
  run: async (base) => {
    const stamp = async () => {
      const r = await fetch(`${base}${p("/isr")}`);
      return /stamp-(?:<!-- -->)?(\d+)/.exec(await r.text())?.[1];
    };
    const before = await stamp();
    const response = await observe(base, actionRequest(base, p("/actions"), ids.refresh, {}));
    const after = await stamp();
    const again = await stamp();
    return { response, regenerated: before !== after, stable: after === again };
  },
};

// --- Isolation under concurrency -------------------------------------------------------
scenarios["concurrent-request-isolation"] = {
  kind: "compare",
  run: async (base) => {
    const total = 240;
    const concurrency = 48;
    let next = 0;
    let wrong = 0;
    let failed = 0;
    async function worker() {
      while (next < total) {
        const i = next++;
        const value = `c${i}`;
        try {
          const r = await fetch(`${base}${p(`/proxy/rewrite-dynamic/${value}?setcookie=${value}`)}`, {
            headers: { cookie: `theme=${value}` },
          });
          const html = await r.text();
          const info = JSON.parse(
            (/id="info">([^<]*)</.exec(html)?.[1] ?? "{}").replace(/&quot;/g, '"'),
          );
          const slug = /id="slug">([^<]*)</.exec(html)?.[1];
          const cookie = r.headers.getSetCookie().some((c) => c.startsWith(`proxy-custom=${value};`));
          if (r.status !== 200 || slug !== value || info.proxyCustom !== value || !cookie) wrong++;
        } catch {
          failed++;
        }
      }
    }
    await Promise.all(Array.from({ length: concurrency }, worker));
    return { total, wrong, failed };
  },
};

// --- HTTP/1.0 framing of a proxy's direct response ------------------------------------
// HTTP/1.0 has no chunked encoding: a body without Content-Length must end
// with the connection (NextRS once kept it open, breaking keep-alive clients).
scenarios["http10-direct-response-framing"] = {
  kind: "compare",
  run: (base) =>
    new Promise((resolve) => {
      const url = new URL(base);
      const socket = net.connect(Number(url.port), url.hostname, () => {
        socket.write(`GET ${p("/proxy/json")} HTTP/1.0\r\nHost: ${url.host}\r\nConnection: keep-alive\r\n\r\n`);
      });
      const chunks = [];
      let timedOut = false;
      const timer = setTimeout(() => {
        timedOut = true;
        socket.destroy();
      }, 3000);
      socket.on("data", (c) => chunks.push(c));
      socket.on("close", () => {
        clearTimeout(timer);
        const text = Buffer.concat(chunks).toString("latin1");
        const [head, ...rest] = text.split("\r\n\r\n");
        const body = rest.join("\r\n\r\n");
        const lengthHeader = /\r\ncontent-length: *(\d+)/i.exec(head)?.[1];
        let parsed = null;
        try {
          parsed = JSON.parse(body.slice(0, lengthHeader ? Number(lengthHeader) : undefined)).proxied;
        } catch {}
        resolve({ status: head.split(" ")[1], bodyIsCompleteJson: parsed === true, closedOrLength: !timedOut || Boolean(lengthHeader) });
      });
      socket.on("error", () => {});
    }),
};

// --- NextRS-only reliability checks ------------------------------------------------------
scenarios["abort-mid-proxy"] = {
  kind: "candidate",
  run: async (base) => {
    const controller = new AbortController();
    const pending = fetch(`${base}${p("/proxy/slow?ms=1500")}`, { signal: controller.signal }).catch((e) => e.name);
    await sleep(100);
    controller.abort();
    const aborted = await pending;
    const after = await observe(base, { path: p("/dynamic") });
    return { ok: aborted === "AbortError" && after.status === 200, detail: { aborted, after: after.status } };
  },
};
if (args["request-timeout-ms"]) {
  const timeout = Number(args["request-timeout-ms"]);
  scenarios["proxy-timeout-never-continues"] = {
    kind: "candidate",
    run: async (base) => {
      const r = await observe(base, { path: p(`/proxy/slow?ms=${timeout + 2000}`) });
      const ok = r.status === 504 && r.headers["x-nextrs-error"] === "request-timeout" && r.body?.title === undefined;
      return { ok, detail: { status: r.status, error: r.headers?.["x-nextrs-error"] } };
    },
  };
}
if (args["stall-ms"]) {
  const stall = Number(args["stall-ms"]);
  scenarios["proxy-stall-restarts-isolate"] = {
    kind: "candidate",
    run: async (base) => {
      const r = await observe(base, { path: p(`/proxy/spin?ms=${stall + 2000}`) });
      let after;
      for (let i = 0; i < 40; i++) {
        after = await observe(base, { path: p("/dynamic") });
        if (after.status === 200) break;
        await sleep(250);
      }
      const ok = [502, 503].includes(r.status) && r.body?.title === undefined && after.status === 200;
      return { ok, detail: { stalled: r.status, error: r.headers?.["x-nextrs-error"], after: after.status } };
    },
  };
}

const failures = await runScenarios(`proxy (${variant})`, scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
