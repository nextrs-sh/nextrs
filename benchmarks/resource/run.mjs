// Resource-efficiency (RE) measurements: memory, CPU and failures of one
// storefront under container limits, on the Series 1 local stack
// (benchmarks/series1: network, API, tools container, images).
//
//   node benchmarks/series1/run.mjs setup --loadgen PATH --bin base=PATH ...
//   node benchmarks/resource/run.mjs setup
//   node benchmarks/resource/run.mjs discover
//   node benchmarks/resource/run.mjs profile --variants FILE \
//        [--configs 1g:2,512m:1,256m:1,256m:0.5] [--rounds 2] \
//        [--workload full|memory] [--only LABEL,...] [--first-round N] --out DIR
//   node benchmarks/resource/summarize.mjs DIR
//   node benchmarks/resource/run.mjs teardown
//
// A variants file is a JSON array of
//   { "label": "base", "runtime": "base", "isolates": 1, "args": "", "env": [] }
// where `runtime` is `node` (the Next.js standalone server), `rc2` or a
// NextRS binary label (`nextrs-s1:<label>`, built by series1 setup), `args`
// go to `nextrs start` (NEXTRS_EXTRA_ARGS) and `env` are NAME=value
// entries (allocator and runtime experiments). `image` overrides the image.
//
// Each (variant, config, round) gets a fresh container. A sampler in the
// probe container reads the container's cgroup and its main process from
// outside once a second (sampler.mjs), so sampling adds nothing to the
// measured CPU or memory. Rounds alternate the variant order. A run that
// fails (OOM kill, readiness timeout, failed requests) is recorded with its
// reason, never dropped.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import * as L from "../series1/lib.mjs";

// A second lane sets S1_SF, S1_TOOLS, RE_PROBE and the S1_*_CPUSET
// variables (series1/lib.mjs) to run beside the first.
const PROBE = process.env.RE_PROBE ?? "re-probe";
const SPECS = "/work/specs";

const argv = process.argv.slice(2);
const command = argv[0];
const opts = {};
for (let i = 1; i < argv.length; i++) {
  const k = argv[i].replace(/^--/, "");
  const v = argv[i + 1];
  if (v === undefined || v.startsWith("--")) opts[k] = true;
  else (opts[k] = v), i++;
}
const list = (v, d) => String(v ?? d).split(",").filter(Boolean);
const num = (v, d) => (v === undefined ? d : Number(v));
const log = (...a) => console.error(new Date().toISOString().slice(11, 19), ...a);

/** The probe: host PID and cgroup namespaces, on the tools CPUs. */
function setup() {
  if (L.running(PROBE)) return;
  L.remove(PROBE);
  L.docker([
    "run", "-d", "--name", PROBE, "--pid", "host", "--cgroupns", "host",
    "--cap-drop", "ALL", "--cap-add", "SYS_PTRACE", "--cap-add", "DAC_READ_SEARCH",
    "--security-opt", "no-new-privileges:true", "--cpuset-cpus", L.LIMITS.toolsCpuset,
    "-v", "/sys/fs/cgroup:/cg:ro", "-v", `${L.HERE}/../resource:/re:ro`, "-v", "nextrs-s1-work:/work",
    "--user", "root", "--entrypoint", "sleep", L.IMAGES.node, "infinity",
  ]);
  L.docker(["exec", PROBE, "mkdir", "-p", "/work/re"]);
  log("probe up");
}

async function discover() {
  const variant = { runtime: opts.runtime ?? "base" };
  await L.storefrontUp(variant.runtime);
  log(L.toolsNode("discover.mjs", [`http://${L.SF}:8080`, SPECS]).trim());
  L.docker(["cp", join(L.HERE, "../resource/discover-dynamic.mjs"), `${L.TOOLS}:/tmp/discover-dynamic.mjs`]);
  log(L.docker(["exec", L.TOOLS, "node", "/tmp/discover-dynamic.mjs", `http://${L.SF}:8080`, SPECS]).stdout.trim());
  L.remove(L.SF);
}

function inspect(name) {
  const r = L.docker(["inspect", "-f", "{{json .State}} {{.Id}}", name], { allowFail: true });
  if (r.status !== 0) return null;
  const [state, id] = [r.stdout.slice(0, r.stdout.lastIndexOf(" ")), r.stdout.trim().split(" ").pop()];
  return { state: JSON.parse(state), id };
}

/** One reading of the container's cgroup, from the probe. */
function cgroup(id) {
  const dir = `/cg/docker/${id}`;
  const r = L.docker(["exec", PROBE, "sh", "-c",
    `cat ${dir}/cpu.stat; echo memory.current $(cat ${dir}/memory.current); echo memory.peak $(cat ${dir}/memory.peak); cat ${dir}/memory.events`],
  { allowFail: true });
  const o = { t: Date.now() };
  for (const line of r.stdout.split("\n")) {
    const [k, v] = line.trim().split(/\s+/);
    if (k) o[k] = Number(v);
  }
  return r.status === 0 ? o : null;
}

/** Keeps the parts of NextRS's metrics.json these reports use. */
function metrics(runtime) {
  const m = L.nextrsMetrics(runtime);
  if (!m) return null;
  const pick = (o, keys) => Object.fromEntries(keys.filter((k) => o?.[k] !== undefined).map((k) => [k, o[k]]));
  return {
    ...pick(m, ["isolates", "queued", "requests_admitted", "requests_completed", "execution_seconds_total",
      "queue_wait_seconds_total", "queue_waits", "rejected_queue_full", "rejected_queue_timeout",
      "rejected_unavailable", "request_timeouts", "memory", "cache", "compression_cache"]),
    executors: (m.executors ?? []).map((e) => pick(e, ["slot", "state", "generation", "heap_used_bytes",
      "heap_total_bytes", "external_bytes", "heap_used_after_gc_bytes", "full_gcs", "restarts", "failures"])),
    process: m.process,
  };
}

/**
 * The load generator, killed after its duration plus 60 s: it does not
 * give up on a target that died during a fixed-count run (a 128 MiB
 * container killed at startup hung the baseline for hours).
 */
function loadgen(spec, { rate, connections, duration, requests }) {
  const limit = Math.ceil(duration + 60);
  const r = L.docker([
    "exec", L.TOOLS, "timeout", "-s", "KILL", String(limit), "nextrs-loadgen", "--target", `${L.SF}:8080`,
    "--spec", spec, "--rate", String(rate), "--connections", String(connections), "--duration", String(duration),
    "--warmup", "0", "--requests", String(requests), "--threads", "2",
  ], { allowFail: true });
  if (r.status !== 0) throw new Error(`loadgen exit ${r.status}${r.status === 137 ? ` (killed after ${limit} s)` : ""}: ${r.stderr.slice(0, 300)}`);
  return JSON.parse(r.stdout);
}

const WORKLOADS = {
  // Baselines: every workload at a fixed rate below one isolate's
  // saturation on 1 CPU (the 2-CPU smoke run saturated at 60 mixed
  // requests/s), then saturation (closed loop).
  full: [
    { name: "steady-static", spec: "static-gzip.json", rate: 200, connections: 32, duration: 45 },
    { name: "steady-rsc", spec: "rsc-gzip.json", rate: 30, connections: 16, duration: 45 },
    { name: "steady-ssr", spec: "ssr-gzip.json", rate: 5, connections: 16, duration: 45 },
    { name: "steady-api", spec: "api-gzip.json", rate: 15, connections: 16, duration: 45 },
    { name: "steady-mixed", spec: "mixed-gzip.json", rate: 30, connections: 32, duration: 60 },
    { name: "burst-mixed", spec: "mixed-gzip.json", rate: 0, connections: 16, duration: 30, perIsolate: true },
  ],
  // Screening for configuration experiments: the mixed workload only.
  memory: [
    { name: "steady-mixed", spec: "mixed-gzip.json", rate: 30, connections: 32, duration: 60 },
    { name: "burst-mixed", spec: "mixed-gzip.json", rate: 0, connections: 16, duration: 30, perIsolate: true },
  ],
};
const RECOVERY_MARKS = [10, 30, 60, 120];

async function phase(run, id, runtime, p, isolates) {
  const connections = p.perIsolate ? p.connections * Math.max(isolates, 1) : p.connections;
  const before = cgroup(id);
  const mBefore = metrics(runtime);
  const started = Date.now();
  let result;
  try {
    result = loadgen(`${SPECS}/${p.spec}`, { rate: p.rate, connections, duration: p.duration, requests: p.requests ?? 0 });
  } catch (error) {
    result = { error: String(error.message).slice(0, 500) };
  }
  const after = cgroup(id);
  const entry = {
    name: p.name, spec: p.spec, rate: p.rate, connections, duration: p.duration, started, ended: Date.now(),
    before, after, metricsBefore: mBefore, metricsAfter: metrics(runtime),
  };
  if (result.error) {
    entry.error = result.error;
  } else {
    const { check_failure_samples, passed_any_by_index, ...rest } = result;
    entry.result = { ...rest, check_failure_samples: (check_failure_samples ?? []).slice(0, 3) };
    entry.failed = result.status_mismatch + result.body_mismatch + result.io_errors + result.timeouts;
  }
  if (before && after) {
    entry.cpu_usec = after.usage_usec - before.usage_usec;
    entry.cpu_ms_per_ok = result.ok ? entry.cpu_usec / 1000 / result.ok : null;
    entry.throttled_usec = after.throttled_usec - before.throttled_usec;
  }
  run.phases.push(entry);
  const r = entry.result;
  log(`  ${p.name}: ${r ? `${r.ok}/${r.completed} ok, ${r.achieved_rps.toFixed(1)}/s, p50 ${r.latency.p50_ms} p99 ${r.latency.p99_ms} ms, cpu ${entry.cpu_ms_per_ok?.toFixed(3)} ms/ok` : entry.error}` +
    `${entry.failed ? `, FAILED ${entry.failed}` : ""}`);
  return entry;
}

async function one(variant, config, round, workload, out) {
  const [memory, cpus] = config.split(":");
  const isolates = variant.isolates ?? 1;
  const file = `${variant.label}-${memory}-${cpus}cpu-r${round}`;
  const run = { variant, memory, cpus, round, workload, phases: [], marks: {}, contention: { before: L.contention() } };
  log(`${file}`);
  let started;
  try {
    started = Date.now();
    run.readyMs = (await L.storefrontUp(variant.runtime, {
      isolates, extraArgs: variant.args ?? "", env: variant.env ?? [], image: variant.image, memory, cpus,
      readyTimeoutMs: 120_000,
    })).readyMs;
  } catch (error) {
    run.error = `not ready: ${String(error.message).slice(0, 300)}`;
  }
  const info = inspect(L.SF);
  const samples = `/work/re/${L.SF}-${file}.jsonl`;
  if (info) {
    L.docker(["exec", PROBE, "rm", "-f", samples], { allowFail: true });
    L.docker(["exec", "-d", PROBE, "node", "/re/sampler.mjs", `/cg/docker/${info.id}`, String(info.state.Pid), samples, "1000", "5"]);
  }
  run.marks.start = started;
  if (!run.error) {
    await L.sleep(30_000);
    run.marks.idle = Date.now();
    run.idle = { cgroup: cgroup(info.id), metrics: metrics(variant.runtime) };
    // Correctness: every SSR and route-handler request once, body-checked.
    for (const spec of ["ssr-identity.json", "api-identity.json"]) {
      const n = JSON.parse(L.docker(["exec", L.TOOLS, "cat", `${SPECS}/${spec}`]).stdout).requests.length;
      await phase(run, info.id, variant.runtime, { name: `check-${spec.split("-")[0]}`, spec, rate: 0, connections: 1, duration: 60, requests: n }, isolates);
    }
    // Cold: every path of the mixed workload once, one connection.
    const n = JSON.parse(L.docker(["exec", L.TOOLS, "cat", `${SPECS}/mixed-gzip.json`]).stdout).requests.length;
    await phase(run, info.id, variant.runtime, { name: "cold-mixed", spec: "mixed-gzip.json", rate: 0, connections: 1, duration: 120, requests: n }, isolates);
    for (const p of WORKLOADS[workload]) {
      if (!inspect(L.SF)?.state.Running) break;
      await phase(run, info.id, variant.runtime, p, isolates);
    }
    run.marks.loadEnd = Date.now();
    run.recovery = [];
    for (const mark of RECOVERY_MARKS) {
      await L.sleep(Math.max(0, run.marks.loadEnd + mark * 1000 - Date.now()));
      if (!inspect(L.SF)?.state.Running) break;
      run.recovery.push({ after_s: mark, cgroup: cgroup(info.id), metrics: mark === RECOVERY_MARKS.at(-1) ? metrics(variant.runtime) : undefined });
    }
  }
  const end = inspect(L.SF);
  run.final = end && { running: end.state.Running, oomKilled: end.state.OOMKilled, exitCode: end.state.ExitCode, restarts: end.state.RestartCount ?? 0 };
  run.lastCgroup = info && cgroup(info.id);
  if (end && !end.state.Running) run.logTail = L.docker(["logs", "--tail", "20", L.SF], { allowFail: true }).stderr.slice(-4000);
  run.contention.after = L.contention();
  L.remove(L.SF);
  await L.sleep(1500); // the sampler stops when the cgroup disappears
  const jsonl = L.docker(["exec", PROBE, "cat", samples], { allowFail: true }).stdout;
  writeFileSync(join(out, `${file}.samples.jsonl`), jsonl);
  writeFileSync(join(out, `${file}.json`), JSON.stringify(run));
  const oom = run.lastCgroup?.oom_kill ?? null;
  log(`  done: ready ${run.readyMs ?? "-"} ms${run.error ? `, ${run.error}` : ""}${run.final?.oomKilled || oom ? `, OOM (oom_kill ${oom})` : ""}`);
}

async function profile() {
  const variants = JSON.parse(readFileSync(resolve(opts.variants), "utf8"))
    .filter((v) => !opts.only || list(opts.only).includes(v.label));
  const configs = list(opts.configs, "1g:2,512m:1,256m:1,256m:0.5");
  const rounds = num(opts.rounds, 2);
  const workload = opts.workload ?? "full";
  if (!WORKLOADS[workload]) throw new Error(`--workload ${Object.keys(WORKLOADS).join("|")}`);
  const out = resolve(opts.out);
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, `meta-${new Date().toISOString().replace(/[:.]/g, "")}.json`), JSON.stringify({
    command: argv.join(" "), commit: L.gitCommit(), started: new Date().toISOString(), host: L.hostInfo(),
    variants, configs, rounds, workload, phases: WORKLOADS[workload], recoveryMarks: RECOVERY_MARKS,
    images: Object.fromEntries(variants.map((v) => [v.label, L.docker(["image", "inspect", "-f", "{{.Id}}", v.image ?? L.image(v.runtime)], { allowFail: true }).stdout.trim()])),
  }, null, 1));
  const first = num(opts["first-round"], 1);
  for (let round = first; round < first + rounds; round++) {
    const order = round % 2 ? variants : [...variants].reverse();
    for (const config of configs) {
      for (const variant of order) await one(variant, config, round, workload, out);
    }
  }
}

function teardown() {
  L.remove(PROBE);
}

const commands = { setup, discover, profile, teardown };
if (!commands[command]) {
  console.error(`usage: run.mjs ${Object.keys(commands).join("|")} [options]`);
  process.exit(2);
}
await commands[command]();
