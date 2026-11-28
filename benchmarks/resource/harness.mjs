// Shared parts of the resource harness (run.mjs, faults.mjs, validate.mjs):
// option parsing, variants and configurations, the workloads, the probe
// and its sampler, NextRS metrics, the load generator, and the
// correctness gate. Importing this module runs nothing.
import { existsSync, readFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import * as L from "../series1/lib.mjs";

// A second lane sets S1_SF, S1_TOOLS, RE_PROBE and the S1_*_CPUSET
// variables (series1/lib.mjs) to run beside the first.
export const PROBE = process.env.RE_PROBE ?? "re-probe";
export const SPECS = "/work/specs";

// The Node.js release every image and the harness are meant to run
// (Velasia's Dockerfile.node, Dockerfile.nextrs build stage and
// docker/Dockerfile.api pin node:24.18.0-bookworm-slim by digest; the Mac
// RE-1 to RE-4 series used the same). CI pins 22.20.0 for its own jobs.
export const NODE_PIN = "24.18.0";

/** `--k v`, `--flag`; `--bin`, `--commit` and `--allow-config` repeat. */
export function parseArgs(argv) {
  const command = argv[0];
  const opts = { bin: [], commit: [], "allow-config": [] };
  for (let i = 1; i < argv.length; i++) {
    const k = argv[i].replace(/^--/, "");
    const v = argv[i + 1];
    if (Array.isArray(opts[k])) {
      if (v === undefined) throw new Error(`--${k} needs a value`);
      opts[k].push(v);
      i++;
    } else if (v === undefined || v.startsWith("--")) opts[k] = true;
    else (opts[k] = v), i++;
  }
  return { command, opts };
}

/** `label=value` pairs (repeated options) as a Map. */
export function pairs(values, what) {
  const m = new Map();
  for (const p of values) {
    const at = p.indexOf("=");
    if (at <= 0 || at === p.length - 1) throw new Error(`${what}: expected LABEL=VALUE, got "${p}"`);
    m.set(p.slice(0, at), p.slice(at + 1));
  }
  return m;
}

export const list = (v, d) => String(v ?? d).split(",").filter(Boolean);
export const num = (v, d) => (v === undefined ? d : Number(v));
export const log = (...a) => console.error(new Date().toISOString().slice(11, 19), ...a);

/** Shell-quoted command line, for dry runs and plans. */
export function shellLine(cmd, args) {
  const q = (s) => (/^[A-Za-z0-9_@%+=:,./-]+$/.test(s) ? s : `'${String(s).replace(/'/g, "'\\''")}'`);
  return [cmd, ...args].map(q).join(" ");
}

/** `--out` must be absolute: the runbook `cd`s between steps. */
export function requireAbsoluteOut(out) {
  if (!out || out === true) throw new Error("--out DIR is required (an absolute path)");
  if (!isAbsolute(out)) throw new Error(`--out must be an absolute path (got "${out}"): the runbook changes directory between steps`);
  return out;
}

// ---------------------------------------------------------------------------
// Variants and configurations

/** `256m:1` -> { memory: "256m", cpus: "1", bytes, cpusNumber }. */
export function parseConfig(c) {
  const m = String(c).match(/^(\d+(?:\.\d+)?)([kmg])(?::(\d+(?:\.\d+)?))$/i);
  if (!m) return null;
  const unit = { k: 2 ** 10, m: 2 ** 20, g: 2 ** 30 }[m[2].toLowerCase()];
  return { memory: `${m[1]}${m[2].toLowerCase()}`, cpus: m[3], bytes: Number(m[1]) * unit, cpusNumber: Number(m[3]) };
}

/**
 * Whether a configuration matches a pattern: `256m:1` (exact), `256m`
 * or `256m:*` (that memory, any CPUs), `*:0.5` (any memory, those CPUs).
 */
export function configMatches(pattern, config) {
  const [pm, pc = "*"] = String(pattern).toLowerCase().split(":");
  const c = parseConfig(config);
  if (!c) return false;
  const pb = pm === "*" ? null : parseConfig(`${pm}:1`)?.bytes;
  return (pm === "*" || pb === c.bytes) && (pc === "*" || Number(pc) === c.cpusNumber);
}

const LABEL = /^[a-z0-9][a-z0-9._-]*$/;

/**
 * Reads and checks a variants file. Fields:
 *   label, runtime (required); args, env, image, isolates (as before);
 *   configs         allow-list of configuration patterns (default: all);
 *   excludeConfigs  configuration patterns this variant never runs in;
 *   excludeReason   required with excludeConfigs (recorded in meta);
 *   gateAs          label of a gated variant this one is equivalent to
 *                   for correctness (same runtime, env and image; e.g. a
 *                   --trace-gc copy of it);
 *   faultImage      image for the fault app (default nextrs-fault:<runtime>).
 * Returns { variants, problems } (problems: strings; empty when valid).
 */
export function readVariants(file) {
  const problems = [];
  let variants;
  try {
    variants = JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    return { variants: [], problems: [`${file}: ${error.message}`] };
  }
  if (!Array.isArray(variants) || !variants.length) return { variants: [], problems: [`${file}: not a non-empty JSON array`] };
  const known = new Set(["label", "runtime", "args", "env", "image", "isolates", "configs", "excludeConfigs", "excludeReason", "gateAs", "faultImage", "note"]);
  const labels = new Set();
  for (const [i, v] of variants.entries()) {
    const at = `variant ${i} (${v?.label ?? "?"})`;
    if (typeof v !== "object" || v === null) {
      problems.push(`${at}: not an object`);
      continue;
    }
    for (const k of Object.keys(v)) if (!known.has(k)) problems.push(`${at}: unknown field "${k}"`);
    if (typeof v.label !== "string" || !LABEL.test(v.label)) problems.push(`${at}: label must match ${LABEL}`);
    else if (labels.has(v.label)) problems.push(`${at}: duplicate label`);
    else labels.add(v.label);
    if (typeof v.runtime !== "string" || !LABEL.test(v.runtime)) problems.push(`${at}: runtime must match ${LABEL}`);
    if (v.args !== undefined && typeof v.args !== "string") problems.push(`${at}: args must be a string`);
    if (v.args && v.runtime === "node") problems.push(`${at}: args apply to NextRS only (runtime node ignores them)`);
    if (v.env !== undefined && (!Array.isArray(v.env) || v.env.some((e) => typeof e !== "string" || !/^[A-Za-z_][A-Za-z0-9_]*=/.test(e)))) {
      problems.push(`${at}: env must be an array of NAME=value strings`);
    }
    if (v.isolates !== undefined && !(Number.isInteger(v.isolates) && v.isolates >= 1)) problems.push(`${at}: isolates must be an integer >= 1`);
    for (const k of ["configs", "excludeConfigs"]) {
      if (v[k] === undefined) continue;
      if (!Array.isArray(v[k]) || v[k].some((p) => typeof p !== "string" || !/^(\*|\d+(\.\d+)?[kmg])(:(\*|\d+(\.\d+)?))?$/i.test(p))) {
        problems.push(`${at}: ${k} must be an array of patterns like 256m:1, 256m, *:0.5`);
      }
    }
    if (v.excludeConfigs?.length && !(typeof v.excludeReason === "string" && v.excludeReason.trim())) {
      problems.push(`${at}: excludeConfigs needs an excludeReason`);
    }
  }
  for (const v of variants) {
    if (v.gateAs === undefined) continue;
    const ref = variants.find((x) => x.label === v.gateAs);
    if (ref && (ref.runtime !== v.runtime || (ref.image ?? null) !== (v.image ?? null) || JSON.stringify(ref.env ?? []) !== JSON.stringify(v.env ?? []))) {
      problems.push(`${v.label}: gateAs ${v.gateAs} must have the same runtime, image and env`);
    }
  }
  return { variants, problems };
}

/**
 * The configurations a variant runs in, from the requested list: its
 * `configs` allow-list, minus `excludeConfigs`, unless `--allow-config
 * LABEL=PATTERN` (with `--allow-reason`) lifts an exclusion.
 */
export function variantConfigs(variant, configs, allow = new Map()) {
  const run = [];
  const excluded = [];
  for (const config of configs) {
    if (variant.configs && !variant.configs.some((p) => configMatches(p, config))) {
      excluded.push({ config, reason: `not in the variant's configs (${variant.configs.join(", ")})` });
      continue;
    }
    const hit = (variant.excludeConfigs ?? []).find((p) => configMatches(p, config));
    const lifted = allow.has(variant.label) && configMatches(allow.get(variant.label), config);
    if (hit && !lifted) {
      excluded.push({ config, reason: variant.excludeReason, pattern: hit });
      continue;
    }
    run.push({ config, lifted: hit && lifted ? hit : null });
  }
  return { run, excluded };
}

// ---------------------------------------------------------------------------
// Workloads

/**
 * `kind` says what a workload is, so continuous traffic and load-idle
 * cycles are never confused:
 *   continuous   back-to-back load phases, no idle gap until the recovery
 *                readings after the last one;
 *   ladder       open-loop rates that rise until the storefront fails;
 *   load-idle    load and idle phases alternating: does memory return
 *                after each cycle?
 *   soak         long load-idle blocks (6 minutes each).
 */
export function workloads(opts = {}) {
  const loadIdleCycles = [1, 2, 3].flatMap((n) => [
    { name: `cycle${n}-mixed`, spec: "mixed-gzip.json", rate: 30, connections: 32, duration: 45 },
    { name: `cycle${n}-burst`, spec: "mixed-gzip.json", rate: 0, connections: 16, duration: 20, perIsolate: true },
    { name: `cycle${n}-idle`, idle: 60 },
  ]);
  const continuousMinutes = Math.max(5, Math.round(num(opts["continuous-minutes"], 30) / 5) * 5);
  return {
    // Baselines: every workload at a fixed rate below one isolate's
    // saturation on 1 CPU (the 2-CPU smoke run saturated at 60 mixed
    // requests/s), then saturation (closed loop).
    full: {
      kind: "continuous",
      description: "static 200/s, RSC 30/s, SSR 5/s, route handlers 15/s, mixed 30/s, closed-loop mixed, Server Actions 5/s; back to back",
      phases: [
        { name: "steady-static", spec: "static-gzip.json", rate: 200, connections: 32, duration: 45 },
        { name: "steady-rsc", spec: "rsc-gzip.json", rate: 30, connections: 16, duration: 45 },
        { name: "steady-ssr", spec: "ssr-gzip.json", rate: 5, connections: 16, duration: 45 },
        { name: "steady-api", spec: "api-gzip.json", rate: 15, connections: 16, duration: 45 },
        { name: "steady-mixed", spec: "mixed-gzip.json", rate: 30, connections: 32, duration: 60 },
        { name: "burst-mixed", spec: "mixed-gzip.json", rate: 0, connections: 16, duration: 30, perIsolate: true },
        // Server Actions (discover-action.mjs), after the RE-1 phases so those
        // stay comparable with the RE-1 baseline.
        { name: "steady-action", spec: "action-gzip.json", rate: 5, connections: 8, duration: 45 },
      ],
    },
    // Screening for configuration experiments: the mixed workload only.
    memory: {
      kind: "continuous",
      description: "mixed 30/s 60 s, then closed-loop mixed 30 s (screening)",
      phases: [
        { name: "steady-mixed", spec: "mixed-gzip.json", rate: 30, connections: 32, duration: 60 },
        { name: "burst-mixed", spec: "mixed-gzip.json", rate: 0, connections: 16, duration: 30, perIsolate: true },
      ],
    },
    // Uninterrupted mixed traffic for --continuous-minutes (default 30),
    // in 5-minute phases (290 s fixed rate, 10 s closed loop): memory
    // under sustained load, without the idle gaps of a soak.
    continuous: {
      kind: "continuous",
      description: `mixed 30/s for ${continuousMinutes} min without idle gaps (5-minute phases ending in 10 s closed loop)`,
      phases: Array.from({ length: continuousMinutes / 5 }, (_, i) => [
        { name: `cont${i + 1}-mixed`, spec: "mixed-gzip.json", rate: 30, connections: 32, duration: 290 },
        { name: `cont${i + 1}-burst`, spec: "mixed-gzip.json", rate: 0, connections: 16, duration: 10, perIsolate: true },
      ]).flat(),
    },
    // Repeated traffic and idle: does memory return to the same level after
    // every cycle (reclamation works, nothing accumulates)? Formerly
    // `cycles` (same phases; the old name is kept as an alias).
    "load-idle-cycles": {
      kind: "load-idle",
      description: "3 × (mixed 30/s 45 s, closed loop 20 s, idle 60 s)",
      phases: loadIdleCycles,
    },
    // Sustainable throughput: the mixed workload at rising fixed rates (open
    // loop, latency from the scheduled send time) until the storefront fails
    // or falls behind; then the closed-loop saturation phase.
    envelope: {
      kind: "ladder",
      description: "mixed at 15, 30, 45, 60, 80, 100, 120/s (40 s each) until the first rate with failures or < 95 % achieved; then closed loop",
      phases: [
        ...[15, 30, 45, 60, 80, 100, 120].map((rate) => (
          { name: `rate-${rate}`, spec: "mixed-gzip.json", rate, connections: 64, duration: 40, ladder: true })),
        { name: "burst-mixed", spec: "mixed-gzip.json", rate: 0, connections: 16, duration: 30, perIsolate: true },
      ],
    },
    // Soak (`--soak-minutes N`): 6-minute blocks of steady mixed traffic, a
    // burst, a Server Action phase and an idle gap, for N minutes. Whether
    // warmed-up memory settles in a range or keeps climbing shows in the
    // samples; a short soak cannot rule out a slow leak.
    soak: {
      kind: "soak",
      description: "6-minute blocks: mixed 30/s 210 s, closed loop 30 s, Server Actions 5/s 30 s, idle 90 s",
      phases: Array.from({ length: Math.max(1, Math.round(num(opts["soak-minutes"], 30) / 6)) }, (_, i) => [
        { name: `soak${i + 1}-mixed`, spec: "mixed-gzip.json", rate: 30, connections: 32, duration: 210 },
        { name: `soak${i + 1}-burst`, spec: "mixed-gzip.json", rate: 0, connections: 16, duration: 30, perIsolate: true },
        { name: `soak${i + 1}-action`, spec: "action-gzip.json", rate: 5, connections: 8, duration: 30 },
        { name: `soak${i + 1}-idle`, idle: 90 },
      ]).flat(),
    },
  };
}
export const WORKLOAD_ALIASES = { cycles: "load-idle-cycles" };

export function workload(name, opts) {
  const all = workloads(opts);
  const key = WORKLOAD_ALIASES[name] ?? name;
  return all[key] ? { name: key, ...all[key] } : null;
}

// ---------------------------------------------------------------------------
// The probe, cgroup readings, metrics

/** The probe: host PID and cgroup namespaces, on the tools CPUs. */
export function probeRunArgs() {
  return [
    "run", "-d", "--name", PROBE, "--pid", "host", "--cgroupns", "host",
    "--cap-drop", "ALL", "--cap-add", "SYS_PTRACE", "--cap-add", "DAC_READ_SEARCH",
    "--security-opt", "no-new-privileges:true", "--cpuset-cpus", L.LIMITS.toolsCpuset,
    "-v", "/sys/fs/cgroup:/cg:ro", "-v", `${L.HERE}/../resource:/re:ro`, "-v", "nextrs-s1-work:/work",
    "--user", "root", "--entrypoint", "sleep", L.IMAGES.node, "infinity",
  ];
}

export function setupProbe() {
  if (L.running(PROBE)) return;
  L.remove(PROBE);
  L.docker(probeRunArgs());
  L.docker(["exec", PROBE, "mkdir", "-p", "/work/re"]);
  log("probe up");
}

export function inspect(name) {
  const r = L.docker(["inspect", "-f", "{{json .State}} {{.Id}} {{.RestartCount}}", name], { allowFail: true });
  if (r.status !== 0) return null;
  const parts = r.stdout.trim().split(" ");
  const restarts = Number(parts.pop());
  const id = parts.pop();
  return { state: JSON.parse(parts.join(" ")), id, restarts };
}

/**
 * The container's cgroup directory as the probe sees it: `docker/ID`
 * (Docker Desktop, cgroupfs driver) or `system.slice/docker-ID.scope`
 * (Linux hosts with the systemd driver). Cached per container.
 */
const cgDirs = new Map();
export function cgDir(id) {
  if (!cgDirs.has(id)) {
    const r = L.docker(["exec", PROBE, "sh", "-c",
      `for d in /cg/docker/${id} /cg/system.slice/docker-${id}.scope; do [ -f $d/memory.current ] && echo $d && break; done`],
    { allowFail: true });
    const dir = r.stdout.trim();
    if (!dir) return `/cg/docker/${id}`; // gone already: readings fail as before
    cgDirs.set(id, dir);
  }
  return cgDirs.get(id);
}

/** One reading of the container's cgroup, from the probe. */
export function cgroup(id) {
  const dir = cgDir(id);
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

/** NextRS's native buffer budget: only in the Prometheus text (/metrics). */
function nativeBuffers(runtime) {
  if (runtime === "node") return null;
  const r = L.docker(["exec", L.TOOLS, "node", "-e",
    `fetch("http://${L.SF}:9100/metrics").then(r=>r.text()).then(t=>process.stdout.write(t)).catch(()=>{})`], { allowFail: true });
  const o = {};
  for (const line of r.stdout.split("\n")) {
    const m = line.match(/^nextrs_native_buffer_([a-z_]+)(\{[^}]*\})?\s+([0-9.eE+-]+)$/);
    if (m) o[`${m[1]}${m[2] ?? ""}`] = Number(m[3]);
  }
  return Object.keys(o).length ? o : null;
}

/**
 * The parts of NextRS's metrics.json these reports use, including the
 * RE-4 counters (admission, external memory, heap-limit episodes,
 * memory-pressure refusals) and the native buffer budget. With `raw`, the
 * whole document is pushed there too (raw preservation: `*.metrics.jsonl`).
 */
export function metrics(runtime, raw = null, at = "") {
  const m = L.nextrsMetrics(runtime);
  if (!m) return null;
  const nb = nativeBuffers(runtime);
  if (raw) raw.push({ t: Date.now(), at, metrics: m, native_buffers: nb });
  const pick = (o, keys) => Object.fromEntries(keys.filter((k) => o?.[k] !== undefined).map((k) => [k, o[k]]));
  return {
    ...pick(m, ["isolates", "queued", "requests_admitted", "requests_completed", "execution_seconds_total",
      "queue_wait_seconds_total", "queue_waits", "rejected_queue_full", "rejected_queue_timeout",
      "rejected_unavailable", "rejected_overloaded", "rejected_memory_pressure", "rejected_shutting_down",
      "request_timeouts", "handler_errors", "request_body_errors", "responses_cancelled",
      "memory", "cache", "compression_cache", "reclaim",
      "memory_reclaims_idle", "memory_reclaims_pressure", "memory_reclaim_seconds_total",
      "admission", "external_memory"]),
    executors: (m.executors ?? []).map((e) => pick(e, ["slot", "state", "generation", "active", "heap_used_bytes",
      "heap_total_bytes", "external_bytes", "heap_limit_bytes", "heap_used_after_gc_bytes", "full_gcs",
      "restarts", "failures", "last_failure", "near_heap_limit_calls", "near_heap_limit_declined", "last_heap_limit"])),
    native_buffers: nb,
    process: m.process,
  };
}

/** The load generator's arguments (see loadgen). */
export function loadgenArgs(spec, { rate, connections, duration, requests }) {
  const limit = Math.ceil(duration + 60);
  return [
    "exec", L.TOOLS, "timeout", "-s", "KILL", String(limit), "nextrs-loadgen", "--target", `${L.SF}:8080`,
    "--spec", spec, "--rate", String(rate), "--connections", String(connections), "--duration", String(duration),
    "--warmup", "0", "--requests", String(requests), "--threads", "2",
  ];
}

/**
 * The load generator, killed after its duration plus 60 s: it does not
 * give up on a target that died during a fixed-count run (a 128 MiB
 * container killed at startup hung the baseline for hours).
 */
export function loadgen(spec, p) {
  const args = loadgenArgs(spec, p);
  const r = L.docker(args, { allowFail: true });
  const limit = args[4];
  if (r.status !== 0) throw new Error(`loadgen exit ${r.status}${r.status === 137 ? ` (killed after ${limit} s)` : ""}: ${r.stderr.slice(0, 300)}`);
  return JSON.parse(r.stdout);
}

/** The sampler (sampler.mjs) for a container, detached in the probe. */
export function samplerArgs(cgroupDir, pid, file) {
  return ["exec", "-d", PROBE, "node", "/re/sampler.mjs", cgroupDir, String(pid), file, "1000", "5"];
}

// ---------------------------------------------------------------------------
// Correctness gate

/**
 * Checks a gate file (run.mjs gate) against the variants about to be
 * measured. `imageIds` (label -> image ID now) is optional: a dry run
 * cannot read images. Returns a list of problems (empty: the gate covers
 * every variant).
 */
export function checkGate(file, variants, imageIds = null) {
  if (!file || file === true) return ["no --gate FILE: run `run.mjs gate` first (or pass --no-gate-reason TEXT)"];
  if (!existsSync(file)) return [`gate file ${file} does not exist`];
  let gate;
  try {
    gate = JSON.parse(readFileSync(file, "utf8"));
  } catch (error) {
    return [`gate file ${file}: ${error.message}`];
  }
  const problems = [];
  if (gate.passed !== true) problems.push(`gate ${file} did not pass`);
  if (gate.build && gate.build.passed !== true) problems.push(`gate ${file}: build identity check did not pass`);
  for (const v of variants) {
    const label = v.gateAs ?? v.label;
    const g = (gate.variants ?? []).find((x) => x.label === label);
    if (!g) {
      problems.push(`${v.label}: not in the gate${v.gateAs ? ` (gateAs ${v.gateAs})` : ""}`);
      continue;
    }
    if (!g.passed) problems.push(`${v.label}: failed the gate (${(g.problems ?? []).slice(0, 2).join("; ")})`);
    if (g.runtime !== v.runtime) problems.push(`${v.label}: gate ran runtime ${g.runtime}, not ${v.runtime}`);
    if (!v.gateAs && (g.args ?? "") !== (v.args ?? "")) problems.push(`${v.label}: gate ran args "${g.args ?? ""}", not "${v.args ?? ""}"`);
    if (JSON.stringify(g.env ?? []) !== JSON.stringify(v.env ?? [])) problems.push(`${v.label}: gate ran another env`);
    if (imageIds && imageIds[v.label] && g.imageId && imageIds[v.label] !== g.imageId) {
      problems.push(`${v.label}: image changed since the gate (${g.imageId.slice(0, 19)} -> ${imageIds[v.label].slice(0, 19)})`);
    }
  }
  return problems;
}

/**
 * Differences between two captures (capture.mjs), as a set of keys
 * `request|field`: the same comparison compare.mjs prints.
 */
export function captureDiffs(A, B) {
  const key = (r) => `${r.name} ${r.method} ${r.path}`;
  const byKey = new Map(B.map((r) => [key(r), r]));
  const out = new Set();
  for (const a of A) {
    const b = byKey.get(key(a));
    if (!b) {
      out.add(`${key(a)}|missing`);
      continue;
    }
    if (a.sha256 !== b.sha256) out.add(`${key(a)}|body`);
    if (a.status !== b.status) out.add(`${key(a)}|status`);
    for (const h of new Set([...Object.keys(a.headers), ...Object.keys(b.headers)])) {
      if (h === "etag" || h === "last-modified") continue;
      if (JSON.stringify(a.headers[h]) !== JSON.stringify(b.headers[h])) out.add(`${key(a)}|header ${h}`);
    }
    if (Boolean(a.headers.etag) !== Boolean(b.headers.etag)) out.add(`${key(a)}|etag presence`);
    if (a.decodeError || b.decodeError) out.add(`${key(a)}|gzip decode`);
  }
  for (const b of B) if (!A.some((a) => key(a) === key(b))) out.add(`${key(b)}|extra`);
  return out;
}

export { join };
