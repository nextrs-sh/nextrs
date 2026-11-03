// Cross-request isolation test against a running server: many overlapping
// requests, each carrying unique values that Next.js reads through
// AsyncLocalStorage (headers(), cookies()) or route params. Every response
// must contain its own values and no other request's.
//
//   node tests/compat/isolation.mjs --target http://127.0.0.1:3001 \
//        [--requests 600] [--concurrency 64] [--aborts 20] [--json out.json]
//
// Streaming /dynamic renders (timers + Suspense) and client aborts run in
// the same window so request work interleaves at suspension points.
import http from "node:http";
import { randomBytes } from "node:crypto";
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
const aborts = Number(args.aborts ?? 20);
const agent = new http.Agent({ keepAlive: true, maxSockets: concurrency });

function get(path, headers, { abortAfterFirstChunk = false } = {}) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: target.hostname, port: target.port, path, headers, agent }, (res) => {
      const chunks = [];
      res.on("data", (chunk) => {
        chunks.push(chunk);
        if (abortAfterFirstChunk) {
          req.destroy();
          resolve({ status: res.statusCode, aborted: true });
        }
      });
      res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString() }));
      res.on("error", () => {});
    });
    req.on("error", (error) => (abortAfterFirstChunk ? resolve({ aborted: true }) : reject(error)));
    req.end();
  });
}

const TOKEN = /nx[pctb]-\d+-[0-9a-f]{8}/g;

function makeCase(i) {
  const salt = randomBytes(4).toString("hex");
  const kind = i % 3;
  if (kind === 0) {
    const probe = `nxp-${i}-${salt}`;
    const theme = `nxc-${i}-${salt}`;
    return {
      kind: "request-info",
      path: "/request-info",
      headers: { "x-nextrs-probe": probe, cookie: `other=1; theme=${theme}` },
      expected: [probe, theme],
    };
  }
  if (kind === 1) {
    const book = `nxb-${i}-${salt}`;
    return { kind: "params", path: `/books/${book}`, headers: {}, expected: [book] };
  }
  // Same request, but also carrying a query token: the URL is request state.
  const probe = `nxp-${i}-${salt}`;
  const query = `nxt-${i}-${salt}`;
  return {
    kind: "request-info+query",
    path: `/request-info?q=${query}`,
    headers: { "x-nextrs-probe": probe, cookie: `theme=${probe.replace("nxp", "nxc")}` },
    expected: [probe, probe.replace("nxp", "nxc")],
    // Next echoes the request's own query in the Flight tree ("q").
    allowed: [query],
  };
}

const failures = [];
const counts = { ok: 0, leaked: 0, missing: 0, badStatus: 0, aborted: 0, noise: 0 };

async function runCase(i) {
  const testCase = makeCase(i);
  const { status, body } = await get(testCase.path, testCase.headers);
  if (status !== 200) {
    counts.badStatus++;
    failures.push({ i, kind: testCase.kind, status });
    return;
  }
  const found = new Set(body.match(TOKEN) ?? []);
  const own = [...testCase.expected, ...(testCase.allowed ?? [])];
  const foreign = [...found].filter((token) => !own.includes(token));
  const missing = testCase.expected.filter((token) => !found.has(token));
  if (foreign.length) {
    counts.leaked++;
    failures.push({ i, kind: testCase.kind, foreign });
  } else if (missing.length) {
    counts.missing++;
    failures.push({ i, kind: testCase.kind, missing });
  } else {
    counts.ok++;
  }
}

const started = performance.now();
let next = 0;
const noise = [];
// Background streaming renders and aborted streams during the whole run.
for (let i = 0; i < aborts; i++) {
  noise.push(
    get("/dynamic", {}, { abortAfterFirstChunk: true }).then(() => counts.aborted++),
    get("/dynamic", {}).then((r) => r.status === 200 && counts.noise++),
  );
}
await Promise.all(
  Array.from({ length: concurrency }, async () => {
    while (next < total) await runCase(next++);
  }),
);
await Promise.all(noise);
// The server must still answer correctly after the aborts.
const after = await get("/request-info", { "x-nextrs-probe": "nxp-999999-00000000" });
const healthyAfter = after.status === 200 && after.body.includes("nxp-999999-00000000");

const result = {
  target: target.href,
  requests: total,
  concurrency,
  elapsedMs: Math.round(performance.now() - started),
  counts,
  healthyAfter,
  failures: failures.slice(0, 20),
};
console.log(JSON.stringify(result, null, 2));
if (args.json) writeFileSync(args.json, JSON.stringify(result, null, 2));
agent.destroy();
process.exitCode = counts.ok === total && healthyAfter ? 0 : 1;
