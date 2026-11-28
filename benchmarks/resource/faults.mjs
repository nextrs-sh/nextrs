// Container fault workloads (`run.mjs profile --workload faults-*`), as
// specified in RE-4-LINUX-BENCHMARK-RUNBOOK §6. Each fault is a list of
// cases; each case gets a fresh storefront container (the run's memory and
// CPU limits), the probe's 1-second sampler, a settle period, its steps
// (fault-client.mjs in the tools container), a recovery window of health
// probes, and a classification from what the harness can observe:
//
//   process-fatal  the container exited, was OOM-killed (docker State),
//                  restarted, its main process changed, or it never
//                  answered its readiness route again (wedged);
//   contained      the process lived, but an isolate was replaced
//                  (/metrics.json generation or restarts), or requests were
//                  refused or failed (non-2xx, connection errors, RE-4
//                  refusal counters);
//   prevented      the process lived and every fault request succeeded.
//
// Node.js has no /metrics.json: its classification uses HTTP and docker
// only. Apps: `velasia` (the Series 1 storefront images) and `fault`
// (benchmarks/resource/fault-app, images nextrs-fault:*).
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import * as L from "../series1/lib.mjs";
import * as H from "./harness.mjs";
import { faultImage } from "./validate.mjs";

const MiB = 2 ** 20;
const CLIENT = "/tmp/fault-client.mjs";
const READY = "/api/internal/health/ready";

/** The fault workloads. `o`: run.mjs options (the --fault-* overrides). */
export function faultWorkloads(o = {}, config = "256m:1") {
  const limitMiB = (H.parseConfig(config)?.bytes ?? 256 * MiB) / MiB;
  const ints = (k, d) => H.list(o[k], d).map(Number);
  const chunk = H.num(o["fault-chunk-mib"], 16);
  const cap = H.num(o["fault-cap-mib"], 2 * limitMiB);
  const duration = H.num(o["fault-duration"], undefined);
  return {
    "faults-native-memory": {
      app: "fault",
      description: `${chunk} MiB native buffers (Buffer.alloc) held by the isolate, one request at a time, up to ${cap} MiB (2 × the limit), then released`,
      record: "OOM kill, RangeError (with --js-external-memory-limit), or admission refusals",
      cases: [{
        name: `held-${chunk}mib`,
        steps: [
          { name: "allocate", role: "fault", client: ["requests", "--path", `/api/fault/native?mib=${chunk}&hold=1`, "--count", String(Math.ceil(cap / chunk)), "--concurrency", "1", "--stop-after-fails", "3", "--timeout", "60"], seconds: 60 },
          { name: "release", role: "probe", client: ["requests", "--path", "/api/fault/native?release=1", "--count", "1", "--timeout", "30"], seconds: 5 },
          { name: "after-release", idle: 30 },
        ],
      }],
    },
    "faults-js-allocation": {
      app: "fault",
      description: "one large JavaScript allocation per container (heap-faults.js modes), by size",
      record: "replaced / aborted / OOM-killed, by size",
      cases: H.list(o["fault-modes"], "heap-single,heap-string").flatMap((mode) => ints("fault-sizes", "64,96,128,192,256").map((mib) => ({
        name: `${mode}-${mib}mib`,
        steps: [{ name: "allocate", role: "fault", client: ["requests", "--path", `/api/fault/heap?mode=${mode}&mib=${mib}`, "--count", "1", "--timeout", "120"], seconds: 30 }],
      }))),
    },
    "faults-concurrent-ssr": {
      app: "velasia",
      description: `closed loop on the SSR search at ${H.list(o["fault-levels"], "16,32,64,128").join(", ")} connections, ${duration ?? 60} s each, a fresh container per level`,
      record: "peak memory, restarts, 503 share",
      cases: ints("fault-levels", "16,32,64,128").map((c) => ({
        name: `ssr-c${c}`,
        steps: [{ name: "ssr", role: "fault", client: ["closed", "--spec", `${H.SPECS}/ssr-gzip.json`, "--connections", String(c), "--duration", String(duration ?? 60), "--timeout", "30", "--probe", READY, "--probe-interval", "5"], seconds: duration ?? 60, metrics: true }],
      })),
    },
    "faults-large-uploads": {
      app: "fault",
      description: `concurrent ${H.num(o["fault-size-mib"], 8)} MiB POSTs (streamed and buffered route handlers) at ${H.list(o["fault-levels"], "8,32").join(", ")} at once`,
      record: "peak memory, 413/503",
      cases: H.list(o["fault-modes"], "stream,buffer").flatMap((mode) => ints("fault-levels", "8,32").map((c) => ({
        name: `upload-${mode}-c${c}`,
        steps: [{ name: "upload", role: "fault", client: ["upload", "--path", `/api/fault/upload${mode === "buffer" ? "?buffer=1" : ""}`, "--size-mib", String(H.num(o["fault-size-mib"], 8)), "--concurrency", String(c), "--count", String(3 * c), "--timeout", "120"], seconds: 60, metrics: true }],
      }))),
    },
    "faults-slow-streams": {
      app: "fault",
      description: `${H.list(o["fault-levels"], "16,64").join(" or ")} clients reading a 64 MiB streamed response at ${H.num(o["fault-rate-bps"], 1024)} B/s for ${duration ?? 120} s; health probed meanwhile`,
      record: "memory per stalled stream",
      cases: ints("fault-levels", "16,64").map((n) => ({
        name: `slow-${n}`,
        streams: n,
        steps: [{ name: "stall", role: "fault", client: ["slow-read", "--path", "/api/fault/stream?size=67108864", "--streams", String(n), "--rate-bps", String(H.num(o["fault-rate-bps"], 1024)), "--duration", String(duration ?? 120), "--probe", READY, "--probe-interval", "5"], seconds: duration ?? 120, metrics: true }],
      })),
    },
    "faults-queue-saturation": {
      app: "velasia",
      description: `mixed workload: closed loop 30 s (saturation), open loop at ${H.num(o["fault-factor"], 2)} × that for ${duration ?? 60} s, 60 s idle, then 0.5 × for 30 s (recovered?)`,
      record: "queue length and bytes, 503 codes, recovery",
      cases: [{
        name: `open-${H.num(o["fault-factor"], 2)}x`,
        steps: [
          { name: "saturate", role: "measure", client: ["closed", "--spec", `${H.SPECS}/mixed-gzip.json`, "--connections", "16", "--duration", "30", "--timeout", "30"], seconds: 30, perIsolate: true },
          { name: "overload", role: "fault", client: ["open", "--spec", `${H.SPECS}/mixed-gzip.json`, "--rate", { from: "saturate", field: "ok_rps", factor: H.num(o["fault-factor"], 2), min: 5 }, "--duration", String(duration ?? 60), "--max-inflight", "1024", "--timeout", "30", "--probe", READY, "--probe-interval", "5"], seconds: duration ?? 60, metrics: true },
          { name: "idle", idle: 60 },
          { name: "after", role: "recover", client: ["open", "--spec", `${H.SPECS}/mixed-gzip.json`, "--rate", { from: "saturate", field: "ok_rps", factor: 0.5, min: 2 }, "--duration", "30", "--timeout", "30"], seconds: 30 },
        ],
      }],
    },
    "faults-isolates": {
      app: "velasia",
      description: `--isolates ${H.num(o["fault-isolates"], 2)}: mixed 30/s 60 s, closed loop 32 connections 30 s`,
      record: "heap clamp warning, restarts",
      defaultConfigs: ["256m:1"],
      cases: [{
        name: `isolates-${H.num(o["fault-isolates"], 2)}`,
        isolates: H.num(o["fault-isolates"], 2),
        steps: [
          { name: "steady", role: "fault", client: ["open", "--spec", `${H.SPECS}/mixed-gzip.json`, "--rate", "30", "--duration", "60", "--timeout", "30"], seconds: 60, metrics: true },
          { name: "burst", role: "fault", client: ["closed", "--spec", `${H.SPECS}/mixed-gzip.json`, "--connections", "32", "--duration", "30", "--timeout", "30"], seconds: 30, metrics: true },
        ],
      }],
    },
    "faults-pressure-cycles": {
      app: "velasia",
      kind: "load-idle",
      description: `${H.num(o["fault-cycles"], 10)} × (closed-loop mixed 16 connections per isolate ${H.num(o["fault-load-seconds"], 60)} s, idle ${H.num(o["fault-idle-seconds"], 60)} s): a load-idle cycle test`,
      record: "does memory return each cycle",
      cases: [{
        name: `cycles-${H.num(o["fault-cycles"], 10)}`,
        steps: Array.from({ length: H.num(o["fault-cycles"], 10) }, (_, i) => [
          { name: `cycle${i + 1}-pressure`, role: "fault", client: ["closed", "--spec", `${H.SPECS}/mixed-gzip.json`, "--connections", "16", "--duration", String(H.num(o["fault-load-seconds"], 60)), "--timeout", "30"], seconds: H.num(o["fault-load-seconds"], 60), perIsolate: true },
          { name: `cycle${i + 1}-idle`, idle: H.num(o["fault-idle-seconds"], 60), cycleEnd: i + 1 },
        ]).flat(),
      }],
    },
  };
}

export function faultWorkload(name, o, config) {
  const w = faultWorkloads(o, config)[name];
  return w ? { name, kind: w.kind ?? "fault", defaultConfigs: ["256m:1"], ...w } : null;
}

/** The fault-client arguments of a step, with `--rate {from}` resolved (or described, in a plan). */
function clientArgs(step, { isolates, results, metricsUrl, plan = false }) {
  const args = [];
  for (let i = 0; i < step.client.length; i++) {
    const a = step.client[i];
    if (typeof a === "object") {
      if (plan) args.push(`<${a.factor} x ${a.field} of step ${a.from}>`);
      else {
        const prev = results[a.from];
        const v = Math.max(a.min ?? 0, (prev?.[a.field] ?? 0) * a.factor);
        args.push(String(Math.round(v * 10) / 10));
      }
    } else if (step.perIsolate && step.client[i - 1] === "--connections") args.push(String(Number(a) * Math.max(isolates, 1)));
    else args.push(a);
  }
  const [mode, ...rest] = args;
  const extra = step.metrics && metricsUrl ? ["--poll-metrics", metricsUrl, "--poll-interval", "1"] : [];
  const limit = Math.ceil((step.seconds ?? 60) + 120);
  return ["exec", L.TOOLS, "timeout", "-s", "KILL", String(limit), "node", CLIENT, mode, "--base", `http://${L.SF}:8080`, ...rest, ...extra];
}

const sum = (xs) => xs.reduce((a, b) => a + (Number(b) || 0), 0);
function counters(m) {
  if (!m) return null;
  const ex = m.executors ?? [];
  return {
    restarts: sum(ex.map((e) => e.restarts)),
    generations: sum(ex.map((e) => e.generation)),
    heapLimitEpisodes: ex.filter((e) => e.last_heap_limit).length,
    rejectedMemoryPressure: m.rejected_memory_pressure ?? 0,
    rejectedQueue: (m.rejected_queue_full ?? 0) + (m.rejected_queue_timeout ?? 0) + (m.rejected_overloaded ?? 0),
    rejectedUnavailable: m.rejected_unavailable ?? 0,
    admissionRejected: sum((m.admission?.rejected ?? []).map((r) => r[1])),
    admissionDeferred: m.admission?.deferred ?? 0,
    externalRefused: m.external_memory?.refused_total ?? m.external_memory?.refused ?? 0,
    nativeBufferRefused: m.native_buffers?.refused_total ?? 0,
    requestTimeouts: m.request_timeouts ?? 0,
  };
}

/** prevented / contained / process-fatal, with the observations behind it. */
export function classify(run) {
  const f = run.final;
  const fatal = [];
  if (run.error) fatal.push(run.error);
  if (!f) fatal.push("container gone (docker inspect failed)");
  else {
    if (f.oomKilled) fatal.push("OOM-killed (State.OOMKilled)");
    if (!f.running) fatal.push(`exited (code ${f.exitCode})`);
    if (f.restarts > 0) fatal.push(`container restarted ${f.restarts} time(s)`);
    if (f.running && run.initial?.pid && f.pid && f.pid !== run.initial.pid) fatal.push("main process replaced");
  }
  if (f?.running && run.recovery && run.recovery.okCount === 0) fatal.push(`alive but readiness never answered 200 in ${run.recovery.count} probes (wedged)`);
  if (fatal.length) {
    const detail = f?.oomKilled ? "OOM-killed" : f && !f.running ? "aborted"
      : fatal.some((r) => r.startsWith("main process")) ? "process replaced"
        : run.error ? "not ready" : "wedged";
    return { outcome: "process-fatal", detail, reasons: fatal };
  }
  const contained = [];
  const a = counters(run.initial?.metrics);
  const b = counters(run.finalMetrics);
  if (a && b) {
    for (const k of Object.keys(b)) {
      const d = (b[k] ?? 0) - (a[k] ?? 0);
      if (d > 0 && k !== "admissionDeferred") contained.push(`${k} +${d}`);
    }
  }
  if (run.lastCgroup?.oom_kill > 0) contained.push(`cgroup oom_kill ${run.lastCgroup.oom_kill} (main process survived)`);
  let caught = 0;
  for (const s of run.steps.filter((x) => x.role === "fault" && x.client)) {
    const c = s.client;
    const bad = Object.entries(c.statuses ?? {}).filter(([k]) => !(Number(k) >= 200 && Number(k) < 300));
    for (const [status, n] of bad) contained.push(`${s.name}: ${n} × ${status === "0" ? "no response" : `HTTP ${status}`}`);
    for (const [e, n] of Object.entries(c.errors ?? {})) contained.push(`${s.name}: ${n} × ${e}`);
    caught += (c.results ?? []).filter((r) => /"caught"/.test(r.body ?? "")).length;
    if (c.clientError) contained.push(`${s.name}: client error ${c.clientError}`);
  }
  if (caught) contained.push(`${caught} allocation(s) refused inside JavaScript (RangeError caught by the route)`);
  if (contained.length) {
    const replaced = a && b && (b.restarts > a.restarts || b.generations > a.generations);
    return { outcome: "contained", detail: replaced ? "isolate replaced" : "request refused or failed", reasons: contained };
  }
  return { outcome: "prevented", detail: "no failure observed", reasons: [] };
}

/** Figures each fault reports, from the run and its samples. */
function derive(fault, c, run, samples) {
  const d = {};
  const w = (s) => samples.filter((x) => x.t >= s.started && x.t <= s.ended);
  const max = (xs, k) => (xs.length ? Math.max(...xs.map(k)) / MiB : null);
  d.peakMiB = max(samples, (s) => s.memory_current);
  d.peakAnonMiB = max(samples, (s) => s.stat?.anon ?? 0);
  d.memoryPeakMiB = run.lastCgroup?.["memory.peak"] ? run.lastCgroup["memory.peak"] / MiB : null;
  const idle = run.initial?.cgroup?.["memory.current"];
  const fault0 = run.steps.find((s) => s.role === "fault" && s.client);
  if (fault0?.client) {
    const cl = fault0.client;
    d.completed = cl.completed ?? null;
    d.statuses = cl.statuses ?? null;
    if (cl.completed) d.share503 = (cl.statuses?.["503"] ?? 0) / cl.completed;
  }
  if (fault === "faults-native-memory") {
    const ok = (fault0?.client?.results ?? []).filter((r) => r.status === 200);
    const held = (r) => {
      try {
        return Number(JSON.parse(r.body || "{}").heldMiB ?? 0);
      } catch {
        return 0; // a body cut at 200 bytes
      }
    };
    d.heldMiB = ok.length ? Math.max(...ok.map(held)) : 0;
    d.firstFailure = (fault0?.client?.results ?? []).find((r) => r.status !== 200 || r.error) ?? null;
  }
  if (fault === "faults-js-allocation") {
    const r = fault0?.client?.results?.[0];
    d.response = r ? { status: r.status, body: r.body, error: r.error, ms: r.ms } : null;
  }
  if (fault === "faults-slow-streams" && fault0) {
    const tail = w(fault0).filter((s) => s.t >= fault0.ended - 30_000);
    const med = (xs) => (xs.length ? [...xs].sort((x, y) => x - y)[Math.floor(xs.length / 2)] : null);
    const during = med(tail.map((s) => s.memory_current));
    d.stalledMiB = during && idle ? (during - idle) / MiB : null;
    d.perStreamKiB = d.stalledMiB !== null ? (d.stalledMiB * 1024) / c.streams : null;
    d.health = fault0.client?.series?.probe ?? null;
  }
  if (fault === "faults-queue-saturation") {
    const sat = run.steps.find((s) => s.name === "saturate")?.client;
    const over = run.steps.find((s) => s.name === "overload")?.client;
    const after = run.steps.find((s) => s.name === "after")?.client;
    d.saturationOkRps = sat?.ok_rps ?? null;
    d.offeredRps = over?.offered_rps ?? null;
    const ms = (over?.series?.metrics ?? []).map((x) => x.metrics).filter(Boolean);
    d.maxQueued = ms.length ? Math.max(...ms.map((m) => m.queued ?? 0)) : null;
    d.maxQueuedBytes = ms.length ? Math.max(...ms.map((m) => m.admission?.queued_bytes ?? 0)) : null;
    d.recovered = after ? after.failed === 0 : null;
  }
  if (fault === "faults-isolates") {
    d.heapWarnings = (run.logLines ?? []).filter((l) => /heap|clamp/i.test(l) && /warn/i.test(l)).slice(0, 10);
  }
  if (fault === "faults-pressure-cycles") {
    const ends = run.steps.filter((s) => s.cycleEnd).map((s) => {
      const at = samples.filter((x) => x.t <= s.ended).at(-1);
      return { cycle: s.cycleEnd, currentMiB: s.after?.["memory.current"] ? s.after["memory.current"] / MiB : null, anonMiB: at ? at.stat.anon / MiB : null };
    });
    d.cycleEnds = ends;
    const ref = ends.find((e) => e.cycle === 2)?.anonMiB;
    const later = ends.filter((e) => e.cycle > 2 && Number.isFinite(e.anonMiB));
    d.memoryReturns = ref && later.length ? later.every((e) => e.anonMiB <= ref + Math.max(8, ref * 0.1)) : null;
  }
  return d;
}

/** Plan lines for a dry run: every container command of every case. */
export function planFaults(w, plan, opts, rounds, first) {
  const lines = [];
  let seconds = 0;
  lines.push(`# ${w.name} (${w.kind}, ${w.app} app): ${w.description}`, `# records: ${w.record}`, "");
  lines.push(H.shellLine("docker", ["cp", join(L.HERE, "../resource/fault-client.mjs"), `${L.TOOLS}:${CLIENT}`]));
  for (let round = first; round < first + rounds; round++) {
    const order = round % 2 ? plan : [...plan].reverse();
    const configs = [...new Set(plan.flatMap((p) => p.run.map((r) => r.config)))];
    for (const config of configs) {
      const cw = faultWorkload(w.name, opts, config);
      for (const p of order) {
        if (!p.run.some((r) => r.config === config)) {
          lines.push(`# skip ${p.variant.label} ${config}: ${p.excluded.find((e) => e.config === config)?.reason}`);
          continue;
        }
        const v = p.variant;
        const [memory, cpus] = config.split(":");
        for (const c of cw.cases) {
          const isolates = c.isolates ?? v.isolates ?? 1;
          const file = `${v.label}-${memory}-${cpus}cpu-${c.name}-r${round}`;
          lines.push("", `## ${file}`);
          lines.push(H.shellLine("docker", L.storefrontRunArgs(v.runtime, {
            isolates, extraArgs: [v.args, c.extraArgs].filter(Boolean).join(" "), env: v.env ?? [],
            image: w.app === "fault" ? faultImage(v) : v.image, memory, cpus,
          })));
          lines.push(`#   wait for ${READY} (120 s at most); sampler: ${H.shellLine("docker", H.samplerArgs("<cgroup dir>", "<pid>", `/work/re/${L.SF}-${file}.jsonl`))}`);
          lines.push("#   settle 15 s");
          let s = 15 + 15;
          const metricsUrl = v.runtime === "node" ? null : `http://${L.SF}:9100/metrics.json`;
          for (const step of c.steps) {
            if (step.idle) {
              lines.push(`#   ${step.name}: idle ${step.idle} s`);
              s += step.idle;
              continue;
            }
            lines.push(`#   ${step.name} (${step.role}):`, H.shellLine("docker", clientArgs(step, { isolates, results: {}, metricsUrl, plan: true })));
            s += step.seconds ?? 60;
          }
          lines.push(`#   recovery: ${H.shellLine("docker", ["exec", L.TOOLS, "node", CLIENT, "health", "--base", `http://${L.SF}:8080`, "--path", READY, "--duration", "30", "--interval", "2"])}`);
          lines.push(`#   then docker inspect ${L.SF} (State.Running, OOMKilled, ExitCode, Pid, RestartCount), /metrics.json, docker logs, docker rm -f ${L.SF}`);
          lines.push("#   classify: process-fatal | contained | prevented");
          s += 30 + 10;
          seconds += s;
        }
      }
    }
  }
  return { lines, seconds };
}

/** Runs one fault workload (profile --workload faults-*). */
export async function runFaults({ w, plan, opts, rounds, first, out }) {
  L.docker(["cp", join(L.HERE, "../resource/fault-client.mjs"), `${L.TOOLS}:${CLIENT}`]);
  const configs = [...new Set(plan.flatMap((p) => p.run.map((r) => r.config)))];
  for (let round = first; round < first + rounds; round++) {
    const order = round % 2 ? plan : [...plan].reverse();
    for (const config of configs) {
      const cw = faultWorkload(w.name, opts, config);
      for (const p of order) {
        if (!p.run.some((r) => r.config === config)) continue;
        for (const c of cw.cases) await runCase(cw, c, p.variant, config, round, out);
      }
    }
  }
}

async function runCase(w, c, v, config, round, out) {
  const [memory, cpus] = config.split(":");
  const isolates = c.isolates ?? v.isolates ?? 1;
  const file = `${v.label}-${memory}-${cpus}cpu-${c.name}-r${round}`;
  const run = { kind: "fault", fault: w.name, app: w.app, case: c.name, variant: v, memory, cpus, round, isolates, steps: [], marks: {} };
  const rawMetrics = [];
  const rawClient = [];
  H.log(file);
  try {
    run.readyMs = (await L.storefrontUp(v.runtime, {
      isolates, extraArgs: [v.args, c.extraArgs].filter(Boolean).join(" "), env: v.env ?? [],
      image: w.app === "fault" ? faultImage(v) : v.image, memory, cpus, readyTimeoutMs: 120_000,
    })).readyMs;
  } catch (error) {
    run.error = `not ready: ${String(error.message).slice(0, 300)}`;
  }
  const info = H.inspect(L.SF);
  const samples = `/work/re/${L.SF}-${file}.jsonl`;
  if (info) {
    L.docker(["exec", H.PROBE, "rm", "-f", samples], { allowFail: true });
    L.docker(H.samplerArgs(H.cgDir(info.id), info.state.Pid, samples));
  }
  const metricsUrl = v.runtime === "node" ? null : `http://${L.SF}:9100/metrics.json`;
  if (!run.error && info) {
    await L.sleep(15_000);
    run.initial = { pid: info.state.Pid, cgroup: H.cgroup(info.id), metrics: H.metrics(v.runtime, rawMetrics, "initial") };
    const results = {};
    for (const step of c.steps) {
      const st = H.inspect(L.SF);
      if (!st?.state.Running) {
        run.steps.push({ name: step.name, role: step.role, skipped: "container not running" });
        continue;
      }
      if (step.idle) {
        const started = Date.now();
        await L.sleep(step.idle * 1000);
        run.steps.push({ name: step.name, idle: step.idle, cycleEnd: step.cycleEnd, started, ended: Date.now(), after: H.cgroup(info.id), metricsAfter: H.metrics(v.runtime, rawMetrics, `${step.name}:after`) });
        continue;
      }
      const args = clientArgs(step, { isolates, results, metricsUrl });
      const before = H.cgroup(info.id);
      const mBefore = H.metrics(v.runtime, rawMetrics, `${step.name}:before`);
      const started = Date.now();
      const r = L.docker(args, { allowFail: true });
      const ended = Date.now();
      let client = null;
      try {
        client = JSON.parse(r.stdout);
      } catch {
        client = { clientError: `exit ${r.status}: ${(r.stderr || r.stdout).slice(0, 300)}` };
      }
      rawClient.push({ step: step.name, args, client });
      results[step.name] = client;
      // The run file keeps the summary; per-request arrays stay in *.client.jsonl.
      const { latencies_ms, statuses_seq, perStream, series, ...summary } = client;
      run.steps.push({
        name: step.name, role: step.role, args, started, ended, before, after: H.cgroup(info.id),
        metricsBefore: mBefore, metricsAfter: H.metrics(v.runtime, rawMetrics, `${step.name}:after`),
        client: { ...summary, results: (summary.results ?? []).map(({ i, status, ms, body, error }) => ({ i, status, ms, body: body?.slice(0, 200), error })) },
      });
      H.log(`  ${step.name}: ${client.completed ?? client.streams ?? "-"} done, statuses ${JSON.stringify(client.statuses ?? {})}${client.clientError ? `, ${client.clientError}` : ""}`);
    }
    // Recovery: does the storefront answer its readiness route again?
    if (H.inspect(L.SF)?.state.Running) {
      const r = L.docker(["exec", L.TOOLS, "timeout", "-s", "KILL", "90", "node", CLIENT, "health", "--base", `http://${L.SF}:8080`, "--path", READY, "--duration", "30", "--interval", "2"], { allowFail: true });
      try {
        run.recovery = JSON.parse(r.stdout);
      } catch {
        run.recovery = { okCount: 0, count: 0, clientError: r.stderr.slice(0, 300) };
      }
    }
  }
  const end = H.inspect(L.SF);
  run.final = end && { running: end.state.Running, oomKilled: end.state.OOMKilled, exitCode: end.state.ExitCode, pid: end.state.Pid, restarts: end.restarts, error: end.state.Error };
  run.lastCgroup = info && H.cgroup(info.id);
  run.finalMetrics = end?.state.Running ? H.metrics(v.runtime, rawMetrics, "final") : null;
  if (!run.finalMetrics && rawMetrics.length) {
    // The process died: its last metrics reading is the best evidence left.
    const last = rawMetrics.at(-1);
    run.finalMetrics = { ...last.metrics, native_buffers: last.native_buffers, stale: true };
  }
  if (end) {
    const logs = L.docker(["logs", L.SF], { allowFail: true });
    writeFileSync(join(out, `${file}.log`), `${logs.stdout}${logs.stderr}`);
    run.logLines = `${logs.stdout}${logs.stderr}`.split("\n").filter((l) => /warn|error|heap|oom|abort|fatal/i.test(l)).slice(-50);
  }
  L.remove(L.SF);
  await L.sleep(1500);
  const jsonl = info ? L.docker(["exec", H.PROBE, "cat", samples], { allowFail: true }).stdout : "";
  writeFileSync(join(out, `${file}.samples.jsonl`), jsonl);
  writeFileSync(join(out, `${file}.client.jsonl`), rawClient.map((x) => JSON.stringify(x)).join("\n") + (rawClient.length ? "\n" : ""));
  writeFileSync(join(out, `${file}.metrics.jsonl`), rawMetrics.map((x) => JSON.stringify(x)).join("\n") + (rawMetrics.length ? "\n" : ""));
  const parsed = jsonl.trim().split("\n").filter(Boolean).map((l) => {
    try {
      return JSON.parse(l);
    } catch {
      return null;
    }
  }).filter(Boolean);
  // A bug in the analysis must not lose the run: the raw files are written
  // already, the run file is written in any case.
  try {
    run.classification = classify(run);
    run.derived = derive(w.name, c, run, parsed);
  } catch (error) {
    run.analysisError = String(error.stack ?? error).slice(0, 1000);
    run.classification ??= { outcome: "unclassified", detail: "analysis error", reasons: [run.analysisError.split("\n")[0]] };
  }
  writeFileSync(join(out, `${file}.json`), JSON.stringify(run));
  H.log(`  ${run.classification.outcome} (${run.classification.detail})${run.classification.reasons.length ? `: ${run.classification.reasons.slice(0, 3).join("; ")}` : ""}`);
}
