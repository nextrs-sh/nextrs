// Request isolation beyond headers and cookies, against a running server.
// Many overlapping requests with unique identities exercise:
//
//   * React.cache: every section of one render sees the same nonce and its
//     own identity; no nonce is shared between requests;
//   * concurrent Suspense boundaries reading request data;
//   * error boundaries: failing and succeeding renders interleaved (a
//     server-component error is a 500 whose boundary renders on the
//     client from the Flight payload, so the HTML must hold no page data);
//   * after(): each callback observes its own request's headers;
//   * request bodies: concurrent uploads hash back to their own content;
//   * unstable_cache: intended to persist, so a key's creator must be one of
//     that key's requesters, and each process (isolate) creates it at most
//     once.
//
//   node tests/compat/features.mjs --target http://127.0.0.1:3001 \
//        [--requests 600] [--concurrency 64] [--json out.json]
import http from "node:http";
import { createHash, randomBytes } from "node:crypto";
import { writeFileSync } from "node:fs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const target = new URL(args.target ?? "http://127.0.0.1:3001");
const total = Number(args.requests ?? 600);
const concurrency = Number(args.concurrency ?? 64);
const agent = new http.Agent({ keepAlive: true, maxSockets: concurrency });
const run = randomBytes(3).toString("hex");

function send(path, { method = "GET", headers = {}, body } = {}) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: target.hostname, port: target.port, method, path, headers, agent }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString() }));
    });
    req.on("error", reject);
    req.end(body);
  });
}

// Server-rendered text without React's text separators or Flight scripts.
const html = (body) => body.replace(/<script[\s\S]*?<\/script>/g, "").replace(/<!-- -->/g, "");
const ID = /fx-[0-9a-f]{6}-\d+/g;

const failures = [];
const counts = {};
const fail = (kind, detail) => {
  failures.push({ kind, ...detail });
  counts[`${kind}-failed`] = (counts[`${kind}-failed`] ?? 0) + 1;
};
const ok = (kind) => (counts[kind] = (counts[kind] ?? 0) + 1);

const nonces = new Map();
const afterIds = [];
const persistent = new Map(); // key -> { requesters:Set, creators:Set }

async function cacheCase(id) {
  const res = await send("/isolation/cache", { headers: { "x-nextrs-id": id } });
  const text = html(res.body);
  const top = text.match(/top=([^/<]+)\/([a-z0-9]+)/);
  const sections = [...text.matchAll(/section-(\d)=([^/<]+)\/([a-z0-9]+)\/([^<]+)/g)];
  const foreign = (text.match(ID) ?? []).filter((x) => x !== id);
  if (res.status !== 200 || !top || sections.length !== 5) return fail("react-cache", { id, status: res.status, sections: sections.length });
  const nonce = top[2];
  const wrong = sections.filter((m) => m[2] !== id || m[3] !== nonce || m[4] !== id);
  if (top[1] !== id || wrong.length || foreign.length) return fail("react-cache", { id, top: top[0], wrong: wrong.map((m) => m[0]), foreign });
  if (nonces.has(nonce)) return fail("react-cache", { id, sharedNonceWith: nonces.get(nonce) });
  nonces.set(nonce, id);
  ok("react-cache");
}

async function boundaryCase(id, seq) {
  const res = await send("/isolation/boundary", { headers: { "x-nextrs-id": id, "x-nextrs-seq": String(seq) } });
  const text = html(res.body);
  const foreign = (text.match(ID) ?? []).filter((x) => x !== id);
  if (seq % 2 === 1) {
    if (res.status !== 500 || /ok=/.test(text)) return fail("error-boundary", { id, status: res.status });
  } else if (res.status !== 200 || !text.includes(`ok=${id}`)) {
    return fail("error-boundary", { id, status: res.status });
  }
  if (foreign.length) return fail("error-boundary", { id, foreign });
  ok("error-boundary");
}

async function afterCase(id) {
  const res = await send(`/api/after?id=${id}`, { headers: { "x-nextrs-id": id } });
  if (res.status !== 200 || JSON.parse(res.body).id !== id) return fail("after", { id, status: res.status });
  afterIds.push(id);
}

async function bodyCase(id, i) {
  const body = Buffer.concat([Buffer.from(id), randomBytes(20_000 + (i % 50) * 1000)]);
  const expected = createHash("sha256").update(body).digest("hex");
  const chunked = i % 2 === 0;
  const res = await send("/api/echo", {
    method: "POST",
    headers: chunked ? { "transfer-encoding": "chunked" } : { "content-length": body.length },
    body,
  });
  const result = res.status === 200 ? JSON.parse(res.body) : {};
  if (result.sha256 !== expected || result.bytes !== body.length) return fail("request-body", { id, status: res.status, result });
  ok("request-body");
}

async function persistentCase(id, i) {
  const key = `${run}-k${i % 5}`;
  const res = await send("/isolation/persistent", { headers: { "x-nextrs-id": id, "x-nextrs-key": key } });
  const m = html(res.body).match(/key=(\S+) createdBy=(\S+) requestedBy=([^<\s]+)/);
  if (res.status !== 200 || !m || m[1] !== key || m[3] !== id) return fail("unstable-cache", { id, status: res.status, entry: m?.[0] });
  const entry = persistent.get(key) ?? { requesters: new Set(), creators: new Set() };
  entry.requesters.add(id);
  entry.creators.add(m[2]);
  persistent.set(key, entry);
  ok("unstable-cache");
}

const started = performance.now();
let next = 0;
async function worker() {
  while (next < total) {
    const i = next++;
    const id = `fx-${run}-${i}`;
    switch (i % 5) {
      case 0: await cacheCase(id); break;
      case 1: await boundaryCase(id, i); break;
      case 2: await afterCase(id); break;
      case 3: await bodyCase(id, i); break;
      default: await persistentCase(id, i);
    }
  }
}
await Promise.all(Array.from({ length: concurrency }, worker));

// after() runs once the response is sent; collect what each observed. With
// several isolates each report only knows its own isolate's callbacks, so
// ask repeatedly and merge.
await new Promise((resolve) => setTimeout(resolve, 300));
const observed = new Map();
for (let attempt = 0; attempt < 40 && observed.size < afterIds.length; attempt++) {
  const pending = afterIds.filter((id) => !observed.has(id));
  for (let i = 0; i < pending.length; i += 50) {
    const res = await send(`/api/after?report=${pending.slice(i, i + 50).join(",")}`);
    for (const [id, seen] of Object.entries(JSON.parse(res.body))) if (seen !== null) observed.set(id, seen);
  }
}
for (const id of afterIds) {
  if (!observed.has(id)) fail("after", { id, reason: "callback result not found" });
  else if (observed.get(id) !== id) fail("after", { id, observed: observed.get(id) });
  else ok("after");
}

const creatorsPerKey = {};
for (const [key, { requesters, creators }] of persistent) {
  creatorsPerKey[key] = creators.size;
  for (const creator of creators) if (!requesters.has(creator)) fail("unstable-cache", { key, creator, reason: "creator never requested this key" });
}

const summary = {
  target: target.href,
  requests: total,
  concurrency,
  elapsedMs: Math.round(performance.now() - started),
  counts,
  uniqueNonces: nonces.size,
  unstableCacheCreatorsPerKey: creatorsPerKey,
  failures: failures.slice(0, 20),
};
console.log(JSON.stringify(summary, null, 2));
if (args.json) writeFileSync(args.json, JSON.stringify(summary, null, 2));
process.exitCode = failures.length ? 1 : 0;
