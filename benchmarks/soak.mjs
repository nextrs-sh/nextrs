// Sustained mixed traffic against a running server, with optional executor
// stall injection, sampling server RSS and executor metrics.
//
//   node benchmarks/soak.mjs --target http://127.0.0.1:3001 --pid <server pid> \
//        [--metrics http://127.0.0.1:9101] [--minutes 10] [--concurrency 48] \
//        [--inject-every 60 --inject-ms 3000] [--sample-every 10] [--json out.json]
//        [--upstream http://127.0.0.1:4999]
//
// Traffic, per worker, in rotation:
//   * /request-info and /books/<id> with unique tokens; any foreign token in
//     a body is a request-state leak (as in tests/compat/isolation.mjs);
//   * /dynamic (streaming Suspense), some aborted after the first chunk;
//   * /api/stream, 4 MiB, read slowly; some aborted mid-body;
//   * POST /api/echo with a 256 KiB body, hash verified;
//   * /isolation/cache: React.cache gives each render its own nonce and
//     identity (as in tests/compat/features.mjs);
//   * /api/after: after() callbacks observe their own request's headers,
//     collected in batches with ?report=;
//   * /api/health.
// With --upstream (tests/compat/upstream.mjs; the server must be allowed to
// fetch from it), the rotation adds outbound fetch and data cache traffic:
//   * /data-fetch: an uncached upstream fetch; the identity rendered after
//     it must be the request's own;
//   * /api/data with force-cache on a warm key (hits) and on keys from a
//     bounded space (misses and stores, evictions under small limits);
//   * a tag revalidated every minute, and clients that leave mid-fetch.
// With --project (the fixture directory, for Server Action ids) and
// --upstream, Server Action and ISR traffic is added:
//   * a client-called Server Function that reports the caller's header
//     before and after a delay (identity, as above);
//   * a multipart action with a 256 KiB file, its SHA-256 verified;
//   * /isr/timed (regenerated every 2 s under load), /isr/<n> over a bounded
//     space (on-demand ISR entries stored, replaced, evicted), and every
//     ~200th request an action that calls revalidatePath on /isr/tagged.
// --app next-proxy (fixtures/next-proxy; --project for its Server Action
// ids) replaces the rotation with proxy traffic: matched requests whose
// rewrite param, query, request cookie and proxy-set cookie must all be the
// request's own; excluded paths; proxy redirects, rewrites and direct
// responses; protected pages without and with a session (prerendered and
// ISR); ISR behind the proxy; a Server Action through the proxy; request
// bodies read by the proxy; proxies aborted by their client; proxy
// exceptions. Stalls are injected in the proxy (/proxy/spin).
// --app platform (fixtures/next-platform-proxy, --upstream required) sends
// a proxy that fetches before deciding (identities checked across awaits),
// timeouts, waitUntil work, external rewrites (echoed with the request's
// token, 1 MiB bodies, streamed bodies read slowly or abandoned, POST
// bodies, a refused destination, an abandoned slow upstream), ISR
// fallback pages, Draft Mode cookies and dynamicParams 404s. Stalls are
// injected in the proxy (/gate/spin).
// --app jwks (fixtures/next-jwks-proxy; --oidc for tests/compat/oidc-stub.mjs,
// --upstream, --project, APP_KEYS in the environment) sends the Phase 2C
// authentication mix: bearer tokens of five algorithms verified against
// the provider's JWKS (each answer must name the token's own subject; the
// fixture refetches the key set every 1.5 s per isolate), invalid tokens
// (expired, wrong audience, tampered: 401 with the right code), a key
// rotation every 5 minutes, ES256 session pages, protected ISR, a protected
// Server Action and after() work (each recorded once, by its own subject,
// in the upstream's ledger: checked later), ES256/PS256/RS256/EdDSA
// tokens NextRS signs (verified with node:crypto), streamed bodies naming
// their subject (some abandoned), external rewrites behind the proxy and
// clients that leave at once. Stalls are injected in the proxy (/proxy/spin).
// With --inject-every, GET /api/spin?ms=<inject-ms> is sent on that period;
// run the server with --js-stall-timeout below inject-ms so the stall
// terminates and replaces an isolate.
//
// Requests that fail because their isolate was terminated (502, aborted
// body) are counted as "lost to restart", not as errors; only leaks,
// corrupted bodies and unexpected statuses fail the run.
import http from "node:http";
import { createHash, createPrivateKey, createPublicKey, randomBytes, randomUUID, sign as nodeSign, verify as nodeVerify } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const target = new URL(args.target ?? "http://127.0.0.1:3001");
const pid = args.pid ? Number(args.pid) : null;
const metricsUrl = args.metrics ? new URL("/metrics.json", args.metrics) : null;
const minutes = Number(args.minutes ?? 10);
const concurrency = Number(args.concurrency ?? 48);
const injectEvery = args["inject-every"] && Number(args["inject-every"]) > 0 ? Number(args["inject-every"]) : null;
const injectMs = Number(args["inject-ms"] ?? 3000);
const sampleEvery = Number(args["sample-every"] ?? 10);
const agent = new http.Agent({ keepAlive: true, maxSockets: concurrency + 8 });
const upstream = args.upstream ?? null;
const project = args.project ?? null;
const actionIds = (() => {
  if (!project || (args.app ?? "next-app") !== "next-app") return null;
  const manifest = JSON.parse(readFileSync(`${project}/.next/server/server-reference-manifest.json`, "utf8"));
  const find = (name, file) => Object.entries(manifest.node).find(([, v]) => v.exportedName === name && v.filename === file)?.[0];
  return { who: find("whoAmI", "app/actions/server.ts"), echo: find("echoForm", "app/actions/server.ts"), isr: find("isrInvalidate", "app/isr/actions.ts") };
})();

const counts = {};
const bump = (key) => (counts[key] = (counts[key] ?? 0) + 1);
const failures = [];
const fail = (kind, detail) => {
  bump(`${kind}-FAILED`);
  if (failures.length < 50) failures.push({ kind, t: Math.round((Date.now() - started) / 1000), ...detail });
};

function request(path, { method = "GET", headers = {}, body, abortAfter, slow, timeoutMs } = {}) {
  return new Promise((resolve) => {
    const req = http.request(
      { host: target.hostname, port: target.port, method, path, headers, agent },
      (res) => {
        const chunks = [];
        let received = 0;
        res.on("data", (chunk) => {
          chunks.push(chunk);
          received += chunk.length;
          if (abortAfter != null && received >= abortAfter) {
            req.destroy();
            resolve({ status: res.statusCode, aborted: true });
            return;
          }
          if (slow) {
            res.pause();
            setTimeout(() => res.resume(), slow);
          }
        });
        res.on("end", () =>
          resolve({ status: res.statusCode, nextrsError: res.headers["x-nextrs-error"], cache: res.headers["x-nextjs-cache"], body: Buffer.concat(chunks) }),
        );
        res.on("error", () => resolve({ status: res.statusCode, truncated: true }));
        res.on("aborted", () => resolve({ status: res.statusCode, truncated: true }));
      },
    );
    req.on("error", (error) => resolve({ error: error.code ?? String(error) }));
    if (timeoutMs) {
      setTimeout(() => {
        req.destroy();
        resolve({ aborted: true });
      }, timeoutMs);
    }
    req.end(body);
  });
}

// A response cut short by an isolate restart: 502 before the head, a body
// that ended without its terminating chunk, or the retry-safe 503 for a
// request that was still waiting in the failed isolate's inbox (never
// delivered to JavaScript).
function lostToRestart(r) {
  // A refused external rewrite is the intended answer, not a casualty.
  if (r.status === 502 && r.nextrsError === "external-rewrite-denied") return false;
  if (r.status === 503 && r.nextrsError === "executor-unavailable") {
    bump("undelivered-503");
    return true;
  }
  return r.status === 502 || r.truncated || r.error === "ECONNRESET" || r.error === "UND_ERR_SOCKET";
}

const TOKEN = /nx[pctb]-\d+-[0-9a-f]{8}/g;
let seq = 0;

async function identityCase() {
  const i = seq++;
  const salt = randomBytes(4).toString("hex");
  let path, headers, expected;
  if (i % 2) {
    const probe = `nxp-${i}-${salt}`;
    const theme = `nxc-${i}-${salt}`;
    path = "/request-info";
    headers = { "x-nextrs-probe": probe, cookie: `theme=${theme}` };
    expected = [probe, theme];
  } else {
    const book = `nxb-${i}-${salt}`;
    path = `/books/${book}`;
    headers = {};
    expected = [book];
  }
  const r = await request(path, { headers });
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("identity-status", { path, status: r.status, error: r.error });
  const found = new Set(r.body.toString().match(TOKEN) ?? []);
  const foreign = [...found].filter((t) => !expected.includes(t));
  const missing = expected.filter((t) => !found.has(t));
  if (foreign.length) return fail("LEAK", { path, foreign });
  if (missing.length) return fail("identity-missing", { path, missing });
  bump("identity-ok");
}

async function dynamicCase(abort) {
  const r = await request("/dynamic", abort ? { abortAfter: 1 } : {});
  if (abort) return bump("dynamic-aborted");
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200 || !r.body.includes("Streamed after Suspense boundary")) {
    return fail("dynamic", { status: r.status });
  }
  bump("dynamic-ok");
}

function checkStream(body, size) {
  if (body.length !== size) return false;
  for (let at = 0; at < body.length; at += 4093) {
    const index = Math.floor(at / 16384);
    const pos = at % 16384;
    const expected = pos < 4 ? (index >>> (8 * (3 - pos))) & 0xff : index % 251;
    if (body[at] !== expected) return false;
  }
  return true;
}

async function streamCase(abort) {
  const size = 4 * 1024 * 1024;
  const r = await request(`/api/stream?size=${size}`, abort ? { abortAfter: 512 * 1024 } : { slow: 2 });
  if (abort) return bump("stream-aborted");
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200 || !checkStream(r.body, size)) return fail("stream", { status: r.status, length: r.body?.length });
  bump("stream-ok");
}

async function uploadCase() {
  const body = randomBytes(256 * 1024);
  const r = await request("/api/echo", { method: "POST", body, headers: { "content-type": "application/octet-stream" } });
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("upload-status", { status: r.status });
  const json = JSON.parse(r.body.toString());
  if (json.sha256 !== createHash("sha256").update(body).digest("hex")) return fail("upload-hash", {});
  bump("upload-ok");
}

const nonces = new Set();
async function reactCacheCase() {
  const id = `nxr-${seq++}-${randomBytes(4).toString("hex")}`;
  const r = await request("/isolation/cache", { headers: { "x-nextrs-id": id } });
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("react-cache-status", { status: r.status });
  const html = r.body.toString().replace(/<!-- -->/g, "");
  const top = /top=([^/<]+)\/([a-z0-9]+)/.exec(html);
  const sections = [...html.matchAll(/section-\d=([^/<]+)\/([a-z0-9]+)\/([^<]+)/g)];
  if (!top || sections.length !== 5) return fail("react-cache", { id, sections: sections.length });
  const wrong = sections.filter((m) => m[1] !== id || m[2] !== top[2] || m[3] !== id);
  if (top[1] !== id || wrong.length) return fail("LEAK", { path: "/isolation/cache", id, top: top[0] });
  if (nonces.has(top[2])) return fail("react-cache-shared-nonce", { id });
  // Bounded: only recent nonces matter for overlap between requests.
  if (nonces.size > 10_000) nonces.clear();
  nonces.add(top[2]);
  bump("react-cache-ok");
}

let afterPending = [];
async function afterCase() {
  const id = `nxa-${seq++}-${randomBytes(4).toString("hex")}`;
  const r = await request(`/api/after?id=${id}`, { headers: { "x-nextrs-id": id } });
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200 || JSON.parse(r.body.toString()).id !== id) return fail("after-status", { status: r.status });
  afterPending.push({ id, at: Date.now(), tries: 0 });
  if (afterPending.length < 25) return bump("after-sent");
  // Report on callbacks that have had a second to run. The observations are
  // module state of the isolate that ran the callback, so with several
  // isolates a report may land elsewhere: retry those a few times. An
  // isolate restart also loses them; only a wrong identity is a failure.
  const due = afterPending.filter((p) => Date.now() - p.at > 1000);
  afterPending = afterPending.filter((p) => Date.now() - p.at <= 1000);
  if (!due.length) return bump("after-sent");
  const report = await request(`/api/after?report=${due.map((p) => p.id).join(",")}`);
  if (report.status !== 200) return bump("after-report-lost");
  const seen = JSON.parse(report.body.toString());
  for (const p of due) {
    if (seen[p.id] === null || seen[p.id] === undefined) {
      if (p.tries < 8) afterPending.push({ ...p, tries: p.tries + 1 });
      else bump("after-unreported");
    } else if (seen[p.id] !== p.id) fail("LEAK", { path: "/api/after", id: p.id, observed: seen[p.id] });
    else bump("after-ok");
  }
}

async function dataFetchCase() {
  const id = `nxd-${seq++}-${randomBytes(4).toString("hex")}`;
  const r = await request("/data-fetch", { headers: { "x-nextrs-id": id } });
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("data-fetch-status", { status: r.status });
  const text = r.body.toString().replace(/<!-- -->/g, "");
  const m = /data">id=(\S+) key=(\S+) hit=\d+ after=([^<\s]+)/.exec(text);
  if (!m) return fail("data-fetch", { id });
  if (m[1] !== id || m[2] !== id || m[3] !== id) return fail("LEAK", { path: "/data-fetch", id, seen: m[0] });
  bump("data-fetch-ok");
}

let coldKey = 0;
async function dataCacheCase(kind) {
  let path;
  if (kind === "warm") path = "/api/data?key=soak-warm&cache=force-cache";
  else if (kind === "tagged") path = "/api/data?key=soak-tagged&revalidate=3600&tag=soak-tag";
  // A bounded key space: entries are reused, replaced and evicted.
  else path = `/api/data?key=soak-cold-${coldKey++ % 20000}&cache=force-cache`;
  const r = await request(path);
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail(`data-${kind}-status`, { status: r.status, body: r.body?.toString().slice(0, 200) });
  const body = JSON.parse(r.body.toString());
  if (body.status !== 200 || typeof body.body?.hit !== "number") return fail(`data-${kind}`, { body });
  bump(`data-${kind}-ok`);
}

async function dataAbortCase() {
  const r = await request(`/api/data?key=soak-abort-${seq++}&delay=400`, { abortAfter: 0, timeoutMs: 100 });
  bump(r.aborted || r.error ? "data-aborted" : "data-abort-completed");
}

async function healthCase() {
  const r = await request("/api/health");
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("health", { status: r.status, error: r.error ?? (r.aborted ? "aborted" : undefined) });
  bump("health-ok");
}

function multipart(boundary, parts) {
  const chunks = [];
  for (const [name, value, filename] of parts) {
    const disposition = filename ? `form-data; name="${name}"; filename="${filename}"` : `form-data; name="${name}"`;
    const type = filename ? "Content-Type: application/octet-stream\r\n" : "";
    chunks.push(Buffer.from(`--${boundary}\r\nContent-Disposition: ${disposition}\r\n${type}\r\n`), Buffer.from(value), Buffer.from("\r\n"));
  }
  chunks.push(Buffer.from(`--${boundary}--\r\n`));
  return Buffer.concat(chunks);
}

async function actionWhoCase() {
  const id = `nxw-${seq++}-${randomBytes(4).toString("hex")}`;
  const r = await request("/actions/client", {
    method: "POST",
    headers: { "next-action": actionIds.who, accept: "text/x-component", "content-type": "text/plain;charset=UTF-8", origin: target.origin, "x-test-user": id },
    body: JSON.stringify([id, 20]),
  });
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("action-status", { status: r.status });
  const text = r.body.toString();
  if (!text.includes(`"user":"${id}"`) || !text.includes(`"userAfterDelay":"${id}"`)) return fail("LEAK", { path: "action whoAmI", id, body: text.slice(0, 300) });
  bump("action-ok");
}

const uploadBytes = randomBytes(256 * 1024);
const uploadHash = createHash("sha256").update(uploadBytes).digest("hex");
async function actionUploadCase() {
  const boundary = `nxsoak${randomBytes(6).toString("hex")}`;
  const body = multipart(boundary, [
    ["_1_$ACTION_REF_1", ""],
    ["_1_$ACTION_1:0", JSON.stringify({ id: actionIds.echo, bound: "$@1" })],
    ["_1_$ACTION_1:1", "[null]"],
    ["_1_title", "soak"],
    ["_1_file", uploadBytes, "soak.bin"],
    ["0", '[null,"$K1"]'],
  ]);
  const r = await request("/actions/form", {
    method: "POST",
    headers: { "next-action": actionIds.echo, accept: "text/x-component", "content-type": `multipart/form-data; boundary=${boundary}`, origin: target.origin },
    body,
  });
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200) return fail("action-upload-status", { status: r.status });
  if (!r.body.toString().includes(uploadHash)) return fail("action-upload-hash", { body: r.body.toString().slice(0, 300) });
  bump("action-upload-ok");
}

let isrSlug = 0;
let isrCount = 0;
async function isrCase() {
  const n = isrCount++;
  let path = n % 2 === 0 ? "/isr/timed" : `/isr/soak-${isrSlug++ % 5000}`;
  if (n % 200 === 199) {
    const boundary = `nxisr${randomBytes(6).toString("hex")}`;
    const body = multipart(boundary, [["_1_how", "revalidatePath"], ["_1_id", `soak-${n}`], ["0", '["$K1"]']]);
    const r = await request("/isr/tagged", {
      method: "POST",
      headers: { "next-action": actionIds.isr, accept: "text/x-component", "content-type": `multipart/form-data; boundary=${boundary}`, origin: target.origin },
      body,
    });
    if (lostToRestart(r)) return bump("lost-to-restart");
    if (r.status !== 200) return fail("isr-action-status", { status: r.status });
    bump("isr-revalidated");
    path = "/isr/tagged";
  }
  const r = await request(path);
  if (lostToRestart(r)) return bump("lost-to-restart");
  if (r.status !== 200 || !/stamp-(?:<!-- -->)?\d+/.test(r.body.toString())) return fail("isr", { path, status: r.status });
  bump(`isr-${r.cache ?? "ok"}`);
}

// --- --app next-proxy -----------------------------------------------------
const appName = args.app ?? "next-app";
const proxyIds = (() => {
  if (appName !== "next-proxy" || !project) return null;
  const manifest = JSON.parse(readFileSync(`${project}/.next/server/server-reference-manifest.json`, "utf8"));
  const find = (name) => Object.entries(manifest.node).find(([, v]) => v.exportedName === name && v.filename === "app/actions/actions.ts")?.[0];
  return { inspect: find("inspect") };
})();
const token = () => `nxp-${seq++}-${randomBytes(4).toString("hex")}`;
const expectStatus = (kind, r, ...statuses) => {
  if (lostToRestart(r)) return bump("lost-to-restart"), false;
  if (!statuses.includes(r.status)) return fail(kind, { status: r.status ?? r.error, error: r.nextrsError, body: r.body?.toString().slice(0, 160) }), false;
  bump(`${kind}-ok`);
  return true;
};
const decode = (s) => s.replace(/&quot;/g, '"').replace(/&amp;/g, "&").replace(/&#x27;/g, "'");

// A proxy rewrite (the token as the rewritten param and in the query the
// proxy appends) or NextResponse.next() with a proxy-set cookie (read back
// by the route, and sent to the client): only the request's own token may
// appear.
async function proxyIdentityCase() {
  const t = token();
  const rewrite = seq % 2 === 0;
  const path = rewrite ? `/proxy/rewrite-dynamic/${t}?setcookie=${t}` : `/dynamic?setcookie=${t}`;
  const r = await request(path, { headers: { cookie: `theme=${t}` } });
  if (lostToRestart(r)) return bump("lost-to-restart");
  const html = r.body?.toString() ?? "";
  let info = {};
  try {
    info = JSON.parse(decode(/id="info">([^<]*)</.exec(html)?.[1] ?? "{}"));
  } catch {}
  const foreign = (html.match(TOKEN) ?? []).filter((x) => x !== t);
  const own = rewrite ? /id="slug">([^<]*)</.exec(html)?.[1] === t : info.proxyCustom === t;
  if (r.status !== 200 || !own || foreign.length) {
    fail("proxy-identity", { status: r.status, rewrite, proxyCustom: info.proxyCustom, foreign: foreign.slice(0, 2) });
  } else bump("proxy-identity-ok");
}
async function proxyJsonCase() {
  const t = token();
  const r = await request(`/proxy/json?k=${t}`);
  if (!expectStatus("proxy-json", r, 201)) return;
  const body = JSON.parse(r.body.toString());
  if (body.query?.k !== t) fail("proxy-json-leak", { got: body.query?.k });
}
async function proxyBodyCase() {
  const t = token();
  const text = `${t}:${"b".repeat(4096)}`;
  const r = await request("/proxy/body", { method: "POST", headers: { "content-type": "text/plain", "content-length": String(text.length) }, body: text });
  if (!expectStatus("proxy-body", r, 200)) return;
  const body = JSON.parse(r.body.toString());
  if (body.text !== text) fail("proxy-body-corrupt", { length: body.length });
}
async function proxyActionCase() {
  if (!proxyIds?.inspect) return;
  const t = token();
  const boundary = `----soak${randomBytes(6).toString("hex")}`;
  const body = multipart(boundary, [
    ["_1_value", t],
    ["0", '["$K1"]'],
  ]);
  const r = await request("/actions", {
    method: "POST",
    headers: { "next-action": proxyIds.inspect, origin: target.origin, accept: "text/x-component", "content-type": `multipart/form-data; boundary=${boundary}`, "content-length": String(body.length) },
    body,
  });
  if (!expectStatus("proxy-action", r, 200)) return;
  const text = r.body.toString();
  // The action's return value in the RSC payload: it ran, behind the
  // proxy (the header the proxy set on the way).
  if (!/\{"proxyRequest":"set-by-proxy","proxyPathname":"\/actions"/.test(text)) fail("proxy-action-wrong", { sample: text.slice(0, 300) });
}
async function proxyAbortCase() {
  await request("/proxy/slow?ms=300", { timeoutMs: 60 });
  bump("proxy-aborted");
}
const proxyRotation = [
  proxyIdentityCase, proxyIdentityCase, proxyIdentityCase,
  async () => expectStatus("proxy-excluded", await request("/excluded"), 200),
  async () => expectStatus("proxy-not-matched", await request("/api/health"), 200),
  async () => expectStatus("proxy-redirect", await request("/proxy/redirect"), 307),
  async () => expectStatus("proxy-rewrite", await request("/proxy/rewrite"), 200),
  async () => expectStatus("proxy-protected-anon", await request("/protected"), 307),
  async () => expectStatus("proxy-protected-session", await request("/protected", { headers: { cookie: "session=synthetic-valid-session" } }), 200),
  async () => expectStatus("proxy-protected-isr", await request("/protected/isr", { headers: { cookie: "session=synthetic-valid-session" } }), 200),
  async () => expectStatus("proxy-isr", await request("/isr"), 200),
  async () => expectStatus("proxy-dynamic", await request("/dynamic?a=1"), 200),
  async () => expectStatus("proxy-exception", await request("/proxy/throw"), 500),
  proxyJsonCase, proxyBodyCase, proxyActionCase, proxyAbortCase, proxyIdentityCase,
];

// --- --app platform ---------------------------------------------------------
async function platformIdentityCase() {
  const t = token();
  const r = await request(`/gate/identity?id=${t}&ms=${seq % 7}`);
  if (lostToRestart(r)) return bump("lost-to-restart");
  const html = r.body?.toString() ?? "";
  let info = {};
  try {
    info = JSON.parse(decode(/id="info">([^<]*)</.exec(html)?.[1] ?? "{}"));
  } catch {}
  if (r.status !== 200 || info.identity !== t || info.seen !== t || info.identityCookie !== t) {
    fail("platform-identity", { status: r.status, info });
  } else bump("platform-identity-ok");
}
async function externalEchoCase() {
  const t = token();
  const r = await request(`/ext/${t}?t=${t}`);
  if (!expectStatus("ext-echo", r, 200)) return;
  const echo = JSON.parse(r.body.toString());
  if (echo.url !== `/echo/${t}?t=${t}`) fail("ext-echo-leak", { url: echo.url });
}
async function externalPostCase() {
  const t = token();
  const text = `${t}|${"p".repeat(32 * 1024)}`;
  const r = await request(`/ext/post-${t}`, { method: "POST", headers: { "content-type": "text/plain", "content-length": String(text.length) }, body: text });
  if (!expectStatus("ext-post", r, 200)) return;
  if (JSON.parse(r.body.toString()).body !== text) fail("ext-post-corrupt", {});
}
async function externalLargeCase() {
  const r = await request("/ext-large/1048576");
  if (!expectStatus("ext-large", r, 200)) return;
  if (r.body.length !== 1048576) fail("ext-large-truncated", { length: r.body.length });
}
async function externalStreamCase(abort) {
  const r = await request("/ext-stream", abort ? { abortAfter: 7 } : { slow: 20 });
  if (abort) return bump("ext-stream-aborted");
  if (!expectStatus("ext-stream", r, 200)) return;
  if (!r.body.toString().endsWith("line 4\n")) fail("ext-stream-corrupt", { tail: r.body.toString().slice(-20) });
}
async function proxyExternalCase() {
  const t = token();
  const r = await request(`/gate/external?client=${t}`);
  if (!expectStatus("proxy-ext", r, 200)) return;
  const echo = JSON.parse(r.body.toString());
  if (!echo.url.includes(`client=${t}`)) fail("proxy-ext-leak", { url: echo.url });
}
const platformRotation = [
  platformIdentityCase, platformIdentityCase, platformIdentityCase,
  async () => expectStatus("gate-open", await request("/gate/open"), 200),
  async () => expectStatus("gate-closed", await request("/gate/closed"), 307),
  async () => expectStatus("gate-direct", await request("/gate/direct"), 202),
  async () => expectStatus("gate-status-404", await request("/gate/status/404"), 200),
  async () => expectStatus("gate-timeout", await request("/gate/timeout"), 504),
  async () => expectStatus("gate-wait-until-fail", await request("/gate/wait-until-fail"), 200),
  async () => expectStatus("no-matcher-home", await request("/"), 200),
  externalEchoCase, externalPostCase, externalLargeCase,
  () => externalStreamCase(false), () => externalStreamCase(true),
  proxyExternalCase, platformIdentityCase,
  async () => expectStatus("ext-denied", await request("/ext-unlisted/x"), 502),
  async () => {
    await request("/ext-slow", { timeoutMs: 80 });
    bump("ext-slow-abandoned");
  },
  async () => expectStatus("isr-fallback", await request(`/isr-posts/s${seq % 64}`), 200),
  async () => expectStatus("posts-unlisted", await request("/posts/zz"), 404),
  async () => expectStatus("draft-forged", await request("/posts/a", { headers: { cookie: "__prerender_bypass=forged" } }), 200),
  async () => expectStatus("instrumentation", await request("/api/instrumentation"), 200),
];

// --- --app jwks ---------------------------------------------------------------
const jwks = await (async () => {
  if (appName !== "jwks") return null;
  const oidc = args.oidc;
  const keys = JSON.parse(process.env.APP_KEYS ?? "null");
  if (!oidc || !keys || !upstream || !project) throw new Error("--app jwks needs --oidc, --upstream, --project and APP_KEYS");
  const manifest = JSON.parse(readFileSync(`${project}/.next/server/server-reference-manifest.json`, "utf8"));
  const record = Object.entries(manifest.node).find(([, v]) => v.exportedName === "recordProtected")?.[0];
  const now = () => Math.floor(Date.now() / 1000);
  const mint = async (spec) => (await (await fetch(`${oidc}/mint`, { method: "POST", body: JSON.stringify(spec) })).json()).token;
  const life = Math.ceil(minutes * 60) + 3600;
  const pool = [];
  for (let u = 0; u < 12; u++) {
    for (const kid of ["rs-1", "ps-1", "es-1", "es384-1", "ed-1"]) {
      const sub = `u-soak-${u}`;
      pool.push({ sub, kid, token: await mint({ kid, claims: { sub, aud: "nextrs-api", exp: now() + life } }) });
    }
  }
  const invalid = [
    { code: "ERR_JWT_EXPIRED", token: await mint({ kid: "es-1", claims: { sub: "u-x", aud: "nextrs-api", exp: now() - 600 } }) },
    { code: "ERR_JWT_CLAIM_VALIDATION_FAILED", token: await mint({ kid: "rs-1", claims: { sub: "u-x", aud: "other", exp: now() + life } }) },
  ];
  const good = await mint({ kid: "ed-1", claims: { sub: "u-x", aud: "nextrs-api", exp: now() + life } });
  invalid.push({ code: "ERR_JWS_SIGNATURE_VERIFICATION_FAILED", token: good.slice(0, -4) + (good.endsWith("AAAA") ? "BBBB" : "AAAA") });
  const b64u = (v) => Buffer.from(v).toString("base64url");
  const privateKey = createPrivateKey(keys.ES256.private);
  const session = (userId) => {
    const input = `${b64u(JSON.stringify({ alg: "ES256" }))}.${b64u(JSON.stringify({ userId, name: userId, iss: "nextrs-jwks-app", aud: "nextrs-jwks-app", iat: now(), exp: now() + life }))}`;
    return `session=${input}.${b64u(nodeSign("sha256", Buffer.from(input), { key: privateKey, dsaEncoding: "ieee-p1363" }))}`;
  };
  const sessions = Array.from({ length: 12 }, (_, u) => ({ userId: `u-soak-${u}`, cookie: session(`u-soak-${u}`) }));
  const VERIFY = {
    ES256: (key, data, sig) => nodeVerify("sha256", data, { key, dsaEncoding: "ieee-p1363" }, sig),
    PS256: (key, data, sig) => nodeVerify("sha256", data, { key, padding: 6, saltLength: 32 }, sig),
    RS256: (key, data, sig) => nodeVerify("sha256", data, key, sig),
    EdDSA: (key, data, sig) => nodeVerify(null, data, key, sig),
  };
  const publicKeys = Object.fromEntries(Object.keys(VERIFY).map((alg) => [alg, createPublicKey(keys[alg].public)]));
  return { oidc, record, pool, invalid, sessions, VERIFY, publicKeys, mint, now, life, pending: [] };
})();
const pick = (list) => list[Math.floor(Math.random() * list.length)];
const bearer = (token) => ({ authorization: `Bearer ${token}` });
async function jwksIdentityCase() {
  const t = pick(jwks.pool);
  const r = await request("/api/me", { headers: bearer(t.token) });
  if (!expectStatus("jwks-verify", r, 200)) return;
  const body = JSON.parse(r.body.toString());
  if (body.sub !== t.sub || body.proxySub !== t.sub) fail("LEAK", { case: "jwks-verify", expected: t.sub, seen: `${body.sub}/${body.proxySub}` });
}
async function jwksInvalidCase() {
  const t = pick(jwks.invalid);
  const r = await request("/api/me", { headers: bearer(t.token) });
  if (!expectStatus("jwks-invalid", r, 401)) return;
  const body = JSON.parse(r.body.toString());
  if (body.code !== t.code || body.sub !== undefined) fail("jwks-invalid-wrong", { expected: t.code, body });
}
async function jwksStreamCase() {
  const t = pick(jwks.pool);
  const abandon = seq++ % 4 === 0;
  const r = await request("/api/stream?chunks=8&gap=3", { headers: bearer(t.token), abortAfter: abandon ? 1 : undefined });
  if (abandon) return bump("jwks-stream-abandoned");
  if (!expectStatus("jwks-stream", r, 200)) return;
  const lines = r.body.toString().trim().split("\n");
  if (lines.length !== 8 || lines.some((line, i) => line !== `${t.sub} ${i}`)) fail(lines.some((l) => !l.startsWith(`${t.sub} `)) ? "LEAK" : "jwks-stream-wrong", { expected: t.sub, sample: lines.slice(0, 3) });
}
async function jwksAfterCase() {
  const t = pick(jwks.pool);
  const key = `soak-after-${randomUUID()}`;
  const r = await request(`/api/after?key=${key}`, { headers: bearer(t.token) });
  if (!expectStatus("jwks-after", r, 200)) return;
  jwks.pending.push({ key, expected: t.sub, at: Date.now(), kind: "after" });
}
async function jwksActionCase() {
  const s = pick(jwks.sessions);
  const key = `soak-action-${randomUUID()}`;
  const boundary = `----soak${randomBytes(6).toString("hex")}`;
  const body = multipart(boundary, [["_1_key", key], ["_1_id", "1"], ["0", '["$K1"]']]);
  const r = await request("/actions/protected", {
    method: "POST",
    headers: { cookie: s.cookie, "next-action": jwks.record, origin: target.origin, accept: "text/x-component", "content-type": `multipart/form-data; boundary=${boundary}`, "content-length": String(body.length) },
    body,
  });
  if (lostToRestart(r)) {
    jwks.pending.push({ key, expected: `${s.userId}:1`, at: Date.now(), kind: "action-lost" });
    return bump("lost-to-restart");
  }
  if (!expectStatus("jwks-action", r, 200)) return;
  jwks.pending.push({ key, expected: `${s.userId}:1`, at: Date.now(), kind: "action" });
}
async function jwksSessionCase() {
  const s = pick(jwks.sessions);
  const r = await request("/dashboard", { headers: { cookie: s.cookie } });
  if (!expectStatus("jwks-session-page", r, 200)) return;
  const info = JSON.parse(decode(/id="info">([^<]*)</.exec(r.body.toString())?.[1] ?? "{}"));
  if (info.userId !== s.userId || info.proxyUser !== s.userId) fail("LEAK", { case: "session-page", expected: s.userId, seen: info });
}
const APP_ALGS = ["ES256", "PS256", "RS256", "EdDSA"];
async function jwksAppTokenCase() {
  const s = pick(jwks.sessions);
  const alg = APP_ALGS[seq++ % APP_ALGS.length];
  const r = await request(`/api/app-token?alg=${alg}`, { headers: { cookie: s.cookie } });
  if (!expectStatus("jwks-app-token", r, 200)) return;
  const [h, p, sig] = JSON.parse(r.body.toString()).token.split(".");
  const claims = JSON.parse(Buffer.from(p, "base64url"));
  if (!jwks.VERIFY[alg](jwks.publicKeys[alg], Buffer.from(`${h}.${p}`), Buffer.from(sig, "base64url"))) fail("jwks-app-token-bad-signature", { alg });
  else if (claims.sub !== s.userId) fail("LEAK", { case: "app-token", expected: s.userId, seen: claims.sub });
}
async function jwksExternalCase() {
  const t = token();
  const r = await request(`/ext/${t}`);
  if (!expectStatus("jwks-external", r, 200)) return;
  const url = JSON.parse(r.body.toString()).url;
  if (url !== `/echo/${t}`) fail(new RegExp(TOKEN.source).test(url) ? "LEAK" : "jwks-external-wrong", { expected: t, url });
}
// Ledger entries a little after they were made: one per call, by its own
// subject. A Server Action that answered must have run; after() work and
// an action lost to a restart may be missing, never doubled or foreign.
async function settleLedger() {
  if (!jwks) return;
  const due = jwks.pending.filter((p) => Date.now() - p.at > 3000);
  jwks.pending = jwks.pending.filter((p) => Date.now() - p.at <= 3000);
  for (const p of due) {
    const ids = (await (await fetch(`${upstream}/mutations?key=${p.key}&forget=1`)).json()).ids;
    if (ids.length > 1) fail("DUPLICATE-MUTATION", { key: p.key, ids });
    else if (ids.length === 1 && ids[0] !== p.expected) fail("LEAK", { case: `ledger-${p.kind}`, expected: p.expected, seen: ids[0] });
    else if (ids.length === 0 && p.kind === "action") fail("MUTATION-LOST", { key: p.key });
    else bump(ids.length ? `ledger-${p.kind}-ok` : `ledger-${p.kind}-missing`);
  }
}
let rotations = 0;
async function rotateKeys() {
  const kid = `rot-${++rotations}`;
  await fetch(`${jwks.oidc}/admin/reset`, { method: "POST" });
  await fetch(`${jwks.oidc}/admin/add?kid=${kid}&like=${rotations % 2 ? "es-1" : "rs-1"}`, { method: "POST" });
  await new Promise((r) => setTimeout(r, 250));
  const t = await jwks.mint({ kid, claims: { sub: "u-rotated", aud: "nextrs-api", exp: jwks.now() + 600 } });
  const r = await request("/api/me", { headers: bearer(t) });
  if (expectStatus("jwks-rotated-key", r, 200) && JSON.parse(r.body.toString()).sub !== "u-rotated") fail("LEAK", { case: "rotated" });
}
const jwksRotation = [
  jwksIdentityCase, jwksIdentityCase, jwksIdentityCase, jwksInvalidCase, jwksIdentityCase,
  jwksSessionCase, jwksStreamCase, jwksIdentityCase, jwksAfterCase, jwksActionCase,
  async () => expectStatus("jwks-anon-page", await request("/dashboard"), 307),
  async () => expectStatus("jwks-protected-isr", await request("/reports", { headers: { cookie: pick(jwks.sessions).cookie } }), 200),
  async () => expectStatus("jwks-no-token", await request("/api/me"), 401),
  jwksAppTokenCase, jwksExternalCase, jwksIdentityCase,
  async () => {
    await request("/api/me", { headers: bearer(pick(jwks.pool).token), timeoutMs: 1 });
    bump("jwks-client-left");
  },
];

const rotation = [
  identityCase, identityCase, identityCase, identityCase,
  () => dynamicCase(false), () => dynamicCase(true),
  () => streamCase(false), () => streamCase(true),
  uploadCase, healthCase, identityCase, healthCase,
  reactCacheCase, afterCase,
];
if (appName === "next-proxy") rotation.splice(0, rotation.length, ...proxyRotation);
else if (appName === "jwks") rotation.splice(0, rotation.length, ...jwksRotation);
else if (appName === "platform") rotation.splice(0, rotation.length, ...platformRotation);
if (appName === "next-app" && upstream && actionIds) {
  rotation.push(actionWhoCase, actionUploadCase, isrCase, actionWhoCase, isrCase, isrCase);
}
if (appName === "next-app" && upstream) {
  rotation.push(
    dataFetchCase, () => dataCacheCase("warm"), () => dataCacheCase("cold"),
    () => dataCacheCase("warm"), () => dataCacheCase("cold"), () => dataCacheCase("tagged"),
    dataFetchCase, dataAbortCase,
  );
}

// /proc/<pid>/smaps_rollup: anonymous versus file-backed memory, in MiB.
function smapsRollup() {
  if (!pid) return null;
  try {
    const text = readFileSync(`/proc/${pid}/smaps_rollup`, "utf8");
    const field = (name) => {
      const m = new RegExp(`^${name}:\\s+(\\d+) kB`, "m").exec(text);
      return m ? +(Number(m[1]) / 1024).toFixed(1) : null;
    };
    return { rss: field("Rss"), anonymous: field("Anonymous"), pssAnon: field("Pss_Anon"), pssFile: field("Pss_File"), swap: field("Swap") };
  } catch {
    return null;
  }
}

function procStatus(field) {
  if (!pid) return null;
  const status = readFileSync(`/proc/${pid}/status`, "utf8");
  return +(Number(new RegExp(`${field}:\\s+(\\d+)`).exec(status)[1]) / 1024).toFixed(1);
}

async function metrics() {
  if (!metricsUrl) return null;
  return new Promise((resolve) => {
    http.get(metricsUrl, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve(JSON.parse(Buffer.concat(chunks).toString())));
    }).on("error", () => resolve(null));
  });
}

const started = Date.now();
const deadline = started + minutes * 60_000;
const samples = [];
let completed = 0;

async function sample() {
  const m = await metrics();
  const row = {
    t: Math.round((Date.now() - started) / 1000),
    rssMiB: procStatus("VmRSS"),
    rssAnonMiB: procStatus("RssAnon"),
    // Pages of the server in swap are missing from RSS: a host under memory
    // pressure can hide growth there.
    swapMiB: procStatus("VmSwap"),
    rssFileMiB: procStatus("RssFile"),
    smaps: smapsRollup(),
    // The C heap (glibc mallinfo2) and live Web Crypto keys, from NextRS.
    mallocInUseMiB: m?.memory?.malloc ? +(m.memory.malloc.in_use_bytes / 1048576).toFixed(1) : null,
    mallocFreeMiB: m?.memory?.malloc ? +(m.memory.malloc.free_bytes / 1048576).toFixed(1) : null,
    mallocArenaMiB: m?.memory?.malloc ? +(m.memory.malloc.arena_bytes / 1048576).toFixed(1) : null,
    mallocMmapMiB: m?.memory?.malloc ? +(m.memory.malloc.mmap_bytes / 1048576).toFixed(1) : null,
    cryptoKeys: m?.memory?.crypto_keys ?? null,
    completed,
    restarts: m ? m.executors.reduce((n, e) => n + e.restarts, 0) : null,
    heapUsedMiB: m ? +(m.executors.reduce((n, e) => n + (e.heap_used_bytes ?? 0), 0) / 1048576).toFixed(1) : null,
    heapTotalMiB: m ? +(m.executors.reduce((n, e) => n + (e.heap_total_bytes ?? 0), 0) / 1048576).toFixed(1) : null,
    externalMiB: m ? +(m.executors.reduce((n, e) => n + (e.external_bytes ?? 0), 0) / 1048576).toFixed(1) : null,
    // Live data: heap in use right after each isolate's latest full GC.
    heapAfterGcMiB: m ? +(m.executors.reduce((n, e) => n + (e.heap_used_after_gc_bytes ?? 0), 0) / 1048576).toFixed(1) : null,
    fullGcs: m ? m.executors.reduce((n, e) => n + (e.full_gcs ?? 0), 0) : null,
    errors5xx: m ? m.handler_errors : null,
    ready: m ? m.executors.filter((e) => e.state === "ready").length : null,
    cancelled: m?.responses_cancelled ?? null,
    backpressureWaits: m?.backpressure_waits ?? null,
    rejected: m ? m.rejected_overloaded + m.rejected_queue_timeout + m.rejected_unavailable : null,
    queued: m?.queued ?? null,
    fetchInFlight: m?.fetch?.in_flight ?? null,
    fetchRequests: m?.fetch?.requests ?? null,
    upstreamRequests: m?.upstream?.requests ?? null,
    upstreamInFlight: m?.upstream?.in_flight ?? null,
    activeRequests: m ? m.executors.reduce((n, e) => n + (e.active ?? 0), 0) : null,
    queueWaitSeconds: m?.queue_wait_seconds_total ?? null,
    queueWaits: m?.queue_waits ?? null,
    cacheEntries: m?.cache?.entries ?? null,
    cacheMemoryMiB: m?.cache ? +(m.cache.memory_bytes / 1048576).toFixed(1) : null,
    cacheStorageMiB: m?.cache ? +(m.cache.storage_bytes / 1048576).toFixed(1) : null,
    cacheEvictions: m?.cache?.evictions ?? null,
    // Per slot: isolate generation and live heap, so a long-lived isolate's
    // trend can be told from restarted ones.
    slots: m
      ? m.executors.map((e) => ({
          slot: e.slot,
          generation: e.generation,
          heapAfterGcMiB: +((e.heap_used_after_gc_bytes ?? 0) / 1048576).toFixed(1),
          heapTotalMiB: +((e.heap_total_bytes ?? 0) / 1048576).toFixed(1),
        }))
      : null,
  };
  samples.push(row);
  console.log(JSON.stringify(row));
}

const sampler = setInterval(sample, sampleEvery * 1000);
const injections = [];
const spinPath = { "next-app": "/api/spin", "next-proxy": "/proxy/spin", platform: "/gate/spin", jwks: "/proxy/spin" }[appName];
const settler = jwks ? setInterval(settleLedger, 10_000) : null;
const rotator = jwks ? setInterval(rotateKeys, 300_000) : null;
const revalidator = upstream && appName === "next-app"
  ? setInterval(() => request("/api/revalidate?tag=soak-tag").then((r) => bump(r.status === 200 ? "revalidate-ok" : "revalidate-failed")), 60_000)
  : null;
const injector = injectEvery
  ? setInterval(async () => {
      const at = Math.round((Date.now() - started) / 1000);
      const r = await request(`${spinPath}?ms=${injectMs}`);
      injections.push({ at, status: r.status ?? r.error });
    }, injectEvery * 1000)
  : null;

await sample();
await Promise.all(
  Array.from({ length: concurrency }, async (_, w) => {
    let k = w;
    while (Date.now() < deadline) {
      await rotation[k++ % rotation.length]();
      completed++;
    }
  }),
);
clearInterval(sampler);
if (settler) clearInterval(settler);
if (rotator) clearInterval(rotator);
if (injector) clearInterval(injector);
if (revalidator) clearInterval(revalidator);
await new Promise((r) => setTimeout(r, 3000));
if (jwks) {
  await new Promise((r) => setTimeout(r, 1000));
  for (const p of jwks.pending) p.at = 0;
  await settleLedger();
}
await sample();

const result = {
  target: target.href,
  minutes,
  concurrency,
  injectEvery,
  injectMs,
  completed,
  counts,
  injections,
  failures,
  samples,
};
if (args.json) writeFileSync(args.json, JSON.stringify(result, null, 2));
console.log(JSON.stringify({ completed, counts, injections: injections.length, failures: failures.length }, null, 2));
agent.destroy();
process.exitCode = Object.keys(counts).some((k) => k.endsWith("-FAILED")) ? 1 : 0;
