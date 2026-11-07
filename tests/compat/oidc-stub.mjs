// A local OpenID provider for the authentication fixtures: what an
// application's proxy and route handlers talk to (discovery, JWKS, the
// authorization code flow with PKCE and nonce, userinfo), and what the
// suites use to mint tokens and to make the provider misbehave. Both
// servers under comparison use the same stub, so its state (keys, modes,
// counters) is shared by them. No real accounts or credentials: keys are
// generated at start, users are synthetic.
//
//   node tests/compat/oidc-stub.mjs --port 5600 [--client-id ID --client-secret S]
//
// Tokens are signed here with node:crypto, independently of jose, so the
// suites test the fixtures' verification against a second implementation.
//
// Provider endpoints (the issuer is http://127.0.0.1:PORT):
//   GET  /.well-known/openid-configuration
//   GET  /jwks                         the JSON Web Key Set (see modes)
//   GET  /authorize?...&login_hint=U   approves user U at once: 302 to redirect_uri with code and state
//   POST /token                        authorization_code (PKCE S256, client_secret_basic/post)
//   GET  /userinfo                     Bearer access token -> claims
// Test endpoints:
//   POST /mint      {claims, kid, alg?, header?, confuse?, like?} -> {token}; claims get iss/iat/exp
//                   defaults; header members set to null are removed
//   POST /admin/rotate?kid=K&to=K2      replaces key K with a fresh key K2 (same type)
//   POST /admin/add?kid=K&like=K0       publishes a fresh key K of K0's type next to the others
//   POST /admin/mode?jwks=M             M: normal | malformed | unavailable | slow | redirect | huge | empty
//   POST /admin/idtoken?mode=M[&kid=K]  the next id_token: normal | wrong-iss | wrong-aud | wrong-nonce |
//                                       expired | bad-signature | unknown-kid; kid: the key id_tokens are
//                                       signed with from now on (default rs-1)
//   POST /admin/reset                   keys back to the initial set, modes normal, counters kept
//   GET  /admin/stats                   {jwksHits, attackerHits, tokenRequests, ...}
//   GET  /attacker/jwks                 a key set nobody should fetch (a token's jku points here)
import { createHash, createHmac, generateKeyPairSync, randomBytes, sign as nodeSign } from "node:crypto";
import http from "node:http";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const port = Number(args.port ?? 5600);
const issuer = `http://127.0.0.1:${port}`;
const clientId = args["client-id"] ?? process.env.OIDC_CLIENT_ID ?? "nextrs-test-client";
const clientSecret = args["client-secret"] ?? process.env.OIDC_CLIENT_SECRET ?? "";

const USERS = {
  alice: { sub: "u-alice", name: "Alice", email: "alice@example.test" },
  bob: { sub: "u-bob", name: "Bob", email: "bob@example.test" },
};

// --- Keys -------------------------------------------------------------------
const TYPES = {
  RS256: () => generateKeyPairSync("rsa", { modulusLength: 2048 }),
  PS256: () => generateKeyPairSync("rsa", { modulusLength: 2048 }),
  ES256: () => generateKeyPairSync("ec", { namedCurve: "P-256" }),
  ES384: () => generateKeyPairSync("ec", { namedCurve: "P-384" }),
  EdDSA: () => generateKeyPairSync("ed25519"),
};
const INITIAL = [
  ["rs-1", "RS256"],
  ["ps-1", "PS256"],
  ["es-1", "ES256"],
  ["es384-1", "ES384"],
  ["ed-1", "EdDSA"],
];
const made = new Map(); // kid -> {alg, privateKey, publicKey}: every key ever made (retired ones still sign)
let published = []; // kids in the JWKS
const makeKey = (kid, alg) => {
  const pair = TYPES[alg]();
  made.set(kid, { alg, ...pair });
};
function reset() {
  published = [];
  for (const [kid, alg] of INITIAL) {
    if (!made.has(kid)) makeKey(kid, alg);
    published.push(kid);
  }
}
reset();

const jwk = (kid) => {
  const key = made.get(kid);
  return { ...key.publicKey.export({ format: "jwk" }), kid, alg: key.alg, use: "sig" };
};

const b64u = (value) => Buffer.from(value).toString("base64url");
function signJws(alg, key, input) {
  const data = Buffer.from(input);
  switch (alg) {
    case "RS256":
      return nodeSign("sha256", data, key);
    case "PS256":
      return nodeSign("sha256", data, { key, padding: 6 /* RSA_PKCS1_PSS_PADDING */, saltLength: 32 });
    case "ES256":
      return nodeSign("sha256", data, { key, dsaEncoding: "ieee-p1363" });
    case "ES384":
      return nodeSign("sha384", data, { key, dsaEncoding: "ieee-p1363" });
    case "EdDSA":
      return nodeSign(null, data, key);
    default:
      throw new Error(`cannot sign ${alg}`);
  }
}

const now = () => Math.floor(Date.now() / 1000);
// claims get iss, iat and exp unless given (null removes one).
// `like`: a kid never made before becomes a fresh, unpublished key of
// that kid's type (a token from a key the provider never published).
function mint({ claims = {}, kid = "rs-1", alg, header = {}, confuse, like } = {}) {
  if (!made.has(kid) && like && made.has(like)) makeKey(kid, made.get(like).alg);
  const key = made.get(kid);
  if (!key && !confuse) throw new Error(`unknown kid ${kid}`);
  const body = { iss: issuer, iat: now(), exp: now() + 600, ...claims };
  for (const [name, value] of Object.entries(body)) if (value === null) delete body[name];
  if (confuse === "none") {
    return `${b64u(JSON.stringify({ alg: "none", ...header }))}.${b64u(JSON.stringify(body))}.`;
  }
  if (confuse === "hs256-public-key") {
    // Algorithm confusion: HS256 keyed with the RSA public key's PEM.
    const secret = made.get(kid).publicKey.export({ type: "spki", format: "pem" });
    const input = `${b64u(JSON.stringify({ alg: "HS256", kid, ...header }))}.${b64u(JSON.stringify(body))}`;
    return `${input}.${createHmac("sha256", secret).update(input).digest("base64url")}`;
  }
  const signAlg = alg ?? key.alg;
  const protectedHeader = { alg: signAlg, kid, typ: "JWT", ...header };
  for (const [name, value] of Object.entries(protectedHeader)) if (value === null) delete protectedHeader[name];
  const input = `${b64u(JSON.stringify(protectedHeader))}.${b64u(JSON.stringify(body))}`;
  // `alg` naming another algorithm than the key's: signed with the key's own.
  return `${input}.${b64u(signJws(key.alg, key.privateKey, input))}`;
}

// --- Modes and counters ---------------------------------------------------------
let jwksMode = "normal";
let idTokenMode = "normal";
let idTokenKid = "rs-1";
const stats = { jwksHits: 0, jwksAborted: 0, attackerHits: 0, authorizeRequests: 0, tokenRequests: 0, tokenFailures: 0, userinfoRequests: 0 };
const codes = new Map(); // code -> {user, redirectUri, challenge, nonce, clientId, used}
const accessTokens = new Map(); // token -> user

function send(res, status, body, headers = {}) {
  const text = typeof body === "string" ? body : JSON.stringify(body);
  res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store", ...headers });
  res.end(text);
}

async function readBody(req) {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  return Buffer.concat(chunks).toString("utf8");
}

function serveJwks(req, res) {
  stats.jwksHits++;
  const keys = { keys: published.map(jwk) };
  res.on("close", () => {
    if (!res.writableFinished) stats.jwksAborted++;
  });
  switch (jwksMode) {
    case "malformed":
      return send(res, 200, '{"keys": [{"kty": "RSA", "kid": ');
    case "unavailable":
      return send(res, 503, { error: "unavailable" });
    case "slow":
      setTimeout(() => !res.destroyed && send(res, 200, keys), 3000);
      return;
    case "redirect":
      res.writeHead(302, { location: `${issuer}/attacker/jwks` });
      return res.end();
    case "huge": {
      // 40 MiB of keys (above NextRS's default 32 MiB fetch limit), streamed: a provider (or an attacker) answering
      // with an unbounded body.
      res.writeHead(200, { "content-type": "application/json" });
      const filler = JSON.stringify({ ...jwk("rs-1"), kid: "filler" });
      let sent = 0;
      res.write('{"keys":[');
      const write = () => {
        while (sent < 40 * 1024 * 1024) {
          sent += filler.length + 1;
          if (!res.write(`${filler},`)) return res.once("drain", write);
        }
        res.end(`${JSON.stringify(jwk("rs-1"))}]}`);
      };
      return write();
    }
    case "empty":
      return send(res, 200, { keys: [] });
    default:
      return send(res, 200, keys);
  }
}

function idToken(user, { nonce, aud }) {
  const mode = idTokenMode;
  idTokenMode = "normal";
  const claims = { sub: user.sub, name: user.name, email: user.email, aud, nonce };
  if (mode === "wrong-iss") claims.iss = "http://evil.example";
  if (mode === "wrong-aud") claims.aud = "another-client";
  if (mode === "wrong-nonce") claims.nonce = "not-the-nonce";
  if (mode === "expired") claims.exp = now() - 120;
  if (mode === "expired") claims.iat = now() - 300;
  if (mode === "unknown-kid") {
    makeKey("never-published", "RS256");
    return mint({ claims, kid: "never-published" });
  }
  const token = mint({ claims, kid: idTokenKid });
  if (mode === "bad-signature") return token.slice(0, -4) + (token.endsWith("AAAA") ? "BBBB" : "AAAA");
  return token;
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, issuer);
  const p = url.searchParams;
  try {
    if (url.pathname === "/.well-known/openid-configuration") {
      return send(res, 200, {
        issuer,
        authorization_endpoint: `${issuer}/authorize`,
        token_endpoint: `${issuer}/token`,
        userinfo_endpoint: `${issuer}/userinfo`,
        jwks_uri: `${issuer}/jwks`,
        response_types_supported: ["code"],
        subject_types_supported: ["public"],
        id_token_signing_alg_values_supported: ["RS256", "PS256", "ES256", "ES384", "EdDSA"],
        token_endpoint_auth_methods_supported: ["client_secret_basic", "client_secret_post"],
        code_challenge_methods_supported: ["S256"],
        scopes_supported: ["openid", "profile", "email"],
      });
    }
    if (url.pathname === "/jwks") return serveJwks(req, res);
    if (url.pathname === "/attacker/jwks") {
      stats.attackerHits++;
      return send(res, 200, { keys: [] });
    }
    if (url.pathname === "/authorize") {
      stats.authorizeRequests++;
      const user = USERS[p.get("login_hint") ?? "alice"];
      const redirectUri = p.get("redirect_uri");
      if (!user || !redirectUri || p.get("response_type") !== "code" || p.get("client_id") !== clientId) {
        return send(res, 400, { error: "invalid_request" });
      }
      const code = randomBytes(16).toString("hex");
      codes.set(code, { user, redirectUri, challenge: p.get("code_challenge"), method: p.get("code_challenge_method"), nonce: p.get("nonce"), used: false });
      const target = new URL(redirectUri);
      target.searchParams.set("code", code);
      if (p.has("state")) target.searchParams.set("state", p.get("state"));
      target.searchParams.set("iss", issuer);
      res.writeHead(302, { location: target.href });
      return res.end();
    }
    if (url.pathname === "/token" && req.method === "POST") {
      stats.tokenRequests++;
      const form = new URLSearchParams(await readBody(req));
      let id = form.get("client_id");
      let secret = form.get("client_secret");
      const basic = /^Basic (.+)$/.exec(req.headers.authorization ?? "");
      if (basic) [id, secret] = Buffer.from(basic[1], "base64").toString().split(":").map(decodeURIComponent);
      const entry = codes.get(form.get("code") ?? "");
      const verifier = form.get("code_verifier") ?? "";
      const pkceOk = !entry?.challenge || createHash("sha256").update(verifier).digest("base64url") === entry.challenge;
      if (id !== clientId || (clientSecret && secret !== clientSecret) || !entry || entry.used || entry.redirectUri !== form.get("redirect_uri") || !pkceOk) {
        stats.tokenFailures++;
        return send(res, 400, { error: "invalid_grant" });
      }
      entry.used = true;
      const access = randomBytes(24).toString("base64url");
      accessTokens.set(access, entry.user);
      return send(res, 200, {
        access_token: access,
        token_type: "Bearer",
        expires_in: 600,
        scope: "openid profile email",
        id_token: idToken(entry.user, { nonce: entry.nonce ?? undefined, aud: clientId }),
      });
    }
    if (url.pathname === "/userinfo") {
      stats.userinfoRequests++;
      const user = accessTokens.get(/^Bearer (.+)$/.exec(req.headers.authorization ?? "")?.[1] ?? "");
      if (!user) return send(res, 401, { error: "invalid_token" });
      return send(res, 200, user);
    }
    if (url.pathname === "/mint" && req.method === "POST") {
      return send(res, 200, { token: mint(JSON.parse((await readBody(req)) || "{}")) });
    }
    if (url.pathname === "/admin/rotate" && req.method === "POST") {
      const from = p.get("kid");
      const to = p.get("to");
      if (!made.has(from) || !to) return send(res, 400, { error: "unknown kid" });
      if (!made.has(to)) makeKey(to, made.get(from).alg);
      published = published.map((kid) => (kid === from ? to : kid));
      return send(res, 200, { published });
    }
    if (url.pathname === "/admin/add" && req.method === "POST") {
      const kid = p.get("kid");
      const like = p.get("like") ?? "rs-1";
      if (!made.has(kid)) makeKey(kid, made.get(like).alg);
      if (!published.includes(kid)) published.push(kid);
      return send(res, 200, { published });
    }
    if (url.pathname === "/admin/mode" && req.method === "POST") {
      jwksMode = p.get("jwks") ?? "normal";
      return send(res, 200, { jwks: jwksMode });
    }
    if (url.pathname === "/admin/idtoken" && req.method === "POST") {
      idTokenMode = p.get("mode") ?? "normal";
      if (p.has("kid")) idTokenKid = p.get("kid");
      return send(res, 200, { idToken: idTokenMode, kid: idTokenKid });
    }
    if (url.pathname === "/admin/reset" && req.method === "POST") {
      reset();
      jwksMode = "normal";
      idTokenMode = "normal";
      idTokenKid = "rs-1";
      return send(res, 200, { published });
    }
    if (url.pathname === "/admin/stats") return send(res, 200, { ...stats, published, jwksMode });
    return send(res, 404, { error: "not found" });
  } catch (error) {
    return send(res, 500, { error: String(error?.message ?? error) });
  }
});
server.keepAliveTimeout = 5000;
server.listen(port, "127.0.0.1", () => console.log(`oidc stub at ${issuer}`));
