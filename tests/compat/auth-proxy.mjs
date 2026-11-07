// An application that authenticates in proxy.ts with a real JWT library
// (jose, the Next.js authentication guide's pattern), compared between next
// start and NextRS on fixtures/next-auth-proxy.
//
//   SESSION_SECRET=<test secret> node tests/compat/auth-proxy.mjs \
//        --baseline URL --candidate URL --upstream URL --dist DIR [--only a,b] [--json out.json]
//
// Both servers must run with the same SESSION_SECRET (a test-only value;
// it is never printed). The suite mints its own HS256 tokens with
// node:crypto, independently of jose, to send valid, expired, re-signed,
// tampered and malformed sessions. next start must listen on 0.0.0.0 (see
// tests/compat/proxy.mjs).
import { createHmac, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { observe, parseArgs, runScenarios, sleep } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, upstream, dist } = args;
const SECRET = process.env.SESSION_SECRET;
if (!baseline || !candidate || !upstream || !dist || !SECRET) {
  console.error("usage: SESSION_SECRET=... auth-proxy.mjs --baseline URL --candidate URL --upstream URL --dist DIR [--json FILE]");
  process.exit(2);
}
const only = args.only ? new Set(args.only.split(",")) : null;
const run = `${Date.now().toString(36)}${randomBytes(2).toString("hex")}`;

// --- Tokens ----------------------------------------------------------------
const b64u = (value) => Buffer.from(value).toString("base64url");
const HASH = { HS256: "sha256", HS384: "sha384", HS512: "sha512" };
function jwt(payload, { secret = SECRET, alg = "HS256", header } = {}) {
  const h = b64u(JSON.stringify(header ?? { alg }));
  const p = b64u(JSON.stringify(payload));
  const signature = alg === "none" ? "" : createHmac(HASH[alg], secret).update(`${h}.${p}`).digest("base64url");
  return `${h}.${p}.${signature}`;
}
const now = () => Math.floor(Date.now() / 1000);
const claims = (userId, name, ttl = 3600) => ({ userId, name, iat: now(), exp: now() + ttl });
const alice = () => jwt(claims("u-alice", "Alice"));
const bob = () => jwt(claims("u-bob", "Bob"));
const cookie = (token) => ({ cookie: `session=${token}` });
function tampered() {
  const [h, , s] = alice().split(".");
  return `${h}.${b64u(JSON.stringify(claims("u-admin", "Admin")))}.${s}`;
}

// --- Observations ------------------------------------------------------------
// A session cookie's value and expiry differ between runs (the JWT's iat and
// exp, the Expires date); the suite checks the token works instead.
const redactCookie = (c) =>
  c.replace(/^session=[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/, "session=<JWT>").replace(/Expires=[^;]+/i, "Expires=<DATE>");
async function look(base, request) {
  const o = await observe(base, request);
  if (o.setCookie) o.setCookie = o.setCookie.map(redactCookie);
  return o;
}

const manifest = JSON.parse(readFileSync(join(dist, "server/server-reference-manifest.json"), "utf8"));
const actionId = (name, file) => {
  const found = Object.entries(manifest.node).find(([, v]) => v.exportedName === name && v.filename === file);
  if (!found) throw new Error(`no action ${name} in ${file}`);
  return found[0];
};
const ids = {
  login: actionId("login", "app/login/actions.ts"),
  logout: actionId("logout", "app/login/actions.ts"),
  record: actionId("recordProtected", "app/actions/protected/actions.ts"),
};
const ledger = async (key) => (await (await fetch(`${upstream}/mutations?key=${encodeURIComponent(key)}`)).json()).ids;

// A Server Action call as React sends one for a FormData argument.
function actionRequest(base, path, id, fields, headers = {}) {
  const form = new FormData();
  for (const [name, value] of Object.entries(fields)) form.append(`_1_${name}`, value);
  form.append("0", '["$K1"]');
  return { method: "POST", path, body: form, headers: { origin: base, "next-action": id, accept: "text/x-component", ...headers } };
}
// A form submitted without JavaScript (progressive enhancement).
function formPost(base, path, id, fields, headers = {}) {
  const form = new FormData();
  form.append(`$ACTION_ID_${id}`, "");
  for (const [name, value] of Object.entries(fields)) form.append(name, value);
  return { method: "POST", path, body: form, headers: { origin: base, ...headers } };
}

const scenarios = {};
const get = (name, path, headers = {}, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (base) => look(base, { path, headers, ...extra }) };
};
// Headers the token getters build at run time (fresh iat/exp per side).
const getWith = (name, path, headers, extra = {}) => {
  scenarios[name] = { kind: "compare", run: (base) => look(base, { path, headers: headers(), ...extra }) };
};

// --- Public pages -------------------------------------------------------------
get("public-home", "/");
get("public-login", "/login");
get("public-login-next", "/login?next=%2Freports");
get("public-health-not-matched", "/api/health");

// --- No session: redirect to login with the destination kept -------------------
get("anon-dashboard", "/dashboard");
get("anon-dashboard-query", "/dashboard?tab=billing&x=1%202");
get("anon-static-page", "/account");
get("anon-isr-page", "/reports");
get("anon-dynamic-param", "/dashboard/projects/p-1");
get("anon-route-handler", "/api/private");
get("anon-action-page", "/actions/protected");
get("anon-head", "/account", {}, { method: "HEAD" });
get("anon-rsc", "/dashboard", { rsc: "1" });
get("anon-rsc-prefetch", "/account", { rsc: "1", "next-router-prefetch": "1" });
get("anon-rsc-segment-prefetch", "/reports", { rsc: "1", "next-router-prefetch": "1", "next-router-segment-prefetch": "/_tree" });
get("anon-rsc-query", "/dashboard?_rsc=abc");

// --- Session states -------------------------------------------------------------
getWith("valid-alice-dashboard", "/dashboard", () => cookie(alice()));
getWith("valid-bob-dashboard", "/dashboard", () => cookie(bob()));
getWith("valid-static-page", "/account", () => cookie(alice()));
getWith("valid-isr-page", "/reports", () => cookie(alice()));
getWith("valid-dynamic-param", "/dashboard/projects/p-7", () => cookie(bob()));
getWith("valid-route-handler", "/api/private", () => cookie(alice()));
getWith("valid-rsc", "/dashboard", () => ({ ...cookie(alice()), rsc: "1" }));
getWith("valid-login-redirects-to-dashboard", "/login", () => cookie(alice()));
getWith("expired-session", "/dashboard", () => cookie(jwt({ ...claims("u-alice", "Alice"), exp: now() - 60 })));
getWith("expired-session-route-handler", "/api/private", () => cookie(jwt({ ...claims("u-alice", "Alice"), exp: now() - 60 })));
getWith("not-yet-valid-session", "/dashboard", () => cookie(jwt({ ...claims("u-alice", "Alice"), nbf: now() + 600 })));
getWith("wrong-secret", "/dashboard", () => cookie(jwt(claims("u-alice", "Alice"), { secret: "not-the-secret" })));
getWith("tampered-payload", "/dashboard", () => cookie(tampered()));
getWith("tampered-signature", "/account", () => cookie(alice().slice(0, -2) + (alice().endsWith("AA") ? "BB" : "AA")));
getWith("alg-none", "/dashboard", () => cookie(jwt(claims("u-alice", "Alice"), { alg: "none" })));
getWith("alg-not-allowed-hs512", "/dashboard", () => cookie(jwt(claims("u-alice", "Alice"), { alg: "HS512" })));
getWith("missing-claims", "/dashboard", () => cookie(jwt({ iat: now(), exp: now() + 60 })));
get("malformed-cookie", "/dashboard", cookie("not-a-jwt"));
get("empty-cookie", "/dashboard", { cookie: "session=" });
get("other-cookie-only", "/dashboard", { cookie: "theme=dark" });
getWith("duplicate-session-cookie", "/dashboard", () => ({ cookie: `session=garbage; session=${alice()}` }));
// The proxy drops a client's own x-auth-user and sets the verified one.
get("spoofed-identity-header-anon", "/dashboard", { "x-auth-user": "u-alice" });
getWith("spoofed-identity-header-session", "/dashboard", () => ({ ...cookie(bob()), "x-auth-user": "u-alice" }));

// --- Bypass attempts (no session): never protected content ----------------------
get("bypass-encoded-first-char", "/%64ashboard", {}, { raw: true });
get("bypass-encoded-last-char", "/dashboar%64", {}, { raw: true });
get("bypass-encoded-static", "/%61ccount", {}, { raw: true });
get("bypass-encoded-isr", "/report%73", {}, { raw: true });
get("bypass-encoded-api", "/api/privat%65", {}, { raw: true });
get("bypass-encoded-rsc", "/%64ashboard", { rsc: "1" }, { raw: true });
get("bypass-encoded-prefetch", "/%61ccount", { rsc: "1", "next-router-prefetch": "1" }, { raw: true });
get("bypass-encoded-slash", "/dashboard%2Fprojects%2Fp-1", {}, { raw: true });
get("bypass-dot-segments", "/login/../dashboard", {}, { raw: true });
get("bypass-encoded-dot-segments", "/login/%2e%2e/account", {}, { raw: true });
get("bypass-double-slash", "//dashboard", {}, { raw: true });
get("bypass-backslash", "/\\account", {}, { raw: true });
get("bypass-trailing-slash", "/account/", {}, { raw: true });
get("bypass-upper-case", "/Dashboard");
get("bypass-upper-case-static", "/ACCOUNT");
get("bypass-semicolon", "/account;x", {}, { raw: true });
get("bypass-rsc-suffix", "/account.rsc");
get("bypass-html-suffix", "/account.html");
get("bypass-data-route", "/_next/data/unknown/account.json");
get("bypass-next-data-query", "/account?__nextDataReq=1");
get("bypass-config-rewrite-alias", "/me");
getWith("config-rewrite-alias-session", "/me", () => cookie(alice()));
get("bypass-internal-headers", "/account", {
  "x-middleware-subrequest": "proxy:proxy:proxy:proxy:proxy:middleware:middleware:middleware:middleware:middleware",
  "x-middleware-next": "1",
  "x-middleware-rewrite": "/",
  "x-middleware-override-headers": "x-auth-user",
  "x-middleware-request-x-auth-user": "u-alice",
  "x-matched-path": "/",
  "x-nextjs-data": "1",
  "x-now-route-matches": "1",
  "next-resume": "1",
});
get("bypass-forwarded-host", "/dashboard", { "x-forwarded-host": "evil.example", "x-forwarded-proto": "https" });

// --- Login and logout (Server Actions), and the redirect back -------------------
// The session the login sets must work: it is sent back for the dashboard.
async function loginFlow(base, request) {
  const response = await look(base, request);
  const raw = await fetch(`${base}${request.path}`, { method: "POST", body: request.body, headers: request.headers, redirect: "manual" }).catch(() => null);
  const set = raw?.headers.getSetCookie().find((c) => c.startsWith("session=")) ?? null;
  let dashboard = null;
  if (set) dashboard = await look(base, { path: "/dashboard", headers: { cookie: set.split(";")[0] } });
  return { response, dashboard };
}
scenarios["login-action-success"] = {
  kind: "compare",
  run: (base) => loginFlow(base, actionRequest(base, "/login", ids.login, { username: "alice", password: "alice-test-password", next: "/reports" })),
};
scenarios["login-action-wrong-password"] = {
  kind: "compare",
  run: (base) => loginFlow(base, actionRequest(base, "/login", ids.login, { username: "alice", password: "nope", next: "/reports" })),
};
scenarios["login-action-open-redirect"] = {
  kind: "compare",
  run: (base) => loginFlow(base, actionRequest(base, "/login", ids.login, { username: "bob", password: "bob-test-password", next: "//evil.example/x" })),
};
scenarios["login-form-without-js"] = {
  kind: "compare",
  run: (base) => loginFlow(base, formPost(base, "/login", ids.login, { username: "bob", password: "bob-test-password", next: "/dashboard/projects/p-2" })),
};
scenarios["login-action-cross-origin"] = {
  kind: "compare",
  run: (base) => loginFlow(base, actionRequest(base, "/login", ids.login, { username: "alice", password: "alice-test-password", next: "/" }, { origin: "https://evil.example" })),
};
scenarios["logout-action"] = {
  kind: "compare",
  run: (base) => look(base, actionRequest(base, "/dashboard", ids.logout, {}, cookie(alice()))),
};

// --- A protected mutation: the proxy guards its page; the action guards itself --
// `expected`: the ledger the candidate must show (compared servers can
// agree on a wrong answer; a malformed body once made both record nothing).
function mutation(name, path, headers, expected) {
  const runOnce = async (base, side) => {
    const key = `auth-${name}-${run}-${side}`;
    const response = await look(base, actionRequest(base, path, ids.record, { key, id: "1" }, headers()));
    await sleep(100);
    return { response, ledger: await ledger(key) };
  };
  scenarios[name] = { kind: "compare", run: runOnce };
  scenarios[`${name}-expected`] = {
    kind: "candidate",
    run: async (base) => {
      const r = await runOnce(base, "expected");
      return { ok: JSON.stringify(r.ledger) === JSON.stringify(expected), detail: { expected, ledger: r.ledger, status: r.response.status } };
    },
  };
}
mutation("action-anon-on-protected-page", "/actions/protected", () => ({}), []);
mutation("action-anon-on-public-page", "/", () => ({}), []);
mutation("action-expired-on-public-page", "/", () => cookie(jwt({ ...claims("u-alice", "Alice"), exp: now() - 5 })), []);
mutation("action-tampered-on-public-page", "/", () => cookie(tampered()), []);
mutation("action-alice-on-protected-page", "/actions/protected", () => cookie(alice()), ["u-alice:1"]);
mutation("action-bob-on-public-page", "/", () => cookie(bob()), ["u-bob:1"]);
mutation("action-anon-encoded-path", "/actions/%70rotected", () => ({}), []);
mutation("action-cross-origin-session", "/actions/protected", () => ({ ...cookie(alice()), origin: "https://evil.example" }), []);

// --- Concurrency: distinct identities never cross ---------------------------------
scenarios["concurrent-identities"] = {
  kind: "compare",
  run: async (base) => {
    const total = 240;
    const concurrency = 48;
    let next = 0;
    const wrong = [];
    let failed = 0;
    async function worker() {
      while (next < total) {
        const i = next++;
        const who = i % 3 === 0 ? null : i % 3 === 1 ? ["u-alice", alice()] : ["u-bob", bob()];
        const path = i % 2 ? "/api/private" : "/dashboard";
        try {
          const r = await fetch(`${base}${path}`, { headers: who ? cookie(who[1]) : {}, redirect: "manual" });
          const text = await r.text();
          let seen;
          if (!who) seen = r.status === 307 || r.status === 401 ? "anon-refused" : `served-${r.status}`;
          else if (path === "/api/private") seen = JSON.parse(text).userId;
          else seen = JSON.parse((/id="info">([^<]*)</.exec(text)?.[1] ?? "{}").replace(/&quot;/g, '"')).proxyUser;
          const expected = who ? who[0] : "anon-refused";
          if (seen !== expected) wrong.push({ i, path, expected, seen });
        } catch {
          failed++;
        }
      }
    }
    await Promise.all(Array.from({ length: concurrency }, worker));
    return { total, wrong: wrong.length, sample: wrong.slice(0, 3), failed };
  },
};

const failures = await runScenarios("auth-proxy (jose)", scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
