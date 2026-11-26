// Series 1 local reproduction driver (see README.md).
//
//   node benchmarks/series1/run.mjs setup --loadgen PATH [--bin LABEL=PATH ...]
//   node benchmarks/series1/run.mjs matrix --runtimes node,rc2,head --rounds 3 --out DIR
//   node benchmarks/series1/run.mjs capture --runtimes node,rc2,head --out DIR
//   node benchmarks/series1/run.mjs memory --runtimes rc2,head --isolates 1,2,4 --out DIR
//   node benchmarks/series1/run.mjs fills --runtimes head,... --out DIR
//   node benchmarks/series1/run.mjs teardown
//
// Every run starts a fresh storefront container (the runtime under test)
// with Railway Series 1's limits (2 vCPU, 1 GB), pinned to its own CPUs.
// Runtimes alternate order between rounds (round 1 as given, round 2
// reversed, ...). Results: one JSON file per run, plus run metadata.
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import * as L from "./lib.mjs";

const argv = process.argv.slice(2);
const command = argv[0];
const opts = { bin: [] };
for (let i = 1; i < argv.length; i++) {
  const k = argv[i].replace(/^--/, "");
  const v = argv[i + 1];
  if (k === "bin") opts.bin.push(v), i++;
  else if (v === undefined || v.startsWith("--")) opts[k] = true;
  else (opts[k] = v), i++;
}
const list = (v, d) => String(v ?? d).split(",").filter(Boolean);
const num = (v, d) => (v === undefined ? d : Number(v));
const SPECS = "/work/specs";

function log(...a) {
  console.error(new Date().toISOString().slice(11, 19), ...a);
}

/** Orders for each round: as given, then reversed, alternating. */
export function orders(runtimes, rounds) {
  return Array.from({ length: rounds }, (_, i) => (i % 2 === 0 ? [...runtimes] : [...runtimes].reverse()));
}

function trimLoadgen(r) {
  const { check_failure_samples, passed_any_by_index, ...rest } = r;
  return { ...rest, check_failure_samples: (check_failure_samples ?? []).slice(0, 3) };
}

/** Keeps the parts of NextRS's metrics.json the reports use. */
function trimMetrics(m) {
  if (!m) return null;
  const pick = (o, keys) => Object.fromEntries(keys.filter((k) => o?.[k] !== undefined).map((k) => [k, o[k]]));
  return {
    ...pick(m, ["isolates", "queued", "requests_admitted", "requests_completed", "execution_seconds_total",
      "queue_wait_seconds_total", "queue_waits", "backpressure_waits", "response_events", "memory", "cache",
      "compression_cache"]),
    executors: (m.executors ?? []).map((e) => pick(e, ["slot", "state", "heap_used_bytes", "heap_total_bytes",
      "external_bytes", "heap_used_after_gc_bytes", "full_gcs", "restarts"])),
    http: m.http && pick(m.http, ["requests", "stages", "stream_compression", "head_seconds", "duration_seconds"]),
    process: m.process,
    raw_keys: Object.keys(m),
  };
}

/**
 * `--max-host-load N`: before a measured window, wait (up to
 * `--gate-wait` seconds, default 600) until the host's 1-minute load
 * average is below N. Other work on the host takes physical cores the
 * Docker VM cannot see; the wait and the load it saw are recorded.
 */
async function gate() {
  const max = opts["max-host-load"] === undefined ? null : Number(opts["max-host-load"]);
  if (max === null) return null;
  const started = Date.now();
  const limit = num(opts["gate-wait"], 600) * 1000;
  for (;;) {
    const load = L.contention().hostLoad1;
    if (load < max) return { waitedMs: Date.now() - started, load, passed: true };
    if (Date.now() - started > limit) return { waitedMs: Date.now() - started, load, passed: false };
    await L.sleep(10_000);
  }
}

async function phase(runtime, name, spec, lg) {
  const gated = name.startsWith("warmup") ? null : await gate();
  if (gated && !gated.passed) log(`  ${name}: host load still ${gated.load} after ${gated.waitedMs} ms (measured anyway, flagged)`);
  const before = L.sample();
  const mBefore = trimMetrics(L.nextrsMetrics(runtime));
  const cBefore = L.contention();
  const result = L.loadgen(`${SPECS}/${spec}`, lg);
  const cAfter = L.contention();
  const after = L.sample();
  const mAfter = trimMetrics(L.nextrsMetrics(runtime));
  const failed = result.status_mismatch + result.body_mismatch + result.io_errors + result.timeouts;
  log(`  ${name}: ${result.completed} req, ${result.achieved_rps.toFixed(1)}/s, p50 ${result.latency.p50_ms} p99 ${result.latency.p99_ms} ms` +
    `, cpu ${((after.cpu_usage_usec - before.cpu_usage_usec) / 1000 / Math.max(result.ok, 1)).toFixed(3)} ms/req${failed ? `, FAILED ${failed}` : ""}`);
  return {
    name, spec, loadgen: lg, result: trimLoadgen(result), before, after,
    metricsBefore: mBefore, metricsAfter: mAfter, contention: { before: cBefore, after: cAfter, gate: gated },
  };
}

async function setup() {
  L.networkUp();
  if (!opts.loadgen) throw new Error("--loadgen PATH (a Linux nextrs-loadgen binary)");
  L.toolsUp(opts.loadgen);
  for (const b of opts.bin) {
    const [label, path] = b.split("=");
    L.buildRuntimeImage(label, path);
    log(`image nextrs-s1:${label} <- ${path}`);
  }
  await L.apiUp();
  log("API ready");
}

let counts = null;
async function discover() {
  const out = L.toolsNode("discover.mjs", [`http://${L.SF}:8080`, SPECS]);
  log("discover", out.trim());
  const manifest = JSON.parse(L.docker(["exec", L.TOOLS, "cat", `${SPECS}/manifest.json`]).stdout);
  counts = { assets: manifest.assets.length, rsc: manifest.rsc.length };
}

async function matrix() {
  const runtimes = list(opts.runtimes, "node,rc2,head");
  const rounds = num(opts.rounds, 3);
  const seconds = num(opts.seconds, 30);
  const staticRates = list(opts["static-rates"], "10,200,800").map(Number);
  const rscRates = list(opts["rsc-rates"], "10,50,150").map(Number);
  const out = resolve(opts.out);
  mkdirSync(out, { recursive: true });
  const meta = {
    command: process.argv.slice(2).join(" "), commit: L.gitCommit(), started: new Date().toISOString(),
    limits: L.LIMITS, images: Object.fromEntries(runtimes.map((r) => [r, L.image(r)])), host: L.hostInfo(),
    rounds, seconds, staticRates, rscRates,
  };
  writeFileSync(join(out, "meta.json"), JSON.stringify(meta, null, 1));
  let discovered = false;
  for (const [ri, order] of orders(runtimes, rounds).entries()) {
    for (const [pi, runtime] of order.entries()) {
      log(`round ${ri + 1} #${pi + 1}: ${runtime}`);
      const run = { runtime, round: ri + 1, position: pi + 1, image: L.image(runtime), phases: [] };
      const { readyMs } = await L.storefrontUp(runtime, { isolates: num(opts.isolates, 1), extraArgs: opts["extra-args"] ?? "" });
      run.readyMs = readyMs;
      if (!discovered) {
        await discover();
        discovered = true;
      }
      await L.sleep(15_000);
      run.idleStart = L.sample();
      run.metricsIdleStart = trimMetrics(L.nextrsMetrics(runtime));
      await L.sleep(30_000);
      run.idleEnd = L.sample();
      // Cold: every asset / RSC path once, one connection (first requests:
      // compression-cache misses, cache seeding from the build output).
      run.phases.push(await phase(runtime, "static-cold", "static-gzip.json", { connections: 1, requests: counts.assets, duration: 120 }));
      run.phases.push(await phase(runtime, "rsc-cold", "rsc-gzip.json", { connections: 1, requests: counts.rsc, duration: 120 }));
      // Warm-up (recorded, not reported as a result).
      run.phases.push(await phase(runtime, "warmup-static", "static-gzip.json", { rate: 100, connections: 32, duration: 15 }));
      run.phases.push(await phase(runtime, "warmup-rsc", "rsc-gzip.json", { rate: 50, connections: 32, duration: 15 }));
      const only = opts.only ? list(opts.only) : null;
      const want = (n) => !only || only.includes(n);
      for (const rate of staticRates.filter((r) => want(`static-open-${r}`))) {
        run.phases.push(await phase(runtime, `static-open-${rate}`, "static-gzip.json", { rate, connections: 64, duration: seconds }));
      }
      for (const rate of rscRates.filter((r) => want(`rsc-open-${r}`))) {
        run.phases.push(await phase(runtime, `rsc-open-${rate}`, "rsc-gzip.json", { rate, connections: 64, duration: seconds }));
      }
      if (want("static-closed-16")) run.phases.push(await phase(runtime, "static-closed-16", "static-gzip.json", { connections: 16, duration: 20 }));
      if (want("rsc-closed-8")) run.phases.push(await phase(runtime, "rsc-closed-8", "rsc-gzip.json", { connections: 8, duration: 20 }));
      // Recovery: idle, then read memory again.
      await L.sleep(60_000);
      run.recovered = L.sample();
      run.metricsRecovered = trimMetrics(L.nextrsMetrics(runtime));
      L.remove(L.SF);
      writeFileSync(join(out, `r${ri + 1}-p${pi + 1}-${runtime}.json`), JSON.stringify(run));
    }
  }
  meta.finished = new Date().toISOString();
  meta.hostAfter = L.hostInfo();
  writeFileSync(join(out, "meta.json"), JSON.stringify(meta, null, 1));
  log("matrix done", out);
}

async function capture() {
  const runtimes = list(opts.runtimes, "node,rc2,head");
  const out = resolve(opts.out);
  mkdirSync(out, { recursive: true });
  let discovered = false;
  for (const runtime of runtimes) {
    await L.storefrontUp(runtime);
    if (!discovered) {
      await discover();
      discovered = true;
    }
    L.docker(["exec", L.TOOLS, "node", "/s1/capture.mjs", `http://${L.SF}:8080`, SPECS, `/work/capture-${runtime}.json`]);
    const json = L.docker(["exec", L.TOOLS, "cat", `/work/capture-${runtime}.json`]).stdout;
    writeFileSync(join(out, `capture-${runtime}.json`), json);
    L.docker(["exec", L.TOOLS, "cat", `${SPECS}/manifest.json`]);
    writeFileSync(join(out, "manifest.json"), L.docker(["exec", L.TOOLS, "cat", `${SPECS}/manifest.json`]).stdout);
    log(`captured ${runtime}`);
    L.remove(L.SF);
  }
}

/**
 * Memory by isolate count: idle after start, after RSC and static load,
 * and after a recovery period. One container per (runtime, isolates).
 */
async function memory() {
  const runtimes = list(opts.runtimes, "head");
  const isolateCounts = list(opts.isolates, "1,2,4").map(Number);
  const rounds = num(opts.rounds, 2);
  const out = resolve(opts.out);
  mkdirSync(out, { recursive: true });
  let discovered = false;
  for (let round = 1; round <= rounds; round++) {
    for (const runtime of round % 2 ? runtimes : [...runtimes].reverse()) {
      const counts = runtime === "node" ? [1] : isolateCounts;
      for (const isolates of counts) {
        const jsModes = runtime === "node" || !opts["no-js"] ? ["default"] : ["none", "default"];
        // --js none|default: only that mode (NextRS).
        for (const jsRuntime of jsModes.filter((m) => !opts.js || runtime === "node" || m === opts.js)) {
          if (jsRuntime === "none" && isolates !== counts[0]) continue;
          log(`memory ${runtime} isolates=${isolates} js=${jsRuntime} round ${round}`);
          const run = { runtime, isolates: jsRuntime === "none" ? 0 : isolates, jsRuntime, round, phases: [] };
          try {
            run.readyMs = (await L.storefrontUp(runtime, { isolates, noJs: jsRuntime === "none" })).readyMs;
          } catch (error) {
            run.error = String(error.message);
            writeFileSync(join(out, `${runtime}-i${isolates}-${jsRuntime}-r${round}.json`), JSON.stringify(run));
            L.remove(L.SF);
            continue;
          }
          // Discovery renders `/`: only on a runtime that runs JavaScript
          // (a no-JS run reads the manifest a previous run wrote).
          if (!discovered && jsRuntime !== "none") {
            await discover();
            discovered = true;
          }
          await L.sleep(20_000);
          run.idle = L.sample();
          run.metricsIdle = trimMetrics(L.nextrsMetrics(runtime));
          run.phases.push(await phase(runtime, "static-load", "static-gzip.json", { rate: 200, connections: 32, duration: 30 }));
          if (jsRuntime !== "none") {
            run.phases.push(await phase(runtime, "rsc-load", "rsc-gzip.json", { connections: 4 * isolates, duration: 30 }));
          }
          run.afterLoad = L.sample();
          await L.sleep(num(opts.recovery, 90) * 1000);
          run.recovered = L.sample();
          run.metricsRecovered = trimMetrics(L.nextrsMetrics(runtime));
          writeFileSync(join(out, `${runtime}-i${isolates}-${jsRuntime}-r${round}.json`), JSON.stringify(run));
          L.remove(L.SF);
        }
      }
    }
  }
}

/**
 * Compression-cache fills and tail latency: fresh containers, so every
 * first request is a miss. (a) 32 simultaneous requests for the largest
 * asset; (b) every asset at once on 32 connections; (c) the same under a
 * 1 MiB cache (eviction and refill on every pass), at a fixed rate.
 */
async function fills() {
  const runtimes = list(opts.runtimes, "head");
  const rounds = num(opts.rounds, 3);
  const out = resolve(opts.out);
  mkdirSync(out, { recursive: true });
  let discovered = false;
  for (let round = 1; round <= rounds; round++) {
    for (const runtime of round % 2 ? runtimes : [...runtimes].reverse()) {
      for (const cache of ["default", "1"]) {
        if (runtime === "node" && cache !== "default") continue;
        const extraArgs = cache === "default" ? "" : `--compression-cache ${cache}`;
        log(`fills ${runtime} cache=${cache} round ${round}`);
        await L.storefrontUp(runtime, { extraArgs });
        if (!discovered) {
          await discover();
          discovered = true;
        }
        await L.sleep(10_000);
        const run = { runtime, cache, round, phases: [] };
        run.phases.push(await phase(runtime, "same-asset-burst-32", "static-largest.json", { connections: 32, requests: 32, duration: 10 }));
        run.phases.push(await phase(runtime, "all-assets-burst-32", "static-gzip.json", { connections: 32, requests: counts.assets, duration: 60 }));
        run.phases.push(await phase(runtime, "static-open-400", "static-gzip.json", { rate: 400, connections: 64, duration: 60 }));
        run.end = L.sample();
        writeFileSync(join(out, `${runtime}-cache${cache}-r${round}.json`), JSON.stringify(run));
        L.remove(L.SF);
      }
    }
  }
}

function teardown() {
  for (const name of [L.SF, L.TOOLS, L.API]) L.remove(name);
  L.docker(["network", "rm", L.NET], { allowFail: true });
}

const commands = { setup, matrix, capture, memory, fills, teardown };
if (!commands[command]) {
  console.error(`usage: run.mjs ${Object.keys(commands).join("|")} [options]`);
  process.exit(2);
}
await commands[command]();
