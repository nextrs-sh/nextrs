// Resource-efficiency (RE) measurements: memory, CPU and failures of one
// storefront under container limits, on the Series 1 local stack
// (benchmarks/series1: network, API, tools container, images).
//
//   node benchmarks/series1/run.mjs setup --loadgen PATH --bin base=PATH ...
//   node benchmarks/resource/run.mjs setup
//   node benchmarks/resource/run.mjs setup-faults --bin LABEL=PATH ...      # fault-app images
//   node benchmarks/resource/run.mjs discover [--runtime LABEL]
//   node benchmarks/resource/run.mjs validate --variants FILE [profile options] [--bin L=PATH] [--commit L=SHA] [--check-images]
//   node benchmarks/resource/run.mjs verify-build (--variants FILE [--app velasia|fault] | --images IMG=ROOT,...) --out ABS [--dry-run]
//   node benchmarks/resource/run.mjs gate --variants FILE --baseline node --reference LABEL --candidates L,... \
//        [--gate-configs 1g:2] --out ABS [--dry-run]
//   node benchmarks/resource/run.mjs profile --variants FILE --gate ABS/gate.json \
//        [--configs 1g:2,512m:1,256m:1,256m:0.5] [--rounds 2] \
//        [--workload full|memory|continuous|load-idle-cycles|envelope|soak|faults-*] [--soak-minutes N]
//        [--only LABEL,...] [--first-round N] [--recovery 10,30,60,120,300] [--save-logs]
//        [--allow-config LABEL=PATTERN --allow-reason TEXT] [--no-gate-reason TEXT] [--dry-run] --out ABS
//   node benchmarks/resource/summarize.mjs DIR
//   node benchmarks/resource/run.mjs teardown
//
// A variants file is a JSON array of
//   { "label": "base", "runtime": "base", "isolates": 1, "args": "", "env": [] }
// where `runtime` is `node` (the Next.js standalone server), `rc2` or a
// NextRS binary label (`nextrs-s1:<label>`, built by series1 setup), `args`
// go to `nextrs start` (NEXTRS_EXTRA_ARGS) and `env` are NAME=value
// entries (allocator and runtime experiments). `image` overrides the image.
// `configs` / `excludeConfigs` (+ `excludeReason`) limit the
// configurations a variant runs in; `gateAs` names the gated variant a
// copy (e.g. with --trace-gc) is equivalent to. See harness.mjs.
//
// Each (variant, config, round) gets a fresh container. A sampler in the
// probe container reads the container's cgroup and its main process from
// outside once a second (sampler.mjs), so sampling adds nothing to the
// measured CPU or memory. Rounds alternate the variant order. A run that
// fails (OOM kill, readiness timeout, failed requests) is recorded with its
// reason, never dropped. Raw data per run: `*.samples.jsonl` (every
// 1-second sample), `*.loadgen.jsonl` (every load generator result,
// untrimmed), `*.metrics.jsonl` (every /metrics.json reading, whole).
//
// `profile` refuses to start without a passing correctness gate (`gate`)
// that covers every variant with the same runtime, args, env and image ID;
// `--no-gate-reason` overrides it for non-decisive runs (recorded).
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import * as L from "../series1/lib.mjs";
import * as H from "./harness.mjs";
import { faultWorkload, planFaults, runFaults } from "./faults.mjs";
import { FAULT_IMAGES, printChecks, validate, variantImage } from "./validate.mjs";

let parsed;
try {
  parsed = H.parseArgs(process.argv.slice(2));
} catch (error) {
  console.error(error.message);
  process.exit(2);
}
const { command, opts } = parsed;
const argv = process.argv.slice(2);
const log = H.log;
const dry = Boolean(opts["dry-run"]);

function setup() {
  if (dry) return console.log(H.shellLine("docker", H.probeRunArgs()));
  H.setupProbe();
}

/**
 * The fault app's NextRS images: `nextrs-fault-base:s1` (build-images.sh
 * fault) with only /usr/local/bin/nextrs added, checked with `nextrs
 * inspect` at build time.
 */
function setupFaults() {
  const bins = H.pairs(opts.bin, "--bin");
  if (!bins.size) throw new Error("--bin LABEL=PATH (one per NextRS build)");
  for (const [label, bin] of bins) {
    const path = resolve(bin);
    const dir = path.slice(0, path.lastIndexOf("/"));
    const file = path.slice(dir.length + 1);
    const dockerfile = `FROM ${FAULT_IMAGES.nextrsBase}\nCOPY --chmod=0755 ${file} /usr/local/bin/nextrs\nRUN ["/usr/local/bin/nextrs", "inspect", "/app"]\n`;
    const args = ["build", "-q", "-t", `nextrs-fault:${label}`, "-f", "-", dir];
    console.log(`${H.shellLine("docker", args)} <<'EOF'\n${dockerfile}EOF`);
    if (!dry) {
      L.docker(args, { input: dockerfile });
      log(`image nextrs-fault:${label} <- ${path}`);
    }
  }
}

async function discover() {
  const variant = { runtime: opts.runtime ?? "base" };
  await L.storefrontUp(variant.runtime);
  log(L.toolsNode("discover.mjs", [`http://${L.SF}:8080`, H.SPECS]).trim());
  L.docker(["cp", join(L.HERE, "../resource/discover-dynamic.mjs"), `${L.TOOLS}:/tmp/discover-dynamic.mjs`]);
  log(L.docker(["exec", L.TOOLS, "node", "/tmp/discover-dynamic.mjs", `http://${L.SF}:8080`, H.SPECS]).stdout.trim());
  L.docker(["cp", join(L.HERE, "../resource/discover-action.mjs"), `${L.TOOLS}:/tmp/discover-action.mjs`]);
  log(L.docker(["exec", L.TOOLS, "node", "/tmp/discover-action.mjs", `http://${L.SF}:8080`, H.SPECS]).stdout.trim());
  L.remove(L.SF);
}

function faultsWorkload() {
  const name = opts.workload ?? "full";
  if (!name.startsWith("faults-")) return null;
  const configs = H.list(opts.configs, "256m:1");
  return faultWorkload(name, opts, configs[0]);
}

function validateCommand() {
  const r = validate(opts, { command: opts.for ?? "profile", faultsWorkload: faultsWorkload() });
  printChecks(r.checks);
  process.exitCode = r.fail ? 1 : 0; // not exit(): let piped stdout drain
}

// ---------------------------------------------------------------------------
// Build identity: every image serves the same `next build` output.

/** A shell script, run in an image, that lists BUILD_ID and every file's sha256. */
function hashScript(root, appDir) {
  const next = appDir === "." ? ".next" : `${appDir}/.next`;
  return [
    "set -eu", `cd ${root}`, `printf 'BUILD_ID %s\\n' "$(cat ${next}/BUILD_ID)"`,
    // Runtime-specific files the images add beside the build: excluded.
    `find . \\( -path ./${next}/cache -o -path ./node-version.txt -o -path ./nextrs-version.txt \\) -prune -o -type f -print | LC_ALL=C sort | xargs -d '\\n' sha256sum`,
  ].join("; ");
}

export function hashArgs(image, root, appDir) {
  return ["run", "--rm", "--network", "none", "--read-only", "--cap-drop", "ALL", "--user", "0:0", "--entrypoint", "sh", image, "-c", hashScript(root, appDir)];
}

function verifyBuildImages(images, appDir, out, name) {
  if (!dry) mkdirSync(out, { recursive: true });
  const results = [];
  for (const { image, root } of images) {
    const args = hashArgs(image, root, appDir);
    if (dry) {
      console.log(H.shellLine("docker", args));
      continue;
    }
    const r = L.docker(args, { allowFail: true });
    if (r.status !== 0) {
      results.push({ image, root, error: (r.stderr || r.stdout).slice(0, 300) });
      continue;
    }
    const lines = r.stdout.trim().split("\n");
    const buildId = lines.shift().replace(/^BUILD_ID /, "");
    const next = appDir === "." ? "./.next/" : `./${appDir}/.next/`;
    const nextLines = lines.filter((l) => l.slice(66).startsWith(next));
    const sha = (ls) => createHash("sha256").update(ls.join("\n") + "\n").digest("hex");
    const id = L.docker(["image", "inspect", "-f", "{{.Id}}", image], { allowFail: true }).stdout.trim();
    const safe = image.replace(/[^A-Za-z0-9._-]/g, "_");
    writeFileSync(join(out, `${name}-${safe}.sha256`), lines.join("\n") + "\n");
    results.push({ image, imageId: id, root, buildId, files: lines.length, nextFiles: nextLines.length, nextSha256: sha(nextLines), treeSha256: sha(lines) });
  }
  if (dry) return null;
  const ok = results.filter((x) => !x.error);
  const same = (k) => ok.length === results.length && new Set(ok.map((x) => x[k])).size === 1;
  const report = {
    name, checked: new Date().toISOString(), appDir, images: results,
    sameBuildId: same("buildId"), sameNext: same("nextSha256"), sameTree: same("treeSha256"),
  };
  report.passed = report.sameBuildId && report.sameNext && report.sameTree;
  writeFileSync(join(out, `${name}-build-identity.json`), JSON.stringify(report, null, 1));
  for (const x of results) log(`${x.image}: ${x.error ? `ERROR ${x.error}` : `BUILD_ID ${x.buildId}, .next ${x.nextSha256.slice(0, 16)} (${x.nextFiles} files), tree ${x.treeSha256.slice(0, 16)} (${x.files} files)`}`);
  log(`build identity ${report.passed ? "PASS" : "FAIL"}: BUILD_ID ${report.sameBuildId ? "same" : "DIFFERS"}, .next ${report.sameNext ? "same" : "DIFFERS"}, standalone tree ${report.sameTree ? "same" : "DIFFERS"}`);
  return report;
}

/** The images of a variants file (plus the builder image, when named). */
function variantBuildImages(variants, app) {
  const images = new Map();
  for (const v of variants) images.set(variantImage(v, app), { image: variantImage(v, app), root: "/app" });
  return [...images.values()];
}

function verifyBuild() {
  const out = H.requireAbsoluteOut(opts.out);
  const app = opts.app ?? "velasia";
  let images;
  if (opts.images) {
    images = H.list(opts.images).map((p) => {
      const at = p.lastIndexOf("=");
      return { image: p.slice(0, at), root: p.slice(at + 1) };
    });
  } else {
    const r = H.readVariants(opts.variants);
    if (r.problems.length) throw new Error(r.problems.join("\n"));
    images = variantBuildImages(r.variants, app);
  }
  const appDir = opts["app-dir"] ?? (app === "fault" ? "." : "apps/web");
  const report = verifyBuildImages(images, appDir, out, opts.name ?? app);
  if (report && !report.passed) process.exit(1);
}

// ---------------------------------------------------------------------------
// Correctness gate: baseline vs candidates, before any timing.

async function gate() {
  const v = validate(opts, { command: "gate" });
  const out = H.requireAbsoluteOut(opts.out);
  const baseline = opts.baseline ?? "node";
  const reference = opts.reference;
  const candidates = H.list(opts.candidates, "");
  const problems = [];
  if (!reference || reference === true) problems.push("--reference LABEL is required (the gated NextRS build the candidates are held to)");
  if (!candidates.length) problems.push("--candidates L,... is required");
  for (const l of [baseline, reference, ...candidates]) if (l && l !== true && !v.variants.some((x) => x.label === l)) problems.push(`${l}: not in the variants file`);
  if (problems.length || v.fail) {
    printChecks(v.checks);
    for (const p of problems) console.log(`FAIL  gate       ${p}`);
    process.exitCode = 1;
    return;
  }
  const configs = v.configs;
  // The variants a configuration captures: those that run in it (excludeConfigs apply).
  const gated = (config) => v.plan.filter((p) => p.run.some((r) => r.config === config)).map((p) => p.variant);
  if (dry) {
    printChecks(v.checks);
    console.log(`\n# gate: baseline ${baseline}, reference ${reference}, candidates ${candidates.join(", ")}; configs ${configs.join(", ")}`);
    verifyBuildImages(variantBuildImages(v.variants, "velasia"), "apps/web", join(out, "build"), "gate");
    for (const config of configs) {
      const [memory, cpus] = config.split(":");
      for (const x of gated(config)) {
        console.log(H.shellLine("docker", L.storefrontRunArgs(x.runtime, { isolates: x.isolates ?? 1, extraArgs: x.args ?? "", env: x.env ?? [], image: x.image, memory, cpus })));
        console.log(H.shellLine("docker", ["exec", L.TOOLS, "node", "/s1/capture.mjs", `http://${L.SF}:8080`, H.SPECS, `/work/capture-${x.label}-${memory}-${cpus}cpu.json`]));
      }
      for (const p of v.plan) for (const e of p.excluded.filter((e) => e.config === config)) console.log(`# skip ${p.variant.label} ${config}: excluded by the variants file`);
    }
    console.log(`# then: for every candidate C, the differences (reference, C) must all be differences (baseline, reference) already; writes ${join(out, "gate.json")}`);
    return;
  }
  mkdirSync(out, { recursive: true });
  const build = verifyBuildImages(variantBuildImages(v.variants, "velasia"), "apps/web", join(out, "build"), "gate");
  const manifest = L.docker(["exec", L.TOOLS, "cat", `${H.SPECS}/manifest.json`], { allowFail: true });
  if (manifest.status !== 0) throw new Error("no discovered specs: run `run.mjs discover` first");
  const entries = [];
  const captures = new Map();
  for (const config of configs) {
    const [memory, cpus] = config.split(":");
    for (const x of gated(config)) {
      const file = `capture-${x.label}-${memory}-${cpus}cpu.json`;
      const entry = { label: x.label, runtime: x.runtime, args: x.args ?? "", env: x.env ?? [], image: variantImage(x), config, capture: file };
      entry.imageId = L.docker(["image", "inspect", "-f", "{{.Id}}", entry.image], { allowFail: true }).stdout.trim();
      try {
        await L.storefrontUp(x.runtime, { isolates: x.isolates ?? 1, extraArgs: x.args ?? "", env: x.env ?? [], image: x.image, memory, cpus, readyTimeoutMs: 120_000 });
        L.docker(["exec", L.TOOLS, "node", "/s1/capture.mjs", `http://${L.SF}:8080`, H.SPECS, `/work/${file}`]);
        const json = L.docker(["exec", L.TOOLS, "cat", `/work/${file}`]).stdout;
        writeFileSync(join(out, file), json);
        captures.set(`${x.label}|${config}`, JSON.parse(json));
      } catch (error) {
        entry.error = String(error.message).slice(0, 300);
      }
      L.remove(L.SF);
      entries.push(entry);
      log(`captured ${x.label} ${config}${entry.error ? `: ${entry.error}` : ""}`);
    }
  }
  const lines = ["# Correctness gate", "", `Baseline ${baseline}, reference ${reference}, candidates ${candidates.join(", ")}.`, ""];
  for (const config of configs) {
    const cap = (l) => captures.get(`${l}|${config}`);
    const known = cap(baseline) && cap(reference) ? H.captureDiffs(cap(baseline), cap(reference)) : null;
    lines.push(`## ${config}`, "", `${baseline} vs ${reference}: ${known ? known.size : "not captured"} known difference(s)`);
    for (const e of entries.filter((x) => x.config === config)) {
      e.problems = [];
      if (e.error) e.problems.push(`not captured: ${e.error}`);
      if (e.label === baseline) e.role = "baseline";
      else if (e.label === reference) e.role = "reference";
      else if (candidates.includes(e.label)) e.role = "candidate";
      else e.role = "informational";
      if (!e.error && cap(baseline)) e.diffsVsBaseline = [...H.captureDiffs(cap(baseline), cap(e.label))];
      if (e.role === "candidate") {
        if (!known) e.problems.push("baseline or reference not captured");
        else if (!e.error) {
          e.newDiffs = [...H.captureDiffs(cap(reference), cap(e.label))].filter((d) => !known.has(d));
          if (e.newDiffs.length) e.problems.push(`${e.newDiffs.length} difference(s) from ${reference} where ${reference} matched ${baseline}: ${e.newDiffs.slice(0, 3).join("; ")}`);
        }
      }
      if ((e.role === "baseline" || e.role === "reference") && e.error) e.problems.push("a gate reference failed to capture");
      e.passed = e.problems.length === 0;
      lines.push(`- ${e.label} (${e.role}): ${e.passed ? "pass" : `FAIL: ${e.problems.join("; ")}`}${e.diffsVsBaseline ? `; ${e.diffsVsBaseline.length} difference(s) from ${baseline}` : ""}`);
    }
    lines.push("");
  }
  // One gate entry per label: it passes only if it passed in every configuration.
  const byLabel = new Map();
  for (const e of entries) {
    const prev = byLabel.get(e.label);
    if (!prev) byLabel.set(e.label, { ...e, configs: [e.config], problems: [...e.problems] });
    else {
      prev.configs.push(e.config);
      prev.passed = prev.passed && e.passed;
      prev.problems.push(...e.problems);
    }
  }
  const result = {
    created: new Date().toISOString(), commit: L.gitCommit(), command: argv.join(" "), variantsFile: resolve(opts.variants),
    baseline, reference, candidates, configs, build: build && { passed: build.passed, sameBuildId: build.sameBuildId, sameNext: build.sameNext, sameTree: build.sameTree },
    variants: [...byLabel.values()],
  };
  result.passed = Boolean(build?.passed) && result.variants.every((e) => e.passed || e.role === "informational");
  writeFileSync(join(out, "gate.json"), JSON.stringify(result, null, 1));
  writeFileSync(join(out, "gate.md"), lines.join("\n"));
  console.log(lines.join("\n"));
  log(`gate ${result.passed ? "PASS" : "FAIL"}: ${join(out, "gate.json")}`);
  if (!result.passed) process.exit(1);
}

// ---------------------------------------------------------------------------
// Profiles

async function phase(run, id, runtime, p, isolates, raw) {
  if (p.idle) {
    const started = Date.now();
    await L.sleep(p.idle * 1000);
    run.phases.push({ name: p.name, idle: p.idle, started, ended: Date.now(), after: H.cgroup(id), metricsAfter: H.metrics(runtime, raw.metrics, `${p.name}:after`) });
    log(`  ${p.name}: idle ${p.idle} s`);
    return null;
  }
  const connections = p.perIsolate ? p.connections * Math.max(isolates, 1) : p.connections;
  const contentionBefore = L.contention();
  const before = H.cgroup(id);
  const mBefore = H.metrics(runtime, raw.metrics, `${p.name}:before`);
  const started = Date.now();
  let result;
  try {
    result = H.loadgen(`${H.SPECS}/${p.spec}`, { rate: p.rate, connections, duration: p.duration, requests: p.requests ?? 0 });
  } catch (error) {
    result = { error: String(error.message).slice(0, 500) };
  }
  raw.loadgen.push({ name: p.name, started, ended: Date.now(), result });
  const after = H.cgroup(id);
  const entry = {
    name: p.name, spec: p.spec, rate: p.rate, connections, duration: p.duration, started, ended: Date.now(),
    before, after, metricsBefore: mBefore, metricsAfter: H.metrics(runtime, raw.metrics, `${p.name}:after`),
    contention: { before: contentionBefore, after: L.contention() },
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

const CHECKS = ["ssr-identity.json", "api-identity.json", "action-identity.json"];

async function one(variant, config, round, w, out) {
  const [memory, cpus] = config.split(":");
  const isolates = variant.isolates ?? 1;
  const file = `${variant.label}-${memory}-${cpus}cpu-r${round}`;
  const run = { variant, memory, cpus, round, workload: w.name, workloadKind: w.kind, phases: [], marks: {}, contention: { before: L.contention() } };
  const raw = { loadgen: [], metrics: [] };
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
  const info = H.inspect(L.SF);
  const samples = `/work/re/${L.SF}-${file}.jsonl`;
  if (info) {
    L.docker(["exec", H.PROBE, "rm", "-f", samples], { allowFail: true });
    L.docker(H.samplerArgs(H.cgDir(info.id), info.state.Pid, samples));
  }
  run.marks.start = started;
  if (info) {
    // Which allocator libraries the process actually mapped: an
    // LD_PRELOAD that did not load would make an allocator run a baseline.
    const maps = L.docker(["exec", H.PROBE, "sh", "-c", `grep -oE '[^ /]*(jemalloc|mimalloc|tcmalloc)[^ ]*' /proc/${info.state.Pid}/maps | sort -u`], { allowFail: true });
    run.mappedAllocators = maps.stdout.trim().split("\n").filter(Boolean);
  }
  if (!run.error) {
    await L.sleep(30_000);
    run.marks.idle = Date.now();
    run.idle = { cgroup: H.cgroup(info.id), metrics: H.metrics(variant.runtime, raw.metrics, "idle") };
    // Correctness: every SSR and route-handler request once, body-checked.
    for (const spec of CHECKS) {
      const n = JSON.parse(L.docker(["exec", L.TOOLS, "cat", `${H.SPECS}/${spec}`]).stdout).requests.length;
      await phase(run, info.id, variant.runtime, { name: `check-${spec.split("-")[0]}`, spec, rate: 0, connections: 1, duration: 60, requests: n }, isolates, raw);
    }
    // Cold: every path of the mixed workload once, one connection.
    const n = JSON.parse(L.docker(["exec", L.TOOLS, "cat", `${H.SPECS}/mixed-gzip.json`]).stdout).requests.length;
    await phase(run, info.id, variant.runtime, { name: "cold-mixed", spec: "mixed-gzip.json", rate: 0, connections: 1, duration: 120, requests: n }, isolates, raw);
    for (const p of w.phases) {
      if (!H.inspect(L.SF)?.state.Running) break;
      const entry = await phase(run, info.id, variant.runtime, p, isolates, raw);
      // The ladder stops at the first rate the storefront cannot sustain.
      if (p.ladder && entry && (entry.error || entry.failed > 0 || (entry.result?.achieved_rps ?? 0) < p.rate * 0.95)) {
        run.ladderStop = p.name;
        break;
      }
    }
    run.marks.loadEnd = Date.now();
    run.recovery = [];
    for (const mark of RECOVERY_MARKS) {
      await L.sleep(Math.max(0, run.marks.loadEnd + mark * 1000 - Date.now()));
      if (!H.inspect(L.SF)?.state.Running) break;
      run.recovery.push({ after_s: mark, cgroup: H.cgroup(info.id), metrics: H.metrics(variant.runtime, raw.metrics, `recovery-${mark}`) });
    }
  }
  const end = H.inspect(L.SF);
  run.final = end && { running: end.state.Running, oomKilled: end.state.OOMKilled, exitCode: end.state.ExitCode, restarts: end.restarts };
  run.lastCgroup = info && H.cgroup(info.id);
  if (end && !end.state.Running) run.logTail = L.docker(["logs", "--tail", "20", L.SF], { allowFail: true }).stderr.slice(-4000);
  // --save-logs: the container's whole log (e.g. V8's --trace-gc lines).
  if (opts["save-logs"] && end) {
    const logs = L.docker(["logs", L.SF], { allowFail: true });
    writeFileSync(join(out, `${file}.log`), `${logs.stdout}${logs.stderr}`);
  }
  run.contention.after = L.contention();
  L.remove(L.SF);
  await L.sleep(1500); // the sampler stops when the cgroup disappears
  const jsonl = L.docker(["exec", H.PROBE, "cat", samples], { allowFail: true }).stdout;
  // Raw first, the run file last: a run file always has its raw data beside it.
  writeFileSync(join(out, `${file}.samples.jsonl`), jsonl);
  writeFileSync(join(out, `${file}.loadgen.jsonl`), raw.loadgen.map((x) => JSON.stringify(x)).join("\n") + (raw.loadgen.length ? "\n" : ""));
  writeFileSync(join(out, `${file}.metrics.jsonl`), raw.metrics.map((x) => JSON.stringify(x)).join("\n") + (raw.metrics.length ? "\n" : ""));
  writeFileSync(join(out, `${file}.json`), JSON.stringify(run));
  const oom = run.lastCgroup?.oom_kill ?? null;
  log(`  done: ready ${run.readyMs ?? "-"} ms${run.error ? `, ${run.error}` : ""}${run.final?.oomKilled || oom ? `, OOM (oom_kill ${oom})` : ""}`);
}

const RECOVERY_MARKS = H.list(opts.recovery, "10,30,60,120,300").map(Number);

/** Printed by --dry-run: every container command, the phases and a time estimate. */
function planProfile(v, w, rounds, first) {
  const lines = [`# ${w.name} (${w.kind}): ${w.description}`, ""];
  let seconds = 0;
  for (let round = first; round < first + rounds; round++) {
    const order = round % 2 ? v.plan : [...v.plan].reverse();
    for (const config of v.configs) {
      for (const p of order) {
        const x = p.variant;
        if (!p.run.some((r) => r.config === config)) {
          lines.push(`# skip ${x.label} ${config}: ${p.excluded.find((e) => e.config === config)?.reason}`);
          continue;
        }
        const [memory, cpus] = config.split(":");
        const isolates = x.isolates ?? 1;
        const file = `${x.label}-${memory}-${cpus}cpu-r${round}`;
        lines.push("", `## ${file}`);
        lines.push(H.shellLine("docker", L.storefrontRunArgs(x.runtime, { isolates, extraArgs: x.args ?? "", env: x.env ?? [], image: x.image, memory, cpus })));
        lines.push(`#   ready (120 s at most); ${H.shellLine("docker", H.samplerArgs("<cgroup dir>", "<pid>", `/work/re/${L.SF}-${file}.jsonl`))}`);
        lines.push(`#   idle 30 s; checks ${CHECKS.join(", ")}; cold-mixed`);
        let s = 15 + 30 + 20 + 40;
        for (const ph of w.phases) {
          if (ph.idle) {
            lines.push(`#   ${ph.name}: idle ${ph.idle} s`);
            s += ph.idle;
            continue;
          }
          const c = ph.perIsolate ? ph.connections * Math.max(isolates, 1) : ph.connections;
          lines.push(`#   ${ph.name}: ${H.shellLine("docker", H.loadgenArgs(`${H.SPECS}/${ph.spec}`, { rate: ph.rate, connections: c, duration: ph.duration, requests: 0 }))}`);
          s += ph.duration + 1;
        }
        lines.push(`#   recovery readings at ${RECOVERY_MARKS.join(", ")} s; docker inspect; ${opts["save-logs"] ? "docker logs > *.log; " : ""}docker rm -f ${L.SF}`);
        lines.push(`#   writes ${file}.samples.jsonl, .loadgen.jsonl, .metrics.jsonl, .json`);
        s += Math.max(...RECOVERY_MARKS, 0) + 2;
        seconds += s;
      }
    }
  }
  return { lines, seconds };
}

async function profile() {
  const fw = faultsWorkload();
  const v = validate(opts, { command: "profile", faultsWorkload: fw });
  const rounds = H.num(opts.rounds, 2);
  const first = H.num(opts["first-round"], 1);
  if (!(Number.isInteger(rounds) && rounds >= 1)) v.checks.push({ status: "FAIL", name: "rounds", detail: `--rounds ${opts.rounds}` }), (v.fail = true);
  if (dry) {
    printChecks(v.checks);
    if (v.workload) {
      const p = fw ? planFaults(fw, v.plan, opts, rounds, first) : planProfile(v, v.workload, rounds, first);
      console.log(`\n${p.lines.join("\n")}`);
      console.log(`\n# about ${(p.seconds / 3600).toFixed(1)} h of runs (${rounds} round(s); excludes image pulls and container start-up beyond the estimates)`);
    }
    console.log(v.fail ? "\nDRY RUN: validation FAILED; a real run would refuse to start." : "\nDRY RUN: nothing was started.");
    process.exitCode = v.fail ? 1 : 0; // not exit(): let piped stdout drain
    return;
  }
  if (v.fail) {
    printChecks(v.checks, console.error);
    console.error("\nrefusing to start: fix the FAIL lines (validate prints the same checks)");
    process.exit(1);
  }
  const out = H.requireAbsoluteOut(opts.out);
  mkdirSync(out, { recursive: true });
  const images = Object.fromEntries(v.variants.map((x) => [x.label, L.docker(["image", "inspect", "-f", "{{.Id}}", variantImage(x, v.app)], { allowFail: true }).stdout.trim()]));
  // The images must be the ones the gate checked.
  if (!opts["no-gate-reason"] && v.app === "velasia") {
    const problems = H.checkGate(opts.gate, v.variants, images);
    if (problems.length) {
      for (const p of problems) console.error(`FAIL  gate  ${p}`);
      process.exit(1);
    }
  }
  const gateFile = opts.gate && opts.gate !== true ? resolve(opts.gate) : null;
  writeFileSync(join(out, `meta-${new Date().toISOString().replace(/[:.]/g, "")}.json`), JSON.stringify({
    command: argv.join(" "), commit: L.gitCommit(), started: new Date().toISOString(), host: L.hostInfo(), node: process.versions.node,
    variants: v.variants, configs: v.configs, rounds, workload: v.workload.name, workloadKind: v.workload.kind, app: v.app,
    phases: v.workload.phases ?? v.workload.cases, recoveryMarks: RECOVERY_MARKS,
    excluded: v.plan.flatMap((p) => p.excluded.map((e) => ({ label: p.variant.label, ...e }))),
    allowed: v.plan.flatMap((p) => p.run.filter((r) => r.lifted).map((r) => ({ label: p.variant.label, config: r.config, pattern: r.lifted, reason: opts["allow-reason"] }))),
    gate: gateFile && existsSync(gateFile) ? { file: gateFile, sha256: createHash("sha256").update(readFileSync(gateFile)).digest("hex") } : null,
    noGateReason: opts["no-gate-reason"] ?? null,
    images,
  }, null, 1));
  if (fw) return runFaults({ w: fw, plan: v.plan, opts, rounds, first, out });
  for (let round = first; round < first + rounds; round++) {
    const order = round % 2 ? v.plan : [...v.plan].reverse();
    for (const config of v.configs) {
      for (const p of order) if (p.run.some((r) => r.config === config)) await one(p.variant, config, round, v.workload, out);
    }
  }
}

function teardown() {
  L.remove(H.PROBE);
}

const commands = { setup, "setup-faults": setupFaults, discover, validate: validateCommand, "verify-build": verifyBuild, gate, profile, teardown };
if (!commands[command]) {
  console.error(`usage: run.mjs ${Object.keys(commands).join("|")} [options]`);
  process.exit(2);
}
try {
  await commands[command]();
} catch (error) {
  console.error(`${command}: ${error.message}`);
  process.exit(1);
}
