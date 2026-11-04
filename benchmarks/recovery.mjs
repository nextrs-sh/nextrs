// Executor recovery time after a CPU stall, seen from a client.
//
//   nextrs start fixtures/next-app --port 3501 --js-runtime embedded-v8 \
//       --js-stall-timeout 1 --metrics-addr 127.0.0.1:9501
//   node benchmarks/recovery.mjs --target http://127.0.0.1:3501 \
//       --metrics http://127.0.0.1:9501 [--runs 5] [--spacing 65] [--json out.json]
//
// Each run sends GET /api/spin?ms=5000 (the isolate never yields), then
// polls /api/health every 10 ms. It records when the spin request was
// answered (502 after the watchdog fired) and when /api/health first
// succeeded on the replacement isolate (generation changed). Runs are
// spaced beyond the supervisor's 60 s failure window so the restart
// backoff stays at its initial 100 ms.
import http from "node:http";
import { writeFileSync } from "node:fs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const target = new URL(args.target ?? "http://127.0.0.1:3501");
const metricsUrl = new URL("/metrics.json", args.metrics ?? "http://127.0.0.1:9501");
const runs = Number(args.runs ?? 5);
const spacing = Number(args.spacing ?? 65);

function get(url, timeoutMs = 10_000) {
  return new Promise((resolve) => {
    const req = http.get(url, { agent: false, timeout: timeoutMs }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString() }));
      res.on("error", () => resolve({ status: 0 }));
    });
    req.on("timeout", () => req.destroy());
    req.on("error", () => resolve({ status: 0 }));
  });
}

const generation = async () => JSON.parse((await get(metricsUrl)).body).executors[0].generation;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const results = [];
for (let run = 0; run < runs; run++) {
  if (run > 0) await sleep(spacing * 1000);
  const before = await generation();
  const t0 = performance.now();
  let spinAnswered = null;
  let spinStatus = null;
  get(new URL("/api/spin?ms=5000", target), 30_000).then((r) => {
    spinAnswered = performance.now() - t0;
    spinStatus = r.status;
  });
  await sleep(20);
  let firstOk = null;
  while (performance.now() - t0 < 20_000) {
    const r = await get(new URL("/api/health", target), 2_000);
    if (r.status === 200 && (await generation()) > before) {
      firstOk = performance.now() - t0;
      break;
    }
    await sleep(10);
  }
  while (spinAnswered === null) await sleep(10);
  const row = {
    run,
    spinStatus,
    spinAnsweredMs: Math.round(spinAnswered),
    servingAgainMs: firstOk === null ? null : Math.round(firstOk),
    afterWatchdogMs: firstOk === null ? null : Math.round(firstOk - 1000),
    generation: await generation(),
  };
  console.log(JSON.stringify(row));
  results.push(row);
}
if (args.json) writeFileSync(args.json, JSON.stringify({ target: target.href, runs: results }, null, 2));
