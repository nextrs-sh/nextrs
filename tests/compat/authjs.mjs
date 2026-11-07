// Auth.js (next-auth v5) set up as its documentation shows, compared
// between next start and NextRS on fixtures/next-authjs: the credentials
// sign-in with its CSRF check, the OpenID Connect code flow against the
// local provider (PKCE, state, nonce, ID token validation, key rotation),
// JWT sessions (JWE dir + A256CBC-HS512 under an HKDF-derived key), sign-out,
// the proxy's `authorized` check, route handlers, a protected Server Action
// and concurrent users.
//
//   AUTH_SECRET=... node tests/compat/authjs.mjs --baseline URL --candidate URL --oidc URL \
//        --upstream URL --project DIR [--isolates N] [--only a,b] [--json out.json]
//
// Sessions NextRS issues are decoded here with Node's own @auth/core/jwt
// (from the fixture's node_modules), and sessions sent to the servers are
// minted with it: the suite checks NextRS's JWE against a second runtime,
// not against itself. Every important scenario asserts its absolute
// outcome as well as the comparison.
import { randomBytes } from "node:crypto";
import { createRequire } from "node:module";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { observe, parseArgs, runScenarios, sleep } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, oidc, upstream, project } = args;
const SECRET = process.env.AUTH_SECRET;
if (!baseline || !candidate || !oidc || !upstream || !project || !SECRET) {
  console.error("usage: AUTH_SECRET=... authjs.mjs --baseline URL --candidate URL --oidc URL --upstream URL --project DIR");
  process.exit(2);
}
const only = args.only ? new Set(args.only.split(",")) : null;
const run = `${Date.now().toString(36)}${randomBytes(2).toString("hex")}`;
const require = createRequire(resolve(project, "package.json"));
const jwt = await import(pathToFileURL(require.resolve("@auth/core/jwt")).href);
const SALT = "authjs.session-token";

// --- Sessions minted and read with Node's Auth.js -----------------------------------
const mintSession = (sub, name, { secret = SECRET, salt = SALT, maxAge = 3600 } = {}) =>
  jwt.encode({ token: { sub, name, email: `${name.toLowerCase()}@example.test` }, secret, salt, maxAge });
async function readSession(value) {
  if (!value) return null;
  try {
    const token = await jwt.decode({ token: value, secret: SECRET, salt: SALT });
    if (!token) return null;
    return { sub: token.sub ?? null, name: token.name ?? null, email: token.email ?? null };
  } catch (error) {
    return { undecodable: String(error?.code ?? error?.message) };
  }
}

// --- A cookie jar --------------------------------------------------------------------
class Jar {
  cookies = new Map();
  take(response) {
    for (const line of response.headers.getSetCookie()) {
      const [pair, ...attrs] = line.split(";");
      const eq = pair.indexOf("=");
      const name = pair.slice(0, eq).trim();
      const value = pair.slice(eq + 1).trim();
      const expired = attrs.some((a) => /^\s*max-age=0\s*$/i.test(a) || (/^\s*expires=/i.test(a) && Date.parse(a.split("=")[1]) < Date.now()));
      if (expired || value === "") this.cookies.delete(name);
      else this.cookies.set(name, value);
    }
    return response;
  }
  header() {
    return [...this.cookies].map(([n, v]) => `${n}=${v}`).join("; ");
  }
  get(name) {
    return this.cookies.get(name);
  }
}
async function send(jar, base, path, { method = "GET", form, headers = {} } = {}) {
  const init = { method, redirect: "manual", headers: { ...headers } };
  if (jar.cookies.size) init.headers.cookie = jar.header();
  if (form) {
    init.body = new URLSearchParams(form).toString();
    init.headers["content-type"] = "application/x-www-form-urlencoded";
  }
  const response = jar.take(await fetch(path.startsWith("http") ? path : `${base}${path}`, init));
  await response.arrayBuffer();
  return response;
}
async function csrf(jar, base) {
  const r = await fetch(`${base}/api/auth/csrf`, { headers: jar.cookies.size ? { cookie: jar.header() } : {} });
  jar.take(r);
  return (await r.json()).csrfToken;
}
// The server's own origin in a Location (next start: 0.0.0.0, NextRS:
// localhost): replaced by the base the suite talks to.
const SELF = /^https?:\/\/(?:localhost|0\.0\.0\.0|127\.0\.0\.1|\[::1\]):\d+/;
const local = (base, location) => (location ?? "").replace(SELF, base);
const shown = (location) => (location ?? "").replace(SELF, "http://SELF");

// --- Observations ----------------------------------------------------------------------
const redact = (c) =>
  c
    .replace(/^(authjs\.[\w.-]+)=[^;]+/, "$1=<VALUE>")
    .replace(/Expires=[^;]+/i, "Expires=<DATE>");
// The server's origin also appears URL-encoded (a callbackUrl), and a
// session's expiry is a time of day.
const ENCODED_SELF = /https?%3A%2F%2F(?:localhost|0\.0\.0\.0|127\.0\.0\.1)%3A\d+/g;
async function look(base, request) {
  const o = await observe(base, request);
  if (o.setCookie) o.setCookie = o.setCookie.map(redact);
  if (o.headers?.location) o.headers.location = o.headers.location.replace(ENCODED_SELF, "http%3A%2F%2FSELF");
  if (typeof o.body === "string") o.body = o.body.replace(/"expires":"[^"]+"/, '"expires":"<DATE>"').replace(ENCODED_SELF, "http%3A%2F%2FSELF");
  return o;
}
const json = (o) => (typeof o.body === "string" ? JSON.parse(o.body) : o.body);
const identity = (sub) => (o) => (o.status === 200 && json(o).userId === sub ? null : `status ${o.status} ${JSON.stringify(o.body)}`);
const anonymous = (o) => (o.status === 401 && json(o).userId === undefined ? null : `status ${o.status} ${JSON.stringify(o.body)}`);
// The proxy's redirect is relative; the callbackUrl names the page.
const toSignIn = (o) =>
  o.status === 307 && /^\/api\/auth\/signin\?callbackUrl=http%3A%2F%2FSELF%2F/.test(o.headers.location ?? "") ? null : `status ${o.status} location ${o.headers.location}`;

const scenarios = {};
const get = (name, path, headers, expect) => {
  scenarios[name] = { kind: "compare", expect, run: async (base) => look(base, { path, headers: await headers() }) };
};
const withSession = async (sub, name, options) => ({ cookie: `${SALT}=${await mintSession(sub, name, options)}` });
// Auth.js decodes sessions with 15 s of clock tolerance: an expired
// session is one that ended more than that ago.
const EXPIRED = -60;

// --- Auth.js's own endpoints ---------------------------------------------------------------
get("providers", "/api/auth/providers", async () => ({}), (o) => {
  const body = json(o);
  return o.status === 200 && body.credentials?.type === "credentials" && body["local-oidc"]?.type === "oidc" ? null : `status ${o.status} ${JSON.stringify(body)}`;
});
scenarios["csrf-token"] = {
  kind: "compare",
  expect: (o) => (o.status === 200 && o.tokenShape && o.cookieMatches ? null : JSON.stringify(o)),
  run: async (base) => {
    const r = await fetch(`${base}/api/auth/csrf`);
    const body = await r.json();
    const set = r.headers.getSetCookie().find((c) => c.startsWith("authjs.csrf-token="));
    // The cookie holds `token|hash(token, secret)`.
    const [token, hash] = decodeURIComponent(set?.split(";")[0].split("=")[1] ?? "").split("|");
    return { status: r.status, tokenShape: /^[0-9a-f]{64}$/.test(body.csrfToken), cookieMatches: token === body.csrfToken && /^[0-9a-f]{64}$/.test(hash ?? ""), setCookie: r.headers.getSetCookie().map(redact) };
  },
};
get("session-anonymous", "/api/auth/session", async () => ({}), (o) => (o.status === 200 && o.body === "null" ? null : `status ${o.status} body ${o.body}`));
get("signin-page", "/api/auth/signin", async () => ({}), (o) => (o.status === 200 && o.headers["content-type"]?.startsWith("text/html") ? null : `status ${o.status}`));

// --- Credentials sign-in ---------------------------------------------------------------------
async function credentialsSignIn(base, fields, { csrfToken, jar = new Jar() } = {}) {
  const token = csrfToken === undefined ? await csrf(jar, base) : csrfToken;
  const r = await send(jar, base, "/api/auth/callback/credentials", { method: "POST", form: { ...fields, ...(token === null ? {} : { csrfToken: token }) } });
  return { status: r.status, location: shown(r.headers.get("location")), jar };
}
async function afterSignIn(base, result) {
  const value = result.jar.get(SALT);
  return {
    status: result.status,
    location: result.location,
    session: await readSession(value),
    api: value ? await look(base, { path: "/api/private", headers: { cookie: `${SALT}=${value}` } }) : null,
  };
}
const signedIn = (sub, location) => (o) =>
  o.status !== 302 || o.location !== location
    ? `status ${o.status} location ${o.location}, expected 302 ${location}`
    : o.session?.sub !== sub
      ? `session ${JSON.stringify(o.session)} (decoded with Node's Auth.js), expected ${sub}`
      : identity(sub)(o.api);
const notSignedIn = (pattern) => (o) =>
  o.session !== null ? `a session was issued: ${JSON.stringify(o.session)}` : pattern.test(o.location) ? null : `location ${o.location}`;

scenarios["credentials-sign-in"] = {
  kind: "compare",
  expect: signedIn("u-alice", "http://SELF/dashboard"),
  run: async (base) => afterSignIn(base, await credentialsSignIn(base, { username: "alice", password: "alice-test-password", callbackUrl: "/dashboard" })),
};
scenarios["credentials-sign-in-bob"] = {
  kind: "compare",
  expect: signedIn("u-bob", "http://SELF/reports"),
  run: async (base) => afterSignIn(base, await credentialsSignIn(base, { username: "bob", password: "bob-test-password", callbackUrl: "/reports" })),
};
scenarios["credentials-wrong-password"] = {
  kind: "compare",
  expect: notSignedIn(/^http:\/\/SELF\/api\/auth\/signin\?error=CredentialsSignin/),
  run: async (base) => afterSignIn(base, await credentialsSignIn(base, { username: "alice", password: "nope" })),
};
scenarios["credentials-unknown-user"] = {
  kind: "compare",
  expect: notSignedIn(/error=CredentialsSignin/),
  run: async (base) => afterSignIn(base, await credentialsSignIn(base, { username: "mallory", password: "x" })),
};
scenarios["credentials-missing-csrf"] = {
  kind: "compare",
  expect: notSignedIn(/error=MissingCSRF/),
  run: async (base) => afterSignIn(base, await credentialsSignIn(base, { username: "alice", password: "alice-test-password" }, { csrfToken: null })),
};
// A token from another browser's cookie: the hash check fails.
scenarios["credentials-foreign-csrf"] = {
  kind: "compare",
  expect: notSignedIn(/error=MissingCSRF/),
  run: async (base) => {
    const foreign = await csrf(new Jar(), base);
    return afterSignIn(base, await credentialsSignIn(base, { username: "alice", password: "alice-test-password" }, { csrfToken: foreign }));
  },
};
scenarios["credentials-open-redirect"] = {
  kind: "compare",
  // Auth.js's redirect callback keeps only same-origin callback URLs.
  expect: signedIn("u-alice", "http://SELF"),
  run: async (base) => afterSignIn(base, await credentialsSignIn(base, { username: "alice", password: "alice-test-password", callbackUrl: "https://evil.example/x" })),
};

// --- Sign-out -------------------------------------------------------------------------------------
scenarios["sign-out"] = {
  kind: "compare",
  expect: (o) => (o.status === 302 && o.cleared && o.sessionAfter === "null" ? null : JSON.stringify(o)),
  run: async (base) => {
    const { jar } = await credentialsSignIn(base, { username: "alice", password: "alice-test-password" });
    const token = await csrf(jar, base);
    const before = jar.get(SALT);
    const r = await send(jar, base, "/api/auth/signout", { method: "POST", form: { csrfToken: token, callbackUrl: "/" } });
    const cleared = Boolean(before) && !jar.get(SALT);
    const after = await fetch(`${base}/api/auth/session`, { headers: { cookie: jar.header() } });
    return { status: r.status, location: shown(r.headers.get("location")), cleared, sessionAfter: await after.text() };
  },
};

// --- Sessions minted with Node's Auth.js -----------------------------------------------------------
get("session-alice-api", "/api/private", () => withSession("u-alice", "Alice"), identity("u-alice"));
get("session-bob-api", "/api/private", () => withSession("u-bob", "Bob"), identity("u-bob"));
get("session-endpoint", "/api/auth/session", () => withSession("u-bob", "Bob"), (o) =>
  o.status === 200 && json(o).user?.id === "u-bob" && json(o).user?.name === "Bob" ? null : `status ${o.status} ${o.body}`,
);
get("session-dashboard", "/dashboard", () => withSession("u-alice", "Alice"), (o) =>
  o.status === 200 && o.body?.info === JSON.stringify({ userId: "u-alice", email: "alice@example.test" }) ? null : `status ${o.status} ${JSON.stringify(o.body)}`,
);
get("session-isr", "/reports", () => withSession("u-bob", "Bob"), (o) => (o.status === 200 && /proxy-protected/.test(o.body?.secret ?? "") ? null : `status ${o.status}`));
get("session-home", "/", () => withSession("u-bob", "Bob"), (o) => (o.status === 200 && o.body?.info === JSON.stringify({ user: "u-bob" }) ? null : JSON.stringify(o.body)));
get("anon-dashboard", "/dashboard", async () => ({}), toSignIn);
get("anon-isr", "/reports", async () => ({}), toSignIn);
get("anon-action-page", "/actions/protected", async () => ({}), toSignIn);
get("anon-api", "/api/private", async () => ({}), anonymous);
get("anon-home", "/", async () => ({}), (o) => (o.status === 200 && o.body?.info === JSON.stringify({ user: null }) ? null : JSON.stringify(o.body)));
get("session-wrong-secret", "/api/private", () => withSession("u-alice", "Alice", { secret: "another-secret-another-secret-00" }), anonymous);
get("session-wrong-secret-page", "/dashboard", () => withSession("u-alice", "Alice", { secret: "another-secret-another-secret-00" }), toSignIn);
get("session-wrong-salt", "/api/private", () => withSession("u-alice", "Alice", { salt: "__Secure-authjs.session-token" }), anonymous);
get("session-expired", "/api/private", () => withSession("u-alice", "Alice", { maxAge: EXPIRED }), anonymous);
get("session-expired-page", "/dashboard", () => withSession("u-alice", "Alice", { maxAge: EXPIRED }), toSignIn);
get("session-tampered", "/api/private", async () => {
  const value = await mintSession("u-alice", "Alice");
  const parts = value.split(".");
  // The ciphertext (fourth part) with one character changed.
  parts[3] = parts[3].slice(0, 5) + (parts[3][5] === "A" ? "B" : "A") + parts[3].slice(6);
  return { cookie: `${SALT}=${parts.join(".")}` };
}, anonymous);
get("session-tampered-tag", "/api/private", async () => {
  const value = await mintSession("u-alice", "Alice");
  const parts = value.split(".");
  parts[4] = parts[4].slice(0, -2) + (parts[4].endsWith("AA") ? "BB" : "AA");
  return { cookie: `${SALT}=${parts.join(".")}` };
}, anonymous);
get("session-not-a-jwe", "/api/private", async () => ({ cookie: `${SALT}=garbage` }), anonymous);
get("session-signed-jwt-not-jwe", "/api/private", async () => {
  const header = Buffer.from(JSON.stringify({ alg: "none" })).toString("base64url");
  const payload = Buffer.from(JSON.stringify({ sub: "u-alice", exp: Math.floor(Date.now() / 1000) + 600 })).toString("base64url");
  return { cookie: `${SALT}=${header}.${payload}.` };
}, anonymous);
// The session endpoint re-issues the cookie (a rolling session): the new
// value must still decode to the same user with Node's Auth.js.
scenarios["session-rolled"] = {
  kind: "compare",
  expect: (o) => (o.status === 200 && o.rolled?.sub === "u-alice" ? null : JSON.stringify(o)),
  run: async (base) => {
    const r = await fetch(`${base}/api/auth/session`, { headers: await withSession("u-alice", "Alice") });
    await r.text();
    const set = r.headers.getSetCookie().find((c) => c.startsWith(`${SALT}=`));
    return { status: r.status, rolled: await readSession(set?.split(";")[0].slice(SALT.length + 1)), setCookie: r.headers.getSetCookie().map(redact) };
  },
};
// A session that expires while in use: it ended 13 s ago, so the 15 s
// tolerance accepts it for 2 s more.
scenarios["session-expires"] = {
  kind: "compare",
  expect: (o) => identity("u-alice")(o.before) ?? anonymous(o.after),
  run: async (base) => {
    const headers = await withSession("u-alice", "Alice", { maxAge: -13 });
    const before = await look(base, { path: "/api/private", headers });
    await sleep(3100);
    const after = await look(base, { path: "/api/private", headers });
    return { before, after };
  },
};

// --- OpenID Connect: the authorization code flow ------------------------------------------------------
async function oidcAdmin(path) {
  const r = await fetch(`${oidc}${path}`, { method: "POST" });
  if (!r.ok) throw new Error(`oidc ${path}: ${r.status}`);
}
// Runs the flow; `tamper` may change the authorize URL, the callback URL or
// the jar before the callback.
async function oidcFlow(base, { user = "alice", callbackUrl = "/dashboard", tamper = {} } = {}) {
  const jar = new Jar();
  const token = await csrf(jar, base);
  const start = await send(jar, base, "/api/auth/signin/local-oidc", { method: "POST", form: { csrfToken: token, callbackUrl } });
  const authorize = new URL(start.headers.get("location") ?? "about:blank");
  const request = Object.fromEntries(authorize.searchParams);
  authorize.searchParams.set("login_hint", user);
  tamper.authorize?.(authorize);
  const provider = await fetch(authorize, { redirect: "manual" });
  await provider.arrayBuffer();
  const callback = new URL(local(base, provider.headers.get("location")));
  tamper.callback?.(callback);
  tamper.jar?.(jar);
  const done = await send(jar, base, callback.href);
  const out = {
    start: start.status,
    authorizeOrigin: authorize.origin === oidc,
    request: {
      response_type: request.response_type,
      client_id: request.client_id,
      scope: request.scope,
      code_challenge_method: request.code_challenge_method,
      redirect_uri: shown(request.redirect_uri),
      has: ["state", "nonce", "code_challenge"].filter((p) => request[p]),
    },
    callback: done.status,
    location: shown(done.headers.get("location")),
    session: await readSession(jar.get(SALT)),
    jar,
    callbackHref: callback.href,
  };
  return out;
}
const flowOk = (sub, location = "http://SELF/dashboard") => (o) =>
  !o.authorizeOrigin || o.request.code_challenge_method !== "S256" || o.request.has.length !== 3
    ? `authorization request ${JSON.stringify(o.request)}`
    : o.callback !== 302 || o.location !== location
      ? `callback ${o.callback} ${o.location}`
      : o.session?.sub !== sub
        ? `session ${JSON.stringify(o.session)}`
        : null;
const flowRefused = (o) =>
  o.session !== null ? `a session was issued: ${JSON.stringify(o.session)}` : /^http:\/\/SELF\/api\/auth\/(error|signin)\?error=/.test(o.location) ? null : `callback ${o.callback} ${o.location}`;
const strip = ({ jar, callbackHref, ...rest }) => rest;
function flow(name, expect, options = {}, before, after) {
  scenarios[name] = {
    kind: "compare",
    expect,
    run: async (base) => {
      if (before) await before();
      try {
        return strip(await oidcFlow(base, options));
      } finally {
        if (after) await after();
      }
    },
  };
}
flow("oidc-sign-in-alice", flowOk("u-alice"));
flow("oidc-sign-in-bob", flowOk("u-bob", "http://SELF/reports"), { user: "bob", callbackUrl: "/reports" });
flow("oidc-wrong-state", flowRefused, { tamper: { callback: (u) => u.searchParams.set("state", "forged-state") } });
flow("oidc-no-state-cookie", flowRefused, { tamper: { jar: (j) => j.cookies.delete("authjs.state") } });
flow("oidc-no-pkce-cookie", flowRefused, { tamper: { jar: (j) => j.cookies.delete("authjs.pkce.code_verifier") } });
flow("oidc-no-nonce-cookie", flowRefused, { tamper: { jar: (j) => j.cookies.delete("authjs.nonce") } });
flow("oidc-forged-code", flowRefused, { tamper: { callback: (u) => u.searchParams.set("code", "0".repeat(32)) } });
flow("oidc-missing-code", flowRefused, { tamper: { callback: (u) => u.searchParams.delete("code") } });
flow("oidc-wrong-iss-parameter", flowRefused, { tamper: { callback: (u) => u.searchParams.set("iss", "http://evil.example") } });
for (const mode of ["wrong-iss", "wrong-aud", "wrong-nonce", "expired"]) {
  flow(`oidc-id-token-${mode}`, flowRefused, {}, () => oidcAdmin(`/admin/idtoken?mode=${mode}`), () => oidcAdmin("/admin/idtoken?mode=normal"));
}
// Auth.js (oauth4webapi) does not verify the signature of an ID token it
// received directly from the token endpoint (OpenID Connect Core 3.1.3.7
// lets TLS to the token endpoint stand in for it): a bad signature or an
// unknown kid still signs in, on next start as on NextRS. Asserted as
// such, so a change in that behaviour shows.
for (const mode of ["bad-signature", "unknown-kid"]) {
  flow(`oidc-id-token-${mode}-not-verified`, flowOk("u-alice"), {}, () => oidcAdmin(`/admin/idtoken?mode=${mode}`), () => oidcAdmin("/admin/idtoken?mode=normal"));
}
// The provider rotates its signing key: ID tokens of the new key verify.
flow(
  "oidc-key-rotation",
  flowOk("u-alice"),
  {},
  async () => {
    await oidcAdmin("/admin/rotate?kid=rs-1&to=rs-rotated");
    await oidcAdmin("/admin/idtoken?mode=normal&kid=rs-rotated");
  },
  () => oidcAdmin("/admin/reset"),
);
// A callback replayed after it succeeded: the code is spent and the state
// cookie gone; no second session.
scenarios["oidc-callback-replay"] = {
  kind: "compare",
  expect: (o) => flowOk("u-alice")(o.first) ?? (o.replay.session === null && o.replay.status === 302 ? null : JSON.stringify(o.replay)),
  run: async (base) => {
    const first = await oidcFlow(base);
    const fresh = new Jar();
    const r = await send(fresh, base, first.callbackHref);
    return { first: strip(first), replay: { status: r.status, location: shown(r.headers.get("location")), session: await readSession(fresh.get(SALT)) } };
  },
};

// --- A protected Server Action ------------------------------------------------------------------------
const manifest = JSON.parse(await import("node:fs").then((fs) => fs.readFileSync(join(project, ".next/server/server-reference-manifest.json"), "utf8")));
const recordId = Object.entries(manifest.node).find(([, v]) => v.exportedName === "recordProtected")?.[0];
const ledger = async (key) => (await (await fetch(`${upstream}/mutations?key=${encodeURIComponent(key)}`)).json()).ids;
function mutation(name, path, headers, expected) {
  scenarios[name] = {
    kind: "compare",
    expect: (o) => (JSON.stringify(o.ledger) === JSON.stringify(expected) ? null : `ledger ${JSON.stringify(o.ledger)}, expected ${JSON.stringify(expected)}`),
    run: async (base, side) => {
      const key = `authjs-${name}-${run}-${side}`;
      const form = new FormData();
      form.append("_1_key", key);
      form.append("_1_id", "1");
      form.append("0", '["$K1"]');
      const response = await look(base, { method: "POST", path, body: form, headers: { origin: base, "next-action": recordId, accept: "text/x-component", ...(await headers()) } });
      await sleep(100);
      return { response, ledger: await ledger(key) };
    },
  };
}
mutation("action-anon", "/", async () => ({}), []);
mutation("action-alice", "/actions/protected", () => withSession("u-alice", "Alice"), ["u-alice:1"]);
mutation("action-bob-public-page", "/", () => withSession("u-bob", "Bob"), ["u-bob:1"]);
mutation("action-expired", "/", () => withSession("u-alice", "Alice", { maxAge: EXPIRED }), []);
mutation("action-wrong-secret", "/", () => withSession("u-alice", "Alice", { secret: "another-secret-another-secret-00" }), []);

// --- Concurrent users ------------------------------------------------------------------------------------
scenarios["concurrent-users"] = {
  kind: "compare",
  expect: (o) => (o.total === 300 && o.wrong === 0 && o.failed === 0 ? null : JSON.stringify(o)),
  run: async (base) => {
    const people = [["u-alice", "Alice"], ["u-bob", "Bob"], ["u-carol", "Carol"], ["u-dave", "Dave"]];
    const cookies = await Promise.all(people.map(([sub, name]) => withSession(sub, name)));
    const expired = await withSession("u-erin", "Erin", { maxAge: EXPIRED });
    const total = 300;
    let next = 0;
    let wrong = 0;
    let failed = 0;
    const sample = [];
    async function worker() {
      while (next < total) {
        const i = next++;
        const who = i % 5;
        try {
          const r = await fetch(`${base}/api/private`, { headers: who === 4 ? expired : cookies[who] });
          const body = await r.json();
          const seen = r.status === 401 ? "anonymous" : body.userId;
          const expected = who === 4 ? "anonymous" : people[who][0];
          if (seen !== expected) {
            wrong++;
            if (sample.length < 3) sample.push({ i, expected, seen });
          }
        } catch {
          failed++;
        }
      }
    }
    await Promise.all(Array.from({ length: 40 }, worker));
    return { total, wrong, failed, sample };
  },
};

// --- After an executor restart ------------------------------------------------------------------------------
// NextRS restarts an executor that stalls past its limit (3 s in the
// suites): sessions are stateless, so the restarted executor accepts them
// and keeps refusing forged ones. next start has no such restart; it only
// runs the checks.
scenarios["session-after-executor-restart"] = {
  kind: "compare",
  expect: (o) => identity("u-alice")(o.alice) ?? anonymous(o.forged) ?? identity("u-bob")(o.bob),
  run: async (base, side) => {
    if (side === "candidate") {
      await fetch(`${base}/api/stall?ms=6000`).then((r) => r.arrayBuffer()).catch(() => null);
      await sleep(500);
    }
    return {
      alice: await look(base, { path: "/api/private", headers: await withSession("u-alice", "Alice") }),
      forged: await look(base, { path: "/api/private", headers: await withSession("u-alice", "Alice", { secret: "another-secret-another-secret-00" }) }),
      bob: await look(base, { path: "/api/private", headers: await withSession("u-bob", "Bob") }),
    };
  },
};

await fetch(`${oidc}/admin/reset`, { method: "POST" });
const failures = await runScenarios("authjs (next-auth 5)", scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
