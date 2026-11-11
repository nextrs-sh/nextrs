// Phase 3A benchmark driver: NextRS against `next start` on identical
// builds, workload by workload. See engineering/benchmarks.md and
// docs/research/public-benchmark-methodology.md for the method.
//
//   node benchmarks/phase3a/run.mjs --out benchmarks/results/phase3a/<name>
//     [--nextrs target/release/nextrs] [--label current]
//     [--nextrs-b PATH --label-b NAME]       # a second NextRS build (A/B)
//     [--args "..." --args-b "..."]          # extra nextrs start arguments per build
//     [--env K=V,... --env-b K=V,...]        # extra environment per build
//     [--isolates 1,2,4] [--no-next]          # configurations
//     [--groups app,pages,...] [--only id,...] # workload selection
//     [--rounds 5] [--duration 10] [--warmup 5] [--connections 32]
//     [--rate N]                              # open loop only, at N req/s
//     [--open 0.5]                            # open-loop rate as a fraction
//                                             # of the slowest config's closed rps (0 = skip)
//     [--server-cpus 0-3] [--load-cpus 4-5] [--seed 1]
//
// One server process per (group, configuration, round); configurations
// are interleaved in a shuffled order each round. Per workload: an oracle
// preflight, a cold sample (first requests after start), a warm-up run,
// then the measured run with process-tree CPU, perf instructions and
// memory sampled over exactly that window. Every response is checked by
// the load generator; a run with failed checks is marked invalid.
import { mkdirSync, writeFileSync, appendFileSync } from "node:fs";
import { join, resolve } from "node:path";
import * as L from "./lib.mjs";
import { groups as allGroups } from "./workloads.mjs";

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) {
  if (!argv[i].startsWith("--")) continue;
  const k = argv[i].slice(2);
  args[k] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
}
const ROOT = resolve(new URL("../..", import.meta.url).pathname);
const out = resolve(args.out ?? join(ROOT, "benchmarks/results/phase3a", new Date().toISOString().replace(/[:.]/g, "")));
mkdirSync(out, { recursive: true });
const node = process.execPath;
const loadgenBin = join(ROOT, "benchmarks/loadgen/target/release/nextrs-loadgen");
const serverCpus = args["server-cpus"] ?? "0-3";
const loadCpus = args["load-cpus"] ?? "4-5";
const rounds = Number(args.rounds ?? 5);
const duration = Number(args.duration ?? 10);
const warmup = Number(args.warmup ?? 5);
const openFraction = Number(args.open ?? 0.5);
const r = L.rng(Number(args.seed ?? 1));

// --args / --args-b: extra `nextrs start` arguments per build (whitespace-
// separated), e.g. V8 flags for a flag-only experiment with one binary.
const splitArgs = (a) => (typeof a === "string" ? a.match(/(?:[^\s"]+|"[^"]*")+/g).map((x) => x.replace(/^"|"$/g, "")) : []);
// --env / --env-b: extra environment per build, "K=V,K2=V2" (allocator
// tunables such as MALLOC_ARENA_MAX or GLIBC_TUNABLES).
const parseEnv = (e) => Object.fromEntries(typeof e === "string" ? e.split(",").map((kv) => [kv.slice(0, kv.indexOf("=")), kv.slice(kv.indexOf("=") + 1)]) : []);
const builds = [{ label: args.label ?? "nextrs", bin: resolve(args.nextrs ?? join(ROOT, "target/release/nextrs")), extra: splitArgs(args.args), env: parseEnv(args.env) }];
if (args["nextrs-b"]) builds.push({ label: args["label-b"] ?? "nextrs-b", bin: resolve(args["nextrs-b"]), extra: splitArgs(args["args-b"]), env: parseEnv(args["env-b"]) });
const isolates = String(args.isolates ?? "1,2,4").split(",").map(Number);
const configs = [];
if (!args["no-next"]) configs.push({ id: "next-start", kind: "next" });
for (const b of builds) for (const n of isolates) configs.push({ id: `${b.label}-i${n}`, kind: "nextrs", bin: b.bin, isolates: n, extra: b.extra, env: b.env });

const selectedGroups = args.groups ? new Set(String(args.groups).split(",")) : null;
const only = args.only ? new Set(String(args.only).split(",")) : null;
const groups = allGroups
  .filter((g) => !selectedGroups || selectedGroups.has(g.id))
  .map((g) => ({ ...g, workloads: g.workloads.filter((w) => !only || only.has(w.id)) }))
  .filter((g) => g.workloads.length);

const rowsFile = join(out, "rows.jsonl");
const log = (...m) => console.log(new Date().toISOString().slice(11, 19), ...m);
const env = L.environment({ node, nextrs: builds[0].bin, root: ROOT, loadgenBin, serverCpus, loadCpus });
env.builds = builds.map((b) => ({ ...b, sha256: L.environment({ node, nextrs: b.bin, root: ROOT, loadgenBin, serverCpus, loadCpus }).nextrs_binary_sha256 }));
env.parameters = { rounds, duration, warmup, openFraction, configs: configs.map((c) => c.id), seed: Number(args.seed ?? 1) };
env.fixtures = {};
// Paths inside the repository are recorded relative to it (results are
// published; absolute paths would only describe this machine).
const repoRelative = (value) => JSON.parse(JSON.stringify(value).replaceAll(`${ROOT}/`, "").replaceAll(ROOT, "."));
const writeEnvironment = () => writeFileSync(join(out, "environment.json"), JSON.stringify(repoRelative(env), null, 2));
writeEnvironment();

let portBase = 6100;
const ports = () => ({ port: portBase++, metricsPort: portBase++ });

async function withServer(group, config, fn) {
  const { port, metricsPort } = ports();
  const ctx = { group, config, port, root: ROOT, node, scratch: join(out, "specs") };
  mkdirSync(ctx.scratch, { recursive: true });
  const services = group.services ? await group.services(ctx) : null;
  ctx.services = services;
  const s = await L.startServer({
    kind: config.kind,
    node,
    nextrs: config.bin,
    project: join(ROOT, group.fixture),
    dist: group.dist ?? ".next",
    port,
    metricsPort,
    isolates: config.isolates,
    cpus: serverCpus,
    env: { ...(group.env?.(ctx) ?? {}), ...(config.env ?? {}) },
    extraArgs: [...(group.nextrsArgs?.(ctx) ?? []), ...(config.extra ?? [])],
    readyPath: group.readyPath ?? "/",
  });
  ctx.server = s;
  try {
    return await fn(ctx, s);
  } finally {
    await L.stopServer(s);
    await services?.stop?.();
  }
}

async function measure(ctx, s, w, mode, rate) {
  const specPath = join(ctx.scratch, `${ctx.group.id}.${w.id}.${ctx.config.id}.json`);
  const spec = typeof w.spec === "function" ? await w.spec(ctx) : w.spec;
  writeFileSync(specPath, JSON.stringify(spec));
  const target = `127.0.0.1:${s.port}`;
  const connections = w.connections ?? Number(args.connections ?? 32);
  const lg = (o) => L.loadgen(loadgenBin, { cpus: loadCpus, target, spec: specPath, connections, ...o });

  // Oracle preflight: every request in the spec at least twice, checked.
  const pre = await lg({ connections: 1, requests: spec.requests.length * 2, duration: 60, warmup: 0 });
  const preOk = pre.ok === pre.completed && pre.completed > 0 && pre.io_errors === 0;
  // Cold: the first requests after this workload starts (closed, 1 connection).
  const cold = mode === "closed" && ctx.firstOfServer ? await lg({ connections: 1, requests: 20, duration: 60, warmup: 0 }) : null;
  await lg({ rate, duration: warmup, warmup: 0 });
  await w.beforeMeasure?.(ctx);
  const m0 = await L.metrics(s.metricsPort);
  const c0 = L.sample(s.pid);
  const stopMem = L.memorySampler(s.pid);
  const stopPerf = L.perfStat(s.pid);
  const res = await lg({ rate, duration, warmup: 0 });
  const perf = await stopPerf();
  const mem = stopMem();
  const c1 = L.sample(s.pid);
  const m1 = await L.metrics(s.metricsPort);
  await L.sleep(2000);
  const steady = L.sample(s.pid);
  const post = w.postcheck ? await w.postcheck(ctx, res) : null;
  const cpu = c1.cpu_user_s + c1.cpu_sys_s - (c0.cpu_user_s + c0.cpu_sys_s);
  const n = res.completed || 1;
  const exec = executorDelta(m0, m1);
  const valid = preOk && res.ok === res.completed && res.io_errors === 0 && res.timeouts === 0 && (!post || post.ok);
  return {
    group: ctx.group.id, workload: w.id, label: w.label, config: ctx.config.id, kind: ctx.config.kind, isolates: ctx.config.isolates ?? null,
    round: ctx.round, mode, offered_rate: rate, connections, valid,
    preflight: { ok: preOk, completed: pre.completed, failures: pre.check_failure_samples },
    cold: cold && { p50_ms: cold.service.p50_ms, max_ms: cold.service.max_ms, ok: cold.ok === cold.completed },
    rps: res.achieved_rps, completed: res.completed,
    latency: res.latency, service: res.service, ttfb: res.ttfb,
    by_kind: Object.keys(res.by_kind ?? {}).length ? res.by_kind : undefined,
    errors: { status: res.status_mismatch, body: res.body_mismatch, io: res.io_errors, timeouts: res.timeouts, samples: res.check_failure_samples },
    late_starts: res.late_starts,
    loadgen_cpu_s: res.loadgen_cpu_s,
    // The generator (2 threads) used >= 70% of its CPUs: it may have limited
    // the run, so rps is a lower bound (CPU per request stays valid).
    load_limited: res.loadgen_cpu_s / res.duration_s >= 0.7 * 2,
    cpu_ms_per_req: (cpu * 1000) / n, cpu_user_s: c1.cpu_user_s - c0.cpu_user_s, cpu_sys_s: c1.cpu_sys_s - c0.cpu_sys_s,
    cpu_utilization: cpu / res.duration_s,
    instructions_per_req: perf.instructions ? perf.instructions / n : null,
    cycles_per_req: perf.cycles ? perf.cycles / n : null,
    mem_peak: mem.peak, mem_end: { rss_mib: c1.rss_mib, anon_mib: c1.anon_mib, file_mib: c1.file_mib, swap_mib: c1.swap_mib, threads: c1.threads },
    mem_steady: { rss_mib: steady.rss_mib, anon_mib: steady.anon_mib, swap_mib: steady.swap_mib },
    executor: exec,
    postcheck: post,
    startup_ms: s.startup_ms,
  };
}

function executorDelta(a, b) {
  if (!a || !b) return null;
  const d = (k) => (typeof b[k] === "number" && typeof a[k] === "number" ? b[k] - a[k] : null);
  const reqs = d("requests_completed");
  const mib = (x) => +(x / 1048576).toFixed(1);
  const isolates = Array.isArray(b.executors) ? b.executors.map((e) => ({ heap_used_after_gc_mib: mib(e.heap_used_after_gc_bytes), heap_total_mib: mib(e.heap_total_bytes), external_mib: mib(e.external_bytes), full_gcs: e.full_gcs, restarts: e.restarts })) : null;
  return {
    requests: reqs,
    queue_wait_ms_per_req: reqs ? (d("queue_wait_seconds_total") * 1000) / reqs : null,
    execution_ms_per_req: reqs ? (d("execution_seconds_total") * 1000) / reqs : null,
    queue_waits: d("queue_waits"),
    rejected: ["rejected_overloaded", "rejected_queue_timeout", "rejected_shutting_down", "rejected_unavailable"].reduce((a, k) => a + (d(k) ?? 0), 0),
    cancelled: d("responses_cancelled"),
    handler_errors: d("handler_errors"),
    cache_hits: b.cache && a.cache ? b.cache.hits - a.cache.hits : null,
    cache_misses: b.cache && a.cache ? b.cache.misses - a.cache.misses : null,
    malloc_end: b.memory?.malloc ?? null,
    isolates,
    raw_end: b,
  };
}

async function phase(mode, rateFor) {
  for (let round = 1; round <= rounds; round++) {
    for (const group of groups) {
      env.fixtures[group.id] ??= L.fixtureIdentity(join(ROOT, group.fixture), group.dist ?? ".next");
      for (const config of L.shuffle(configs, r)) {
        if (group.nextrsOnly && config.kind === "next") continue;
        log(mode, `round ${round}/${rounds}`, group.id, config.id);
        try {
          await withServer(group, config, async (ctx, s) => {
            ctx.round = round;
            ctx.firstOfServer = true;
            await group.setup?.(ctx);
            for (const w of group.workloads) {
              if (w.nextrsOnly && config.kind === "next") continue;
              const rate = rateFor ? rateFor(group.id, w.id) : 0;
              if (rateFor && !rate) continue;
              const row = await measure(ctx, s, w, mode, rate);
              ctx.firstOfServer = false;
              appendFileSync(rowsFile, JSON.stringify(row) + "\n");
              log(`  ${w.id.padEnd(22)} ${row.valid ? "ok " : "INVALID"} ${row.rps.toFixed(0).padStart(6)} rps p95 ${row.latency.p95_ms.toFixed(2)} ms cpu ${row.cpu_ms_per_req.toFixed(3)} ms/req rss ${row.mem_peak.rss_mib} MiB${row.load_limited ? " (loadgen-limited)" : ""}`);
              if (!row.valid) log("   ", JSON.stringify({ pre: row.preflight, errors: row.errors, post: row.postcheck }).slice(0, 600));
            }
          });
        } catch (e) {
          log("  FAILED", e.message.slice(0, 800));
          appendFileSync(rowsFile, JSON.stringify({ group: group.id, config: config.id, round, mode, failed: e.message.slice(0, 2000) }) + "\n");
        }
      }
    }
  }
  writeEnvironment();
}

if (args.rate) {
  // Open loop only, at a fixed offered rate for every workload.
  await phase("open", () => Number(args.rate));
} else {
await phase("closed", null);
if (openFraction > 0) {
  // Open-loop rate per workload: a fraction of the slowest configuration's
  // median closed-loop throughput, so every configuration is measured at
  // the same offered load, below saturation.
  const { readFileSync } = await import("node:fs");
  const rows = readFileSync(rowsFile, "utf8").trim().split("\n").map((l) => JSON.parse(l)).filter((x) => x.mode === "closed" && x.valid);
  const rates = {};
  for (const g of groups) for (const w of g.workloads) {
    const per = configs.map((c) => L.median(rows.filter((x) => x.group === g.id && x.workload === w.id && x.config === c.id).map((x) => x.rps))).filter((x) => x);
    if (per.length) rates[`${g.id}/${w.id}`] = Math.max(1, Math.floor(Math.min(...per) * openFraction));
  }
  writeFileSync(join(out, "open-rates.json"), JSON.stringify(rates, null, 2));
  await phase("open", (g, w) => rates[`${g}/${w}`]);
}
}
log("done", out.replace(`${ROOT}/`, ""));
