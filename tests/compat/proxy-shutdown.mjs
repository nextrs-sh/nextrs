// Graceful shutdown with compiled-proxy work in flight (NextRS only):
// starts nextrs, puts requests in each state below in flight, sends
// SIGTERM, and checks what each request got, that new connections are
// refused, that the process exits within its grace periods, and that no
// mutation ran twice.
//
//   node tests/compat/proxy-shutdown.mjs --nextrs target/release/nextrs \
//        --platform fixtures/next-platform-proxy --proxy fixtures/next-proxy \
//        --upstream URL --closed-port N --unlisted-port N [--isolates N] [--json out.json]
//
// The platform fixture must be built against --upstream (its external
// rewrite destinations are fixed at build time). In flight at SIGTERM:
//   * a proxy awaiting a 2 s upstream fetch (/gate/timeout's sibling: a
//     slow flag, answered within the grace period);
//   * an external rewrite to a 3 s upstream, and a streamed one;
//   * a proxy rewrite to a dynamic SSR page (identity checked);
//   * waitUntil work that writes to the ledger 1.5 s after its response,
//     and waitUntil work that never settles;
//   * a proxy stuck on the CPU (/gate/spin, past the stall limit);
//   * Server Actions behind the proxy (fixtures/next-proxy), each with its
//     own ledger id: each may complete or fail, but never run twice, and a
//     200 must have run.
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { readFileSync } from "node:fs";
import http from "node:http";
import net from "node:net";
import { parseArgs, sleep } from "./routing-lib.mjs";

const args = parseArgs();
const { nextrs, platform, proxy, upstream } = args;
if (!nextrs || !platform || !proxy || !upstream) {
  console.error("usage: proxy-shutdown.mjs --nextrs BIN --platform DIR --proxy DIR --upstream URL [--json FILE]");
  process.exit(2);
}
const isolates = String(args.isolates ?? 2);
const grace = 4;
const ledger = async (key) => (await (await fetch(`${upstream}/mutations?key=${encodeURIComponent(key)}`)).json()).ids;
const run = Date.now().toString(36);

function start(serve, port) {
  const child = spawn(
    nextrs,
    [
      "start", serve, "--port", String(port), "--js-runtime", "embedded-v8", "--isolates", isolates,
      "--js-stall-timeout", "2", "--request-timeout", "15", "--shutdown-grace", String(grace),
      "--js-env", "NEXTRS_UPSTREAM", "--js-env", "NEXTRS_CLOSED_PORT", "--js-env", "NEXTRS_UNLISTED_PORT",
      "--net-allow", upstream, "--net-allow-addr", "127.0.0.1",
    ],
    { stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, NEXTRS_UPSTREAM: upstream } },
  );
  let log = "";
  child.stdout.on("data", (d) => (log += d));
  child.stderr.on("data", (d) => (log += d));
  const exited = new Promise((resolve) => child.on("exit", (code, signal) => resolve({ code, signal, at: Date.now() })));
  return { child, exited, log: () => log };
}
async function ready(port) {
  for (let i = 0; i < 200; i++) {
    try {
      if ((await fetch(`http://127.0.0.1:${port}/api/health`)).ok) return;
    } catch {}
    await sleep(100);
  }
  throw new Error(`server on ${port} did not start`);
}
// A request that reports how it ended (status, body, or the error).
function track(port, path, { method = "GET", headers = {}, body } = {}) {
  return new Promise((resolve) => {
    const started = Date.now();
    const req = http.request({ host: "127.0.0.1", port, path, method, headers, agent: false }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString(), ms: Date.now() - started }));
      res.on("error", (e) => resolve({ status: res.statusCode, error: e.code ?? String(e), ms: Date.now() - started }));
      res.on("aborted", () => resolve({ status: res.statusCode, error: "aborted", ms: Date.now() - started }));
    });
    req.on("error", (e) => resolve({ error: e.code ?? String(e), ms: Date.now() - started }));
    req.end(body);
  });
}
const refuses = (port) =>
  new Promise((resolve) => {
    const socket = net.connect(port, "127.0.0.1");
    socket.on("connect", () => {
      socket.destroy();
      resolve(false);
    });
    socket.on("error", () => resolve(true));
  });

const checks = [];
const check = (name, ok, detail) => {
  checks.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${ok ? "" : `  ${JSON.stringify(detail).slice(0, 600)}`}`);
};

// --- Phase 1: the platform fixture -------------------------------------------------
{
  const port = 5900 + Math.floor(Math.random() * 50);
  const server = start(platform, port);
  await ready(port);
  const wuKey = `shutdown-wu-${run}`;
  const inflight = {
    fetchInProxy: track(port, "/gate/status/200"),
    slowFlag: track(port, `/gate/wait-until?key=${wuKey}&ms=1500`),
    waitUntilForever: track(port, "/gate/wait-until-forever"),
    externalSlow: track(port, "/ext-slow"),
    externalStream: track(port, "/ext-stream"),
    rewriteToSsr: track(port, `/gate/identity?id=shutdown-${run}&ms=400`),
  };
  await sleep(250);
  const signalled = Date.now();
  server.child.kill("SIGTERM");
  await sleep(300);
  const refused = await refuses(port);
  const results = Object.fromEntries(await Promise.all(Object.entries(inflight).map(async ([k, p]) => [k, await p])));
  const exit = await server.exited;
  const exitMs = exit.at - signalled;
  await sleep(500);
  const wu = await ledger(wuKey);

  check("new-connections-refused-after-sigterm", refused, { refused });
  check("proxy-awaiting-fetch-completes", results.fetchInProxy.status === 200, results.fetchInProxy);
  check("external-rewrite-to-slow-upstream-completes", results.externalSlow.status === 200 && results.externalSlow.body.includes("ext-slow"), results.externalSlow);
  check("streamed-external-rewrite-completes", results.externalStream.status === 200 && results.externalStream.body.endsWith("line 4\n"), results.externalStream);
  check(
    "proxy-rewrite-to-ssr-completes-with-its-identity",
    results.rewriteToSsr.status === 200 && results.rewriteToSsr.body.includes(`shutdown-${run}`),
    { status: results.rewriteToSsr.status, error: results.rewriteToSsr.error },
  );
  check("response-before-pending-wait-until", results.slowFlag.status === 200 && results.waitUntilForever.status === 200, {
    slowFlag: results.slowFlag.status,
    forever: results.waitUntilForever.status,
  });
  check("wait-until-within-grace-completes-once", wu.length === 1, { ledger: wu });
  // Connections drain for up to the grace period, then the isolates do:
  // the never-settling waitUntil is ended there. Twice the grace plus
  // margin bounds the exit.
  check("process-exits-within-grace", exit.code === 0 && exitMs < (2 * grace + 3) * 1000, { ...exit, exitMs });
  if (!checks.every((c) => c.ok)) console.log(server.log().split("\n").slice(-25).join("\n"));
}

// --- Phase 1b: a proxy stuck on the CPU at SIGTERM --------------------------------------
// Alone: the stall watchdog fails every request of its isolate (the
// per-isolate blast radius), which is not what the shutdown checks above
// are about.
{
  const port = 5955 + Math.floor(Math.random() * 5);
  const server = start(platform, port);
  await ready(port);
  const stalled = track(port, "/gate/spin?ms=8000");
  await sleep(300);
  const signalled = Date.now();
  server.child.kill("SIGTERM");
  const result = await stalled;
  const exit = await server.exited;
  const exitMs = exit.at - signalled;
  check("stalled-proxy-fails-without-serving", result.status !== 200, result);
  check("stalled-proxy-does-not-hold-shutdown", exit.code === 0 && exitMs < (2 * grace + 3) * 1000, { ...exit, exitMs });
}

// --- Phase 2: Server Actions behind the proxy (fixtures/next-proxy) ------------------
{
  const port = 5960 + Math.floor(Math.random() * 30);
  const server = start(`${proxy}/.next`, port);
  await ready(port);
  const manifest = JSON.parse(readFileSync(`${proxy}/.next/server/server-reference-manifest.json`, "utf8"));
  const record = Object.entries(manifest.node).find(([, v]) => v.exportedName === "record" && v.filename === "app/actions/actions.ts")[0];
  const key = `shutdown-actions-${run}`;
  const sent = [];
  let stop = false;
  const workers = Array.from({ length: 8 }, async (_, w) => {
    for (let i = 0; !stop; i++) {
      const id = `${w}-${i}`;
      const boundary = `----sd${w}${i}`;
      const body = [`--${boundary}`, 'Content-Disposition: form-data; name="_1_key"', "", key, `--${boundary}`, 'Content-Disposition: form-data; name="_1_id"', "", id, `--${boundary}`, 'Content-Disposition: form-data; name="0"', "", '["$K1"]', `--${boundary}--`, ""].join("\r\n");
      const r = await track(port, "/actions", {
        method: "POST",
        headers: { "next-action": record, origin: `http://127.0.0.1:${port}`, accept: "text/x-component", "content-type": `multipart/form-data; boundary=${boundary}`, "content-length": String(Buffer.byteLength(body)) },
        body,
      });
      sent.push({ id, status: r.status, error: r.error });
      if (r.error === "ECONNREFUSED") break;
    }
  });
  await sleep(400);
  server.child.kill("SIGTERM");
  await sleep(200);
  stop = true;
  await Promise.all(workers);
  await server.exited;
  await sleep(300);
  const ids = await ledger(key);
  const counts = new Map();
  for (const id of ids) counts.set(id, (counts.get(id) ?? 0) + 1);
  const twice = [...counts].filter(([, n]) => n > 1);
  const okNotRun = sent.filter((s) => s.status === 200 && !counts.has(s.id));
  check("actions-never-run-twice", twice.length === 0, { twice: twice.slice(0, 5) });
  check("actions-answered-200-did-run", okNotRun.length === 0, { okNotRun: okNotRun.slice(0, 5) });
  check("actions-in-flight-at-sigterm", sent.length > 0, {
    sent: sent.length,
    ok: sent.filter((s) => s.status === 200).length,
    refused: sent.filter((s) => s.error === "ECONNREFUSED").length,
    other: sent.filter((s) => s.status !== 200 && s.error !== "ECONNREFUSED").slice(0, 5),
  });
}

const failed = checks.filter((c) => !c.ok).length;
console.log(`proxy-shutdown: ${checks.length - failed}/${checks.length} PASS`);
if (args.json) writeFileSync(args.json, JSON.stringify({ checks }, null, 2));
process.exit(failed ? 1 : 0);
