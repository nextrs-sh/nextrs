// Memory under sustained load: one workload (default: the mixed App/Pages
// Router + image mix) at a fixed offered rate (open loop) for many minutes,
// then idle, with memory sampled throughout. Compares configurations of
// NextRS (binaries, start arguments, environment such as allocator
// tunables), alternating their order across rounds.
//
//   node benchmarks/phase3a/memory.mjs --out DIR
//     --config "label|binary|start args|ENV=V,ENV2=V2" [--config ...]
//     [--group mixed] [--workload mixed] [--isolates 4] [--rate 1500]
//     [--minutes 10] [--idle 2] [--sample 10] [--rounds 2] [--connections 256]
//
// Per sample: process-tree RSS, anonymous/file RSS, swap, glibc malloc
// arena/in-use/free/releasable bytes and V8 heap per isolate
// (/metrics.json). Summary per run: anonymous RSS at the end of the load,
// its slope over the second half of the load (MiB/min), after idle, and
// malloc free bytes (memory the allocator keeps without live data).
// Requests are checked by the load generator as in run.mjs; a run with
// failed checks is marked invalid.
import { mkdirSync, writeFileSync, appendFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawn } from "node:child_process";
import * as L from "./lib.mjs";
import { groups } from "./workloads.mjs";

const argv = process.argv.slice(2);
const args = { config: [] };
for (let i = 0; i < argv.length; i++) {
  if (!argv[i].startsWith("--")) continue;
  const k = argv[i].slice(2);
  const v = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
  if (k === "config") args.config.push(v);
  else args[k] = v;
}
const ROOT = resolve(new URL("../..", import.meta.url).pathname);
const out = resolve(args.out ?? join(ROOT, "benchmarks/results/phase3a/memory"));
mkdirSync(join(out, "specs"), { recursive: true });
const group = groups.find((g) => g.id === (args.group ?? "mixed"));
const workload = group.workloads.find((w) => w.id === (args.workload ?? group.workloads[0].id));
const isolates = Number(args.isolates ?? 4);
const rate = Number(args.rate ?? 1500);
const minutes = Number(args.minutes ?? 10);
const idle = Number(args.idle ?? 2);
const every = Number(args.sample ?? 10);
const rounds = Number(args.rounds ?? 2);
const connections = Number(args.connections ?? 256);
const loadgenBin = join(ROOT, "benchmarks/loadgen/target/release/nextrs-loadgen");
const configs = args.config.map((c) => {
  const [label, bin, startArgs = "", env = ""] = c.split("|");
  return {
    label,
    bin: resolve(bin),
    extra: startArgs.trim() ? startArgs.trim().split(/\s+/) : [],
    env: Object.fromEntries(env.split(",").filter(Boolean).map((kv) => [kv.slice(0, kv.indexOf("=")), kv.slice(kv.indexOf("=") + 1)])),
  };
});
if (!configs.length) throw new Error("give at least one --config");
const log = (...m) => console.log(new Date().toISOString().slice(11, 19), ...m);
const mib = (b) => (b == null ? null : +(b / 1048576).toFixed(1));
const env = L.environment({ node: process.execPath, nextrs: configs[0].bin, root: ROOT, loadgenBin, serverCpus: "0-3", loadCpus: "4-5" });
env.memory_parameters = { group: group.id, workload: workload.id, isolates, rate, minutes, idle, every, rounds, connections, configs };
writeFileSync(join(out, "environment.json"), JSON.stringify(JSON.parse(JSON.stringify(env).replaceAll(`${ROOT}/`, "")), null, 2));

async function snapshot(s, t0, phase) {
  const p = L.sample(s.pid);
  const m = await L.metrics(s.metricsPort);
  const malloc = m?.memory?.malloc;
  return {
    t: +((Date.now() - t0) / 1000).toFixed(1), phase,
    rss_mib: p.rss_mib, anon_mib: p.anon_mib, file_mib: p.file_mib, swap_mib: p.swap_mib, threads: p.threads,
    malloc_arena_mib: mib(malloc?.arena_bytes), malloc_mmap_mib: mib(malloc?.mmap_bytes), malloc_in_use_mib: mib(malloc?.in_use_bytes),
    malloc_free_mib: mib(malloc?.free_bytes), malloc_releasable_mib: mib(malloc?.releasable_bytes),
    v8_heap_mib: m?.executors?.map((e) => mib(e.heap_total_bytes)), v8_used_after_gc_mib: m?.executors?.map((e) => mib(e.heap_used_after_gc_bytes)),
    v8_external_mib: m?.executors?.map((e) => mib(e.external_bytes)),
    image_cache_mib: mib(m?.images?.cache_memory_bytes),
    image_reserved_mib: mib(m?.images?.memory_reserved_bytes),
    requests: m?.requests_completed,
  };
}

function slope(points) {
  // Least squares, MiB per minute.
  const n = points.length;
  if (n < 2) return null;
  const mx = points.reduce((a, p) => a + p[0], 0) / n, my = points.reduce((a, p) => a + p[1], 0) / n;
  const num = points.reduce((a, p) => a + (p[0] - mx) * (p[1] - my), 0), den = points.reduce((a, p) => a + (p[0] - mx) ** 2, 0);
  return den ? +((num / den) * 60).toFixed(2) : null;
}

let port = 6500;
for (let round = 1; round <= rounds; round++) {
  const order = round % 2 ? configs : [...configs].reverse();
  for (const c of order) {
    const ctx = { group, config: { id: c.label, kind: "nextrs" }, port: port++, root: ROOT, node: process.execPath, scratch: join(out, "specs") };
    const metricsPort = port++;
    ctx.services = group.services ? await group.services(ctx) : null;
    log(`round ${round} ${c.label}`);
    const s = await L.startServer({
      kind: "nextrs", nextrs: c.bin, project: join(ROOT, group.fixture), dist: group.dist ?? ".next", port: ctx.port, metricsPort, isolates,
      cpus: "0-3", env: { ...(group.env?.(ctx) ?? {}), ...c.env }, extraArgs: [...(group.nextrsArgs?.(ctx) ?? []), ...c.extra], readyPath: group.readyPath ?? "/",
    });
    ctx.server = s;
    const series = join(out, `${c.label}.round${round}.jsonl`);
    try {
      const spec = typeof workload.spec === "function" ? await workload.spec(ctx) : workload.spec;
      const specPath = join(out, "specs", `${c.label}.json`);
      writeFileSync(specPath, JSON.stringify(spec));
      const t0 = Date.now();
      const samples = [];
      const record = async (phase) => {
        const x = await snapshot(s, t0, phase);
        samples.push(x);
        appendFileSync(series, JSON.stringify(x) + "\n");
      };
      await record("start");
      // The load runs in its own process; memory is sampled meanwhile.
      const lgOut = join(out, `${c.label}.round${round}.load.json`);
      const lg = spawn("taskset", ["-c", "4-5", loadgenBin, "--target", `127.0.0.1:${ctx.port}`, "--spec", specPath, "--connections", String(connections),
        "--rate", String(rate), "--duration", String(minutes * 60), "--warmup", "0", "--threads", "2", "--timeout", "30", "--out", lgOut], { stdio: "ignore" });
      const done = new Promise((r) => lg.on("close", r));
      let running = true;
      done.then(() => (running = false));
      while (running) {
        await Promise.race([L.sleep(every * 1000), done]);
        await record("load");
      }
      const load = JSON.parse((await import("node:fs")).readFileSync(lgOut, "utf8"));
      const post = workload.postcheck ? await workload.postcheck(ctx, load) : null;
      const idleEnd = Date.now() + idle * 60_000;
      while (Date.now() < idleEnd) {
        await L.sleep(every * 1000);
        await record("idle");
      }
      const loadPts = samples.filter((x) => x.phase === "load");
      const second = loadPts.filter((x) => x.t >= (minutes * 60) / 2);
      const last = loadPts[loadPts.length - 1];
      const end = samples[samples.length - 1];
      const summary = {
        label: c.label, round, valid: load.ok === load.completed && load.io_errors === 0 && load.timeouts === 0 && (!post || post.ok),
        achieved_rps: load.achieved_rps, completed: load.completed, errors: { status: load.status_mismatch, body: load.body_mismatch, io: load.io_errors, timeouts: load.timeouts },
        latency: load.latency, by_kind: load.by_kind, postcheck: post,
        anon_end_load_mib: last?.anon_mib, rss_peak_mib: Math.max(...samples.map((x) => x.rss_mib)), anon_peak_mib: Math.max(...samples.map((x) => x.anon_mib)),
        anon_slope_second_half_mib_per_min: slope(second.map((x) => [x.t, x.anon_mib])),
        anon_after_idle_mib: end.anon_mib, malloc_free_end_load_mib: last?.malloc_free_mib, malloc_free_after_idle_mib: end.malloc_free_mib,
        malloc_in_use_end_load_mib: last?.malloc_in_use_mib, swap_max_mib: Math.max(...samples.map((x) => x.swap_mib)),
      };
      appendFileSync(join(out, "summary.jsonl"), JSON.stringify(summary) + "\n");
      log(`  ${c.label}: ${summary.valid ? "ok" : "INVALID"} ${Math.round(load.achieved_rps)} rps p99 ${load.latency.p99_ms} ms; anon end ${summary.anon_end_load_mib} MiB, slope ${summary.anon_slope_second_half_mib_per_min} MiB/min, after idle ${summary.anon_after_idle_mib}; malloc free ${summary.malloc_free_end_load_mib} -> ${summary.malloc_free_after_idle_mib} MiB`);
    } finally {
      await L.stopServer(s);
      await ctx.services?.stop?.();
    }
  }
}
log("done", out.replace(`${ROOT}/`, ""));
