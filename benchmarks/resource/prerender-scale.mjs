// RE-5B: memory and latency of prerendered output against build size,
// process level (not a container measurement). For each build of
// fixtures/next-prerender-scale (100 / 1 000 / 10 000 / 50 000 routes) and
// each mode (the default, which reads every prerender at startup, and
// --prerender-cache N), start NextRS with one isolate and record:
//
//   startup_ms     until /p/0 answers 200
//   idle           RssAnon / RssFile / VmRSS 5 s after ready (MiB)
//   first / warm   latency of 200 distinct routes (sequential), then again
//   sustained      RssAnon after a sweep of up to 10 000 distinct routes
//                  (8 concurrent), then 5 s idle
//   sha            one body digest per sampled route, compared across modes
//
//   node benchmarks/resource/prerender-scale.mjs --bin target/re5-cand-rel/release/nextrs
//     [--sizes 100,1000,10000,50000] [--modes eager,cache0,cache32] [--rounds 1]
//     [--out engineering/performance/resource-efficiency/results/re5/prerender]
//
// The page cache (file pages read from the build) is charged to a
// container's cgroup but is reclaimable; RssFile shows only what the
// process has mapped. Run it on an otherwise idle host.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, appendFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) if (argv[i].startsWith("--")) args[argv[i].slice(2)] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
const ROOT = resolve(new URL("../..", import.meta.url).pathname);
const bin = resolve(args.bin ?? join(ROOT, "target/release/nextrs"));
const sizes = String(args.sizes ?? "100,1000,10000,50000").split(",").map(Number);
const modes = String(args.modes ?? "eager,cache0,cache32").split(",");
const rounds = Number(args.rounds ?? 1);
const out = resolve(args.out ?? join(ROOT, "engineering/performance/resource-efficiency/results/re5/prerender"));
mkdirSync(out, { recursive: true });
const fixture = join(ROOT, "fixtures/next-prerender-scale");
let port = 7300 + Math.floor(Math.random() * 500);

const modeArgs = (mode) => (mode === "eager" ? [] : ["--prerender-cache", mode.replace("cache", "")]);

function status(pid) {
  const st = readFileSync(`/proc/${pid}/status`, "utf8");
  const kb = (k) => Number(new RegExp(`${k}:\\s+(\\d+)`).exec(st)?.[1] ?? 0);
  const mib = (k) => +(kb(k) / 1024).toFixed(1);
  return { rss_mib: mib("VmRSS"), anon_mib: mib("RssAnon"), file_mib: mib("RssFile"), hwm_mib: mib("VmHWM") };
}

async function get(p, path) {
  const t0 = performance.now();
  const res = await fetch(`http://127.0.0.1:${p}${path}`);
  const body = Buffer.from(await res.arrayBuffer());
  return { ms: performance.now() - t0, status: res.status, body };
}

const pct = (xs, q) => {
  const s = [...xs].sort((a, b) => a - b);
  return +s[Math.min(s.length - 1, Math.floor(q * s.length))].toFixed(3);
};

async function run(size, mode, round) {
  const dist = join(fixture, `.next-${size}`);
  if (!existsSync(dist)) throw new Error(`missing build ${dist}`);
  const p = port++;
  const t0 = performance.now();
  const child = spawn(bin, ["start", dist, "--port", String(p), "--js-runtime", "embedded-v8", "--isolates", "1", ...modeArgs(mode)], {
    cwd: fixture,
    env: { ...process.env, NODE_ENV: "production", RUST_LOG: "warn" },
    stdio: ["ignore", "ignore", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", (c) => (stderr = (stderr + c).slice(-4000)));
  let startup_ms = null;
  for (let i = 0; i < 6000; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${p}/p/0`);
      await r.arrayBuffer();
      if (r.status === 200) { startup_ms = Math.round(performance.now() - t0); break; }
    } catch {}
    if (child.exitCode !== null) throw new Error(`exited: ${stderr}`);
    await sleep(25);
  }
  if (startup_ms === null) throw new Error(`not ready: ${stderr}`);
  await sleep(5000);
  const idle = status(child.pid);
  // 200 distinct routes, spread over the build; the same again warm.
  const sample = Array.from({ length: Math.min(200, size) }, (_, i) => Math.floor((i * size) / Math.min(200, size)));
  const first = [], warm = [];
  const digest = createHash("sha256");
  for (const i of sample) {
    const r = await get(p, `/p/${i}`);
    if (r.status !== 200) throw new Error(`/p/${i}: ${r.status}`);
    first.push(r.ms);
    digest.update(r.body);
  }
  for (const i of sample) warm.push((await get(p, `/p/${i}`)).ms);
  const afterSample = status(child.pid);
  // Sweep: up to 10 000 distinct routes, 8 at a time.
  const sweep = Math.min(size, 10000);
  let next = 0, failed = 0;
  const t1 = performance.now();
  await Promise.all(Array.from({ length: 8 }, async () => {
    while (next < sweep) {
      const i = next++;
      const r = await get(p, `/p/${(i * 7919) % size}`);
      if (r.status !== 200) failed++;
    }
  }));
  const sweep_s = (performance.now() - t1) / 1000;
  const peak = status(child.pid);
  await sleep(5000);
  const sustained = status(child.pid);
  child.kill("SIGTERM");
  await new Promise((r) => child.once("exit", r));
  const row = {
    size, mode, round, startup_ms, idle, after_sample: afterSample, peak_after_sweep: peak, sustained,
    first_ms: { p50: pct(first, 0.5), p90: pct(first, 0.9), p99: pct(first, 0.99) },
    warm_ms: { p50: pct(warm, 0.5), p90: pct(warm, 0.9), p99: pct(warm, 0.99) },
    sweep: { requests: sweep, failed, rps: Math.round(sweep / sweep_s) },
    sha: digest.digest("hex"),
  };
  appendFileSync(join(out, "prerender-scale.jsonl"), JSON.stringify(row) + "\n");
  console.log(`${String(size).padStart(6)} ${mode.padEnd(8)} r${round} start ${String(startup_ms).padStart(6)} ms  idle anon ${String(idle.anon_mib).padStart(7)}  sustained anon ${String(sustained.anon_mib).padStart(7)}  hwm ${String(sustained.hwm_mib).padStart(7)} MiB  first p50 ${row.first_ms.p50} warm p50 ${row.warm_ms.p50} ms  sweep ${row.sweep.rps}/s fail ${failed}  ${row.sha.slice(0, 12)}`);
}

console.log(`bin ${bin}`);
for (let round = 1; round <= rounds; round++) {
  for (const size of sizes) {
    for (const mode of modes) await run(size, mode, round);
  }
}
