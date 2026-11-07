// An API whose access tokens come from an OpenID provider, verified with
// jose's createRemoteJWKSet in proxy.ts and in route handlers, plus the
// application's own asymmetric (ES256) session and service tokens:
// fixtures/next-jwks-proxy, compared between next start and NextRS.
//
//   APP_KEYS=<json> node tests/compat/jwks-proxy.mjs --baseline URL --candidate URL \
//        --oidc URL --upstream URL --dist DIR [--isolates N] [--only a,b] [--json out.json]
//
// The provider is tests/compat/oidc-stub.mjs, shared by both servers; its
// tokens are signed with node:crypto, and the tokens NextRS signs are
// verified here with node:crypto: neither side's checks rely on jose. Every
// important scenario also asserts its absolute outcome (status, error code,
// identity, provider counters), so two servers agreeing on a wrong answer
// fail (see routing-lib.mjs, `expect`).
import { createPrivateKey, createPublicKey, randomBytes, sign as nodeSign, verify as nodeVerify } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { observe, parseArgs, runScenarios, sleep } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, oidc, upstream, dist } = args;
const isolates = Number(args.isolates ?? 1);
const APP_KEYS = process.env.APP_KEYS ? JSON.parse(process.env.APP_KEYS) : null;
if (!baseline || !candidate || !oidc || !upstream || !dist || !APP_KEYS) {
  console.error("usage: APP_KEYS=... jwks-proxy.mjs --baseline URL --candidate URL --oidc URL --upstream URL --dist DIR");
  process.exit(2);
}
const only = args.only ? new Set(args.only.split(",")) : null;
const run = `${Date.now().toString(36)}${randomBytes(2).toString("hex")}`;
const AUD = "nextrs-api";
// The fixture's jose cache: a fetched key set is fresh for 1.5 s, an
// unknown kid may refetch after 0.2 s.
const STALE_MS = 1700;

// --- Provider ---------------------------------------------------------------------
async function admin(path) {
  const r = await fetch(`${oidc}${path}`, { method: "POST" });
  if (!r.ok) throw new Error(`oidc ${path}: ${r.status}`);
  return r.json();
}
const stats = async () => (await fetch(`${oidc}/admin/stats`)).json();
async function mint(spec) {
  const r = await fetch(`${oidc}/mint`, { method: "POST", body: JSON.stringify(spec) });
  if (!r.ok) throw new Error(`mint: ${await r.text()}`);
  return (await r.json()).token;
}
const userClaims = (sub, extra = {}) => ({ sub, aud: AUD, name: sub === "u-alice" ? "Alice" : "Bob", ...extra });
const bearer = (token) => ({ authorization: `Bearer ${token}` });

// --- Application keys (the tokens NextRS signs) ------------------------------------
const b64u = (v) => Buffer.from(v).toString("base64url");
const SIGN = {
  ES256: (key, data) => nodeSign("sha256", data, { key, dsaEncoding: "ieee-p1363" }),
  PS256: (key, data) => nodeSign("sha256", data, { key, padding: 6, saltLength: 32 }),
  RS256: (key, data) => nodeSign("sha256", data, key),
  EdDSA: (key, data) => nodeSign(null, data, key),
};
const VERIFY = {
  ES256: (key, data, sig) => nodeVerify("sha256", data, { key, dsaEncoding: "ieee-p1363" }, sig),
  PS256: (key, data, sig) => nodeVerify("sha256", data, { key, padding: 6, saltLength: 32 }, sig),
  RS256: (key, data, sig) => nodeVerify("sha256", data, key, sig),
  EdDSA: (key, data, sig) => nodeVerify(null, data, key, sig),
};
const appPublic = (alg) => createPublicKey(APP_KEYS[alg].public);
const appPrivate = (alg) => createPrivateKey(APP_KEYS[alg].private);
const now = () => Math.floor(Date.now() / 1000);
function appToken(alg, claims, { signWith = alg, header = {} } = {}) {
  const input = `${b64u(JSON.stringify({ alg, ...header }))}.${b64u(JSON.stringify(claims))}`;
  return `${input}.${b64u(SIGN[signWith](appPrivate(signWith), Buffer.from(input)))}`;
}
// What a JWT NextRS signed says, checked with node:crypto.
function inspect(token, alg) {
  const [h, p, s] = String(token).split(".");
  if (!s) return { valid: false };
  const header = JSON.parse(Buffer.from(h, "base64url"));
  const claims = JSON.parse(Buffer.from(p, "base64url"));
  const valid = VERIFY[alg](appPublic(alg), Buffer.from(`${h}.${p}`), Buffer.from(s, "base64url"));
  const { iat, exp, ...stable } = claims;
  return { valid, header, claims: stable, lifetime: exp - iat };
}
const session = (userId, name, extra = {}) =>
  appToken("ES256", { userId, name, iss: "nextrs-jwks-app", aud: "nextrs-jwks-app", iat: now(), exp: now() + 3600, ...extra });
const cookie = (token) => ({ cookie: `session=${token}` });

// --- Observations -------------------------------------------------------------------
const redactCookie = (c) =>
  c.replace(/^session=[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/, "session=<JWT>").replace(/Expires=[^;]+/i, "Expires=<DATE>");
async function look(base, request) {
  const o = await observe(base, request);
  if (o.setCookie) o.setCookie = o.setCookie.map(redactCookie);
  return o;
}
const json = (o) => (typeof o.body === "string" ? JSON.parse(o.body) : o.body);

// Expectations.
const ok = (sub, alg) => (o) => {
  if (o.status !== 200) return `status ${o.status}, expected 200`;
  const body = json(o);
  if (body.sub !== sub || body.proxySub !== sub) return `identity ${body.sub}/${body.proxySub}, expected ${sub}`;
  if (alg && body.alg !== alg) return `alg ${body.alg}, expected ${alg}`;
  return null;
};
const refused = (status, code, claim) => (o) => {
  if (o.status !== status) return `status ${o.status}, expected ${status}`;
  const body = json(o);
  if (body.sub !== undefined) return "a refused request exposed an identity";
  if (code && body.code !== code) return `code ${body.code}, expected ${code}`;
  if (claim && body.claim !== claim) return `claim ${body.claim}, expected ${claim}`;
  return null;
};
const toLogin = (o) => (o.status === 307 && /^\/login\?next=/.test(o.headers.location ?? "") ? null : `status ${o.status} location ${o.headers.location}, expected a redirect to /login`);

const scenarios = {};
// A GET /api/me with a token minted per side.
function api(name, path, headers, expect, extra = {}) {
  scenarios[name] = { kind: "compare", expect, run: async (base) => look(base, { path, headers: await headers() }), ...extra };
}

// --- Valid tokens, one per algorithm -----------------------------------------------------
for (const [kid, alg] of [["rs-1", "RS256"], ["ps-1", "PS256"], ["es-1", "ES256"], ["es384-1", "ES384"], ["ed-1", "EdDSA"]]) {
  api(`valid-${alg}`, "/api/me", async () => bearer(await mint({ kid, claims: userClaims("u-alice") })), ok("u-alice", alg));
}
api("valid-bob-ES256", "/api/me", async () => bearer(await mint({ kid: "es-1", claims: userClaims("u-bob") })), ok("u-bob", "ES256"));
api("valid-no-kid", "/api/me", async () => bearer(await mint({ kid: "ed-1", header: { kid: null }, claims: userClaims("u-alice") })), ok("u-alice", "EdDSA"));
api("valid-audience-array", "/api/me", async () => bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice", { aud: ["other", AUD] }) })), ok("u-alice"));
api("valid-tenant-main", "/api/t/main/me", async () => bearer(await mint({ kid: "es-1", claims: userClaims("u-bob") })), (o) =>
  o.status === 200 && json(o).sub === "u-bob" && json(o).tenant === "main" ? null : `status ${o.status} body ${JSON.stringify(o.body)}`,
);

// --- Claims ---------------------------------------------------------------------------------
const CLAIM = "ERR_JWT_CLAIM_VALIDATION_FAILED";
api("expired", "/api/me", async () => bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice", { exp: now() - 30 }) })), refused(401, "ERR_JWT_EXPIRED", "exp"));
api("not-yet-valid", "/api/me", async () => bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice", { nbf: now() + 600 }) })), refused(401, CLAIM, "nbf"));
api("wrong-issuer", "/api/me", async () => bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice", { iss: "http://evil.example" }) })), refused(401, CLAIM, "iss"));
api("wrong-audience", "/api/me", async () => bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice", { aud: "another-api" }) })), refused(401, CLAIM, "aud"));
api("no-subject", "/api/me", async () => bearer(await mint({ kid: "es-1", claims: { aud: AUD } })), refused(401, CLAIM, "sub"));
api("no-expiry", "/api/me", async () => bearer(await mint({ kid: "es-1", claims: userClaims("u-alice", { exp: null }) })), refused(401, CLAIM, "exp"));
api("future-iat-is-fine", "/api/me", async () => bearer(await mint({ kid: "es-1", claims: userClaims("u-alice", { iat: now() + 5 }) })), ok("u-alice"));

// --- Signatures, algorithms, key selection ----------------------------------------------------
const SIG = "ERR_JWS_SIGNATURE_VERIFICATION_FAILED";
async function tamperPayload(kid) {
  const [h, , s] = (await mint({ kid, claims: userClaims("u-alice") })).split(".");
  return `${h}.${b64u(JSON.stringify({ ...userClaims("u-admin"), iss: oidc, exp: now() + 600 }))}.${s}`;
}
async function tamperSignature(kid) {
  const t = await mint({ kid, claims: userClaims("u-alice") });
  return t.slice(0, -6) + (t.slice(-6) === "AAAAAA" ? "BBBBBB" : "AAAAAA");
}
for (const kid of ["rs-1", "es-1", "ed-1"]) {
  api(`tampered-payload-${kid}`, "/api/me", async () => bearer(await tamperPayload(kid)), refused(401, SIG));
  api(`tampered-signature-${kid}`, "/api/me", async () => bearer(await tamperSignature(kid)), refused(401));
}
api("alg-none", "/api/me", async () => bearer(await mint({ confuse: "none", claims: userClaims("u-alice") })), refused(401));
api("alg-confusion-hs256-public-key", "/api/me", async () => bearer(await mint({ kid: "rs-1", confuse: "hs256-public-key", claims: userClaims("u-alice") })), refused(401));
// The header names one algorithm, the key (by kid) is of another type.
api("alg-header-rs256-on-ec-key", "/api/me", async () => bearer(await mint({ kid: "es-1", alg: "RS256", claims: userClaims("u-alice") })), refused(401, "ERR_JWKS_NO_MATCHING_KEY"));
// Same key type, but the JWK's own alg (RS256) differs from the header's.
api("alg-header-ps256-on-rs256-key", "/api/me", async () => bearer(await mint({ kid: "rs-1", alg: "PS256", claims: userClaims("u-alice") })), refused(401, "ERR_JWKS_NO_MATCHING_KEY"));
api("alg-not-allowed-hs256", "/api/me", async () => bearer(await mint({ kid: "rs-1", alg: "HS256", claims: userClaims("u-alice") })), refused(401));
api("unknown-kid", "/api/me", async () => bearer(await mint({ kid: "never-published", like: "rs-1", claims: userClaims("u-alice") })), refused(401, "ERR_JWKS_NO_MATCHING_KEY"));
api("malformed-token", "/api/me", async () => bearer("not.a-jwt"), refused(401));
api("no-authorization", "/api/me", async () => ({}), refused(401, "no-bearer-token"));
api("basic-authorization", "/api/me", async () => ({ authorization: "Basic YWxpY2U6c2VjcmV0" }), refused(401, "no-bearer-token"));
api("spoofed-identity-header", "/api/me", async () => ({ ...bearer(await mint({ kid: "es-1", claims: userClaims("u-bob") })), "x-auth-sub": "u-alice" }), ok("u-bob"));
api("spoofed-identity-header-anon", "/api/me", async () => ({ "x-auth-sub": "u-alice" }), refused(401));
// A token pointing at another key set (jku): never fetched, the
// application's own provider decides.
scenarios["jku-header-ignored"] = {
  kind: "compare",
  expect: (o) => ok("u-alice")(o.response) ?? (o.attackerFetches === 0 ? null : `the jku key set was fetched ${o.attackerFetches} times`),
  run: async (base) => {
    const before = (await stats()).attackerHits;
    const token = await mint({ kid: "rs-1", header: { jku: `${oidc}/attacker/jwks` }, claims: userClaims("u-alice") });
    const response = await look(base, { path: "/api/me", headers: bearer(token) });
    return { response, attackerFetches: (await stats()).attackerHits - before };
  },
};

// --- The key set: rotation, several keys, caching ----------------------------------------------
// Rotation: rs-1 is replaced by rs-2. A token of the new key verifies at
// once (an unknown kid refetches); one of the retired key stops verifying
// once every isolate's cached set is stale.
scenarios["key-rotation"] = {
  kind: "compare",
  expect: (o) =>
    ok("u-alice", "RS256")(o.before) ?? ok("u-bob", "RS256")(o.newKey) ?? refused(401, "ERR_JWKS_NO_MATCHING_KEY")(o.retiredKey) ?? ok("u-alice")(o.otherKeysUnchanged),
  run: async (base) => {
    await admin("/admin/reset");
    await sleep(STALE_MS);
    const before = await look(base, { path: "/api/me", headers: bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice") })) });
    await admin("/admin/rotate?kid=rs-1&to=rs-2");
    await sleep(250);
    const newKey = await look(base, { path: "/api/me", headers: bearer(await mint({ kid: "rs-2", claims: userClaims("u-bob") })) });
    await sleep(STALE_MS);
    // Every isolate has to see it: one request per isolate and more.
    const retired = [];
    for (let i = 0; i < isolates * 3; i++) retired.push(await look(base, { path: "/api/me", headers: bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice") })) }));
    const retiredKey = retired.find((o) => o.status !== 401) ?? retired[0];
    const otherKeysUnchanged = await look(base, { path: "/api/me", headers: bearer(await mint({ kid: "es-1", claims: userClaims("u-alice") })) });
    await admin("/admin/reset");
    await sleep(STALE_MS);
    return { before, newKey, retiredKey, otherKeysUnchanged };
  },
};
scenarios["several-keys-same-type"] = {
  kind: "compare",
  expect: (o) => ok("u-alice", "RS256")(o.first) ?? ok("u-bob", "RS256")(o.second),
  run: async (base) => {
    await admin(`/admin/add?kid=rs-extra&like=rs-1`);
    await sleep(250);
    const first = await look(base, { path: "/api/me", headers: bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice") })) });
    const second = await look(base, { path: "/api/me", headers: bearer(await mint({ kid: "rs-extra", claims: userClaims("u-bob") })) });
    await admin("/admin/reset");
    return { first, second };
  },
};
// jose caches the set per isolate: a burst of verifications fetches it at
// most once per isolate (plus one refresh if the burst crosses staleness).
scenarios["key-set-is-cached"] = {
  kind: "compare",
  expect: (o) => (o.statuses.every((s) => s === 200) && o.bounded ? null : `statuses ${o.statuses} fetches ${o.fetches}`),
  run: async (base, side) => {
    await sleep(STALE_MS);
    const before = (await stats()).jwksHits;
    const token = await mint({ kid: "es-1", claims: userClaims("u-alice") });
    const statuses = [];
    for (let i = 0; i < 40; i++) statuses.push((await fetch(`${base}/api/me`, { headers: bearer(token) })).status);
    const fetches = (await stats()).jwksHits - before;
    // next start runs one process (one cache per worker); NextRS one per isolate.
    const bound = side === "baseline" ? 2 : 2 * isolates;
    return { statuses: [...new Set(statuses)], bounded: fetches >= 1 && fetches <= bound };
  },
};

// --- A misbehaving provider -------------------------------------------------------------------
// The mode applies once the cached set is stale; the fixture answers 503
// (provider unavailable) or 401 (no usable key), never an acceptance.
function providerMode(name, mode, expect, kind = "compare", reason) {
  scenarios[name] = {
    kind,
    reason,
    ...(kind === "known-diff" ? { expectCandidate: expect } : { expect }),
    run: async (base) => {
      await sleep(STALE_MS);
      await admin(`/admin/mode?jwks=${mode}`);
      const before = await stats();
      const started = Date.now();
      let response;
      try {
        response = await look(base, { path: "/api/me", headers: bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice") })) });
      } finally {
        await admin("/admin/mode?jwks=normal");
      }
      const after = await stats();
      const elapsed = Date.now() - started;
      await sleep(STALE_MS);
      const recovered = await look(base, { path: "/api/me", headers: bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice") })) });
      return { response, recovered: recovered.status, attackerFetches: after.attackerHits - before.attackerHits, elapsedUnder: elapsed < 2500 ? "2.5s" : "more" };
    },
  };
}
const unavailable = (code) => (o) => refused(503, code)(o.response) ?? (o.recovered === 200 ? null : `not recovered (${o.recovered})`);
providerMode("jwks-unavailable", "unavailable", unavailable("ERR_JOSE_GENERIC"));
providerMode("jwks-malformed", "malformed", unavailable("ERR_JOSE_GENERIC"));
providerMode("jwks-empty", "empty", (o) => refused(401, "ERR_JWKS_NO_MATCHING_KEY")(o.response) ?? (o.recovered === 200 ? null : "not recovered"));
// jose's timeoutDuration (1 s) ends a provider that hangs (3 s).
providerMode("jwks-timeout", "slow", unavailable("ERR_JWKS_TIMEOUT"));
// A redirect is not followed (jose fetches with redirect: "manual").
providerMode("jwks-redirect-not-followed", "redirect", (o) =>
  unavailable("ERR_JOSE_GENERIC")(o) ?? (o.attackerFetches === 0 ? null : "the redirect was followed"),
);
// 40 MiB of keys: next start reads them all (and the token verifies);
// NextRS ends the body at its fetch response limit (32 MiB), jose cannot
// parse what it read, and the provider counts as unavailable.
providerMode(
  "jwks-huge-response",
  "huge",
  (o) => refused(503, "ERR_JOSE_GENERIC")(o.response) ?? (o.recovered === 200 ? null : "not recovered"),
  "known-diff",
  "NextRS bounds every fetch's response (--net-max-response, 32 MiB); next start reads a 40 MiB key set",
);

// --- The network policy ---------------------------------------------------------------------------
// An issuer the policy does not list: next start fetches it (that port
// serves no key set: 503 from jose), NextRS refuses the fetch.
scenarios["tenant-unlisted-issuer"] = {
  kind: "known-diff",
  reason: "NextRS's network policy refuses the unlisted issuer (fetch-failed); next start reaches it",
  expectCandidate: refused(503, "fetch-failed"),
  run: async (base) => look(base, { path: "/api/t/unlisted/me", headers: bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice") })) }),
};
// A cloud metadata address: refused before any connection, so well before
// jose's 1 s timeout.
scenarios["tenant-metadata-address"] = {
  kind: "candidate",
  run: async (base) => {
    const started = Date.now();
    const o = await look(base, { path: "/api/t/metadata/me", headers: bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice") })) });
    const elapsed = Date.now() - started;
    const wrong = refused(503, "fetch-failed")(o);
    return { ok: !wrong && elapsed < 900, detail: { wrong, elapsed, observed: o } };
  },
};
api("tenant-unknown", "/api/t/nobody/me", async () => bearer(await mint({ kid: "rs-1", claims: userClaims("u-alice") })), refused(401, "unknown-tenant"));

// --- Cancellation ------------------------------------------------------------------------------------
// Clients that leave while the proxy waits for a slow provider: the server
// stays healthy and the next request is right.
scenarios["client-abort-during-jwks-fetch"] = {
  kind: "compare",
  expect: (o) => (o.aborted === 4 && o.after === 200 ? null : JSON.stringify(o)),
  run: async (base) => {
    await sleep(STALE_MS);
    await admin("/admin/mode?jwks=slow");
    const token = await mint({ kid: "rs-1", claims: userClaims("u-alice") });
    let aborted = 0;
    await Promise.all(
      Array.from({ length: 4 }, async () => {
        const controller = new AbortController();
        setTimeout(() => controller.abort(), 200);
        await fetch(`${base}/api/me`, { headers: bearer(token), signal: controller.signal }).catch(() => aborted++);
      }),
    );
    await admin("/admin/mode?jwks=normal");
    await sleep(STALE_MS);
    const after = (await fetch(`${base}/api/me`, { headers: bearer(token) })).status;
    return { aborted, after };
  },
};

// --- The application's own asymmetric tokens ------------------------------------------------------
const manifest = JSON.parse(readFileSync(join(dist, "server/server-reference-manifest.json"), "utf8"));
const actionId = (name, file) => {
  const found = Object.entries(manifest.node).find(([, v]) => v.exportedName === name && v.filename === file);
  if (!found) throw new Error(`no action ${name} in ${file}`);
  return found[0];
};
const ids = {
  login: actionId("login", "app/login/actions.ts"),
  record: actionId("recordProtected", "app/actions/protected/actions.ts"),
};
function actionRequest(base, path, id, fields, headers = {}) {
  const form = new FormData();
  for (const [name, value] of Object.entries(fields)) form.append(`_1_${name}`, value);
  form.append("0", '["$K1"]');
  return { method: "POST", path, body: form, headers: { origin: base, "next-action": id, accept: "text/x-component", ...headers } };
}

// Login signs an ES256 session; its signature is checked with node:crypto
// and the cookie must open the dashboard.
scenarios["login-signs-es256-session"] = {
  kind: "compare",
  expect: (o) =>
    o.status !== 303 && o.status !== 200
      ? `login status ${o.status}`
      : !o.session?.valid
        ? "the session's ES256 signature does not verify with node:crypto"
        : JSON.stringify(o.session.claims) !== JSON.stringify({ userId: "u-alice", name: "Alice", iss: "nextrs-jwks-app", aud: "nextrs-jwks-app" })
          ? `session claims ${JSON.stringify(o.session.claims)}`
          : o.dashboard?.body?.info !== JSON.stringify({ userId: "u-alice", proxyUser: "u-alice" })
            ? `dashboard ${JSON.stringify(o.dashboard?.body)}`
            : null,
  run: async (base) => {
    const request = actionRequest(base, "/login", ids.login, { username: "alice", password: "alice-test-password", next: "/dashboard" });
    const raw = await fetch(`${base}/login`, { method: "POST", body: request.body, headers: request.headers, redirect: "manual" });
    await raw.text();
    const set = raw.headers.getSetCookie().find((c) => c.startsWith("session="));
    const token = set?.split(";")[0].slice("session=".length);
    const out = { status: raw.status, session: token ? inspect(token, "ES256") : null };
    if (token) out.dashboard = await look(base, { path: "/dashboard", headers: cookie(token) });
    return out;
  },
};
for (const alg of ["ES256", "PS256", "RS256", "EdDSA"]) {
  scenarios[`app-signs-${alg}`] = {
    kind: "compare",
    expect: (o) =>
      o.status !== 200 || !o.token?.valid
        ? `status ${o.status}, signature ${o.token?.valid ? "valid" : "invalid"} (node:crypto)`
        : o.token.header.alg !== alg || o.token.claims.sub !== "u-bob" || o.token.lifetime !== 300
          ? `token ${JSON.stringify(o.token)}`
          : null,
    run: async (base) => {
      const r = await fetch(`${base}/api/app-token?alg=${alg}`, { headers: cookie(session("u-bob", "Bob")) });
      const body = await r.json().catch(() => null);
      return { status: r.status, token: body?.token ? inspect(body.token, alg) : null };
    },
  };
}
const appClaims = (sub) => ({ sub, iss: "nextrs-jwks-app", aud: "nextrs-services", iat: now(), exp: now() + 300 });
function appVerify(name, token, expect) {
  scenarios[name] = {
    kind: "compare",
    expect,
    run: async (base) => look(base, { method: "POST", path: "/api/app-token/verify", headers: { authorization: `Bearer ${token()}` } }),
  };
}
const verified = (alg) => (o) => (o.status === 200 && json(o).ok && json(o).sub === "u-alice" && json(o).alg === alg ? null : `status ${o.status} ${JSON.stringify(o.body)}`);
const notVerified = (code) => (o) => (o.status === 401 && !json(o).ok && (!code || json(o).code === code) ? null : `status ${o.status} ${JSON.stringify(o.body)}`);
for (const alg of ["ES256", "PS256", "RS256", "EdDSA"]) {
  appVerify(`app-verifies-${alg}`, () => appToken(alg, appClaims("u-alice")), verified(alg));
  appVerify(`app-rejects-tampered-${alg}`, () => {
    const [h, , s] = appToken(alg, appClaims("u-alice")).split(".");
    return `${h}.${b64u(JSON.stringify(appClaims("u-admin")))}.${s}`;
  }, notVerified(SIG));
}
// ES256 in the header, signed with the PS256 key: the ES256 key decides.
appVerify("app-rejects-wrong-key", () => appToken("ES256", appClaims("u-alice"), { signWith: "EdDSA" }), notVerified());
appVerify("app-rejects-hs256", () => {
  const input = `${b64u(JSON.stringify({ alg: "HS256" }))}.${b64u(JSON.stringify(appClaims("u-alice")))}`;
  return `${input}.${b64u(Buffer.from("x"))}`;
}, notVerified("alg-not-allowed"));
appVerify("app-rejects-expired", () => appToken("EdDSA", { ...appClaims("u-alice"), exp: now() - 10 }), notVerified("ERR_JWT_EXPIRED"));

// --- Pages and the session cookie --------------------------------------------------------------------
function page(name, path, headers, expect) {
  scenarios[name] = { kind: "compare", expect, run: async (base) => look(base, { path, headers: headers() }) };
}
const shows = (userId) => (o) => (o.status === 200 && o.body?.info === JSON.stringify({ userId, proxyUser: userId }) ? null : `status ${o.status} ${JSON.stringify(o.body)}`);
const servedSecret = (o) => (o.status === 200 && /proxy-protected/.test(o.body?.secret ?? "") ? null : `status ${o.status} ${JSON.stringify(o.body)}`);
page("anon-dashboard", "/dashboard", () => ({}), toLogin);
page("anon-static", "/account", () => ({}), toLogin);
page("anon-isr", "/reports", () => ({}), toLogin);
page("session-dashboard-alice", "/dashboard", () => cookie(session("u-alice", "Alice")), shows("u-alice"));
page("session-dashboard-bob", "/dashboard", () => cookie(session("u-bob", "Bob")), shows("u-bob"));
page("session-static", "/account", () => cookie(session("u-alice", "Alice")), servedSecret);
page("session-isr", "/reports", () => cookie(session("u-bob", "Bob")), servedSecret);
page("session-expired", "/dashboard", () => cookie(session("u-alice", "Alice", { exp: now() - 5 })), toLogin);
page("session-wrong-key", "/dashboard", () => {
  const t = appToken("ES256", { userId: "u-alice", name: "Alice", iss: "nextrs-jwks-app", aud: "nextrs-jwks-app", iat: now(), exp: now() + 60 });
  // Re-signed with a key the application does not trust.
  const [h, p] = t.split(".");
  const other = createPrivateKey(APP_KEYS.PS256.private);
  return cookie(`${h}.${p}.${b64u(nodeSign("sha256", Buffer.from(`${h}.${p}`), { key: other, padding: 6, saltLength: 32 }))}`);
}, toLogin);
page("session-tampered", "/dashboard", () => {
  const [h, , s] = session("u-alice", "Alice").split(".");
  return cookie(`${h}.${b64u(JSON.stringify({ userId: "u-admin", name: "Admin", iss: "nextrs-jwks-app", aud: "nextrs-jwks-app", exp: now() + 60 }))}.${s}`);
}, toLogin);
page("session-wrong-issuer", "/dashboard", () => cookie(session("u-alice", "Alice", { iss: "someone-else" })), toLogin);
page("session-provider-token-as-cookie", "/dashboard", () => ({}), toLogin);
scenarios["session-provider-token-as-cookie"].run = async (base) =>
  look(base, { path: "/dashboard", headers: cookie(await mint({ kid: "es-1", claims: { userId: "u-alice", name: "Alice", aud: "nextrs-jwks-app" } })) });

// --- A protected Server Action, with its ledger -------------------------------------------------------
const ledger = async (key) => (await (await fetch(`${upstream}/mutations?key=${encodeURIComponent(key)}`)).json()).ids;
function mutation(name, path, headers, expected) {
  scenarios[name] = {
    kind: "compare",
    expect: (o) => (JSON.stringify(o.ledger) === JSON.stringify(expected) ? null : `ledger ${JSON.stringify(o.ledger)}, expected ${JSON.stringify(expected)}`),
    run: async (base, side) => {
      const key = `jwks-${name}-${run}-${side}`;
      const response = await look(base, actionRequest(base, path, ids.record, { key, id: "1" }, headers()));
      await sleep(100);
      return { response, ledger: await ledger(key) };
    },
  };
}
mutation("action-anon", "/", () => ({}), []);
mutation("action-alice", "/actions/protected", () => cookie(session("u-alice", "Alice")), ["u-alice:1"]);
mutation("action-bob-public-page", "/", () => cookie(session("u-bob", "Bob")), ["u-bob:1"]);
mutation("action-expired", "/", () => cookie(session("u-alice", "Alice", { exp: now() - 5 })), []);
mutation("action-bearer-is-not-a-session", "/", () => ({ authorization: "Bearer x" }), []);

// --- Concurrency: identities never cross, across algorithms and isolates --------------------------------
scenarios["concurrent-identities"] = {
  kind: "compare",
  expect: (o) => (o.wrong === 0 && o.failed === 0 && o.total === 300 ? null : JSON.stringify(o)),
  run: async (base) => {
    const people = [
      ["u-alice", "rs-1"],
      ["u-bob", "es-1"],
      ["u-carol", "ed-1"],
      ["u-dave", "ps-1"],
      ["u-erin", "es384-1"],
    ];
    const tokens = await Promise.all(people.map(([sub, kid]) => mint({ kid, claims: { sub, aud: AUD } })));
    const bad = await mint({ kid: "rs-1", claims: { sub: "u-mallory", aud: "wrong" } });
    const total = 300;
    let next = 0;
    let wrong = 0;
    let failed = 0;
    const sample = [];
    async function worker() {
      while (next < total) {
        const i = next++;
        const who = i % 6;
        try {
          const r = await fetch(`${base}/api/me`, { headers: bearer(who === 5 ? bad : tokens[who]) });
          const body = await r.json();
          const expected = who === 5 ? "refused" : people[who][0];
          const seen = r.status === 401 ? "refused" : `${body.sub}/${body.proxySub}`;
          if (seen !== (who === 5 ? "refused" : `${expected}/${expected}`)) {
            wrong++;
            if (sample.length < 3) sample.push({ i, expected, seen });
          }
        } catch {
          failed++;
        }
      }
    }
    await Promise.all(Array.from({ length: 50 }, worker));
    return { total, wrong, failed, sample };
  },
};

await admin("/admin/reset");
const failures = await runScenarios("jwks-proxy (jose JWKS)", scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
