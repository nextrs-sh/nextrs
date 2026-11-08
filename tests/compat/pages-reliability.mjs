// NextRS-only Pages Router reliability: starts its own NextRS on
// fixtures/next-pages with short limits and checks that
//   * a getServerSideProps/API workload keeps being served while one API
//     route stalls its isolate (the isolate is replaced, the others serve);
//   * an isolate that reaches its heap limit is replaced;
//   * a mutation in flight when its isolate fails is recorded once (no
//     replay), and the client gets an error, not a success;
//   * graceful shutdown lets an in-flight getServerSideProps request finish.
//
//   node tests/compat/pages-reliability.mjs --nextrs target/release/nextrs --project fixtures/next-pages
//        --upstream URL [--port 5960] [--json out.json]
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { parseArgs, sleep } from "./routing-lib.mjs";

const args = parseArgs();
const port = Number(args.port ?? 5960);
const base = `http://127.0.0.1:${port}`;
const results = [];
const record = (name, ok, detail) => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}  ${JSON.stringify(detail).slice(0, 300)}`);
};

function start(isolates) {
  const child = spawn(args.nextrs, ["start", args.project, "--port", String(port), "--js-runtime", "embedded-v8", "--isolates", String(isolates),
    "--js-stall-timeout", "2", "--js-heap-limit", "96", "--request-timeout", "20", "--shutdown-grace", "10",
    "--js-env", "NEXTRS_UPSTREAM", "--net-allow", args.upstream, "--net-allow-addr", "127.0.0.1"], { env: { ...process.env, NEXTRS_UPSTREAM: args.upstream }, stdio: ["ignore", "ignore", "pipe"] });
  child.stderr.on("data", () => {});
  return child;
}
async function ready() {
  for (let i = 0; i < 120; i++) {
    try {
      if ((await fetch(`${base}/api/health`)).ok) return;
    } catch {}
    await sleep(250);
  }
  throw new Error("NextRS did not start");
}
const status = (path, init) => fetch(`${base}${path}`, init).then(async (r) => { await r.text(); return r.status; }, () => 0);

// 1-2: four isolates, a stall and a heap exhaustion under load.
let server = start(4);
await ready();
{
  let stop = false;
  const tally = { ok: 0, failed: 0 };
  const load = Array.from({ length: 16 }, async (_, i) => {
    while (!stop) {
      const code = await status(i % 2 ? `/books/${i}` : `/ssr/slow?id=${i}&ms=50`);
      code === 200 ? tally.ok++ : tally.failed++;
    }
  });
  const stalled = await status("/api/spin?ms=8000");
  const heap = await status("/api/heap");
  await sleep(1500);
  stop = true;
  await Promise.all(load);
  const after = await status("/books/after");
  // The requests that shared an isolate with the stall or the heap
  // exhaustion fail with it; the rest are served throughout.
  record("stall-and-heap-limit-under-load", stalled >= 500 && heap >= 500 && after === 200 && tally.ok > 50 && tally.failed < tally.ok / 4, { stalled, heap, after, tally });
}
server.kill("SIGTERM");
await new Promise((r) => server.once("exit", r));

// 3: one isolate: a mutation in flight when the isolate fails.
server = start(1);
await ready();
{
  const key = `rel-${Date.now()}`;
  const mutation = status(`/api/mutate?key=${key}&id=1&ms=3000`, { method: "POST" });
  await sleep(300);
  const spin = status("/api/spin?ms=6000");
  const [mutationStatus, spinStatus] = await Promise.all([mutation, spin]);
  await sleep(500);
  const ledger = await fetch(`${args.upstream}/mutations?key=${key}`).then((r) => r.json());
  const after = await status("/books/1");
  record("no-replay-after-isolate-failure", mutationStatus !== 200 && JSON.stringify(ledger.ids) === '["1"]' && after === 200, { mutationStatus, spinStatus, ledger: ledger.ids, after });
}

// 4: graceful shutdown with a getServerSideProps request in flight.
{
  const inflight = fetch(`${base}/ssr/slow?id=draining&ms=1500`).then(async (r) => ({ status: r.status, body: await r.text() }), (e) => ({ status: 0, body: String(e) }));
  await sleep(300);
  server.kill("SIGTERM");
  const response = await inflight;
  const code = await new Promise((r) => server.once("exit", r));
  record("graceful-shutdown-drains-ssr", response.status === 200 && response.body.replace(/<!-- -->/g, "").includes("Slow draining"), { status: response.status, exit: code });
}

if (args.json) writeFileSync(args.json, JSON.stringify({ results }, null, 2));
process.exit(results.every((r) => r.ok) ? 0 : 1);
