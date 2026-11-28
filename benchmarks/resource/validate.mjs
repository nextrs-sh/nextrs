// Static checks of a planned run (`run.mjs validate`, and the first step of
// every `--dry-run`): nothing is started, nothing is written. Docker is
// asked only with --check-images, and then only `docker image inspect`.
//
// Checks: the variants file, that every variant's runtime resolves to an
// image or a provided --bin (an x86-64 ELF executable), that its `args`
// are flags of the binary's commit (--commit LABEL=SHA, read from git),
// the configurations and per-variant exclusions, the workload, the CPU
// sets, the output directory (absolute), the correctness gate and the
// Node.js version.
import { closeSync, existsSync, openSync, readdirSync, readSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { isAbsolute } from "node:path";
import os from "node:os";
import * as L from "../series1/lib.mjs";
import * as H from "./harness.mjs";
import { checkArgs, cliSource, startFlags } from "./start-flags.mjs";

// Flags the images' start scripts always pass (Velasia's
// docker/nextrs/start.sh at d04127a; fault-app/docker-start.sh).
const VELASIA_START_FLAGS = ["--host", "--port", "--js-runtime", "--isolates", "--shutdown-grace",
  "--image-cache-dir", "--net-allow", "--js-env", "--net-allow-addr", "--metrics-addr"];
const FAULT_START_FLAGS = ["--host", "--port", "--js-runtime", "--isolates", "--metrics-addr"];

export const FAULT_IMAGES = {
  node: process.env.RE_FAULT_NODE_IMAGE ?? "nextrs-fault:node",
  nextrsBase: process.env.RE_FAULT_NEXTRS_IMAGE ?? "nextrs-fault-base:s1",
};
export const faultImage = (v) => v.faultImage ?? (v.runtime === "node" ? FAULT_IMAGES.node : `nextrs-fault:${v.runtime}`);

/** The image a variant runs as, for an app (`velasia` or `fault`). */
export function variantImage(v, app = "velasia") {
  if (app === "fault") return faultImage(v);
  return v.image ?? L.image(v.runtime);
}

function elf(path) {
  const fd = openSync(path, "r");
  const b = Buffer.alloc(20);
  readSync(fd, b, 0, 20, 0);
  closeSync(fd);
  if (b.readUInt32BE(0) !== 0x7f454c46) return "not an ELF file";
  if (b[4] !== 2) return "not a 64-bit ELF";
  if (b.readUInt16LE(18) !== 0x3e) return `ELF machine ${b.readUInt16LE(18)}, not x86-64`;
  return null;
}

function cpuset(s) {
  const out = new Set();
  for (const part of String(s).split(",")) {
    const m = part.trim().match(/^(\d+)(?:-(\d+))?$/);
    if (!m) return null;
    for (let i = Number(m[1]); i <= Number(m[2] ?? m[1]); i++) out.add(i);
  }
  return out;
}

/**
 * `opts`: parsed run.mjs options. `plan`: { command } ("profile", "gate",
 * "validate"). Returns { checks, variants, workload, configs, fail }.
 */
export function validate(opts, { command = "validate", faultsWorkload = null } = {}) {
  const checks = [];
  const add = (status, name, detail = "") => checks.push({ status, name, detail });
  const bins = H.pairs(opts.bin, "--bin");
  const commits = H.pairs(opts.commit, "--commit");
  const allow = H.pairs(opts["allow-config"], "--allow-config");

  // Node.js running the harness.
  const [major] = process.versions.node.split(".").map(Number);
  if (major < 22) add("FAIL", "node", `harness runs on Node.js ${process.versions.node}; needs 22 or later (pin: ${H.NODE_PIN})`);
  else add(process.versions.node === H.NODE_PIN ? "PASS" : "WARN", "node", `harness on Node.js ${process.versions.node} (pin ${H.NODE_PIN})`);

  // Variants file.
  let variants = [];
  if (!opts.variants || opts.variants === true) add("FAIL", "variants", "--variants FILE is required");
  else {
    const r = H.readVariants(opts.variants);
    variants = r.variants.filter((v) => !opts.only || H.list(opts.only).includes(v.label));
    if (r.problems.length) for (const p of r.problems) add("FAIL", "variants", p);
    else add("PASS", "variants", `${opts.variants}: ${r.variants.length} variants${opts.only ? `, ${variants.length} selected by --only` : ""}`);
    if (opts.only && !variants.length) add("FAIL", "variants", `--only ${opts.only} selects nothing`);
  }

  // Workload.
  const name = opts.workload ?? "full";
  const isFault = name.startsWith("faults-");
  const w = isFault ? faultsWorkload : H.workload(name, opts);
  if (command !== "gate") {
    if (!w) add("FAIL", "workload", `unknown --workload ${name}`);
    else add("PASS", "workload", `${w.name ?? name} (${w.kind}${w.app ? `, ${w.app} app` : ""}): ${w.description}`);
    if (H.WORKLOAD_ALIASES[name]) add("WARN", "workload", `--workload ${name} is the old name of ${H.WORKLOAD_ALIASES[name]}`);
    if (name === "soak" && !(Number(opts["soak-minutes"]) >= 6)) add("WARN", "workload", "--soak-minutes not given or < 6: one 6-minute block");
  }
  const app = isFault ? (w?.app ?? "fault") : "velasia";

  // Configurations and exclusions.
  const defaultConfigs = isFault ? (w?.defaultConfigs ?? ["256m:1"]).join(",") : "1g:2,512m:1,256m:1,256m:0.5";
  const configs = H.list(command === "gate" ? (opts["gate-configs"] ?? "1g:2") : opts.configs, defaultConfigs);
  const sfCpus = cpuset(L.LIMITS.sfCpuset);
  for (const c of configs) {
    const p = H.parseConfig(c);
    if (!p) add("FAIL", "configs", `"${c}": expected MEMORY:CPUS such as 256m:1`);
    else if (p.bytes < 64 * 2 ** 20) add("FAIL", "configs", `${c}: under 64 MiB`);
    else if (!(p.cpusNumber > 0)) add("FAIL", "configs", `${c}: CPUs must be > 0`);
    else if (sfCpus && p.cpusNumber > sfCpus.size) add("FAIL", "configs", `${c}: ${p.cpus} CPUs > the storefront cpuset ${L.LIMITS.sfCpuset}`);
  }
  if (!checks.some((c) => c.name === "configs")) add("PASS", "configs", configs.join(", "));
  if (allow.size && !(typeof opts["allow-reason"] === "string" && opts["allow-reason"].trim())) {
    add("FAIL", "configs", "--allow-config needs --allow-reason TEXT (recorded in meta)");
  }
  const plan = [];
  for (const v of variants) {
    const { run, excluded } = H.variantConfigs(v, configs, allow);
    plan.push({ variant: v, run, excluded });
    for (const e of excluded) add("INFO", "exclusion", `${v.label} skips ${e.config}: ${e.reason}`);
    for (const r of run.filter((x) => x.lifted)) add("WARN", "exclusion", `${v.label} runs ${r.config} although excluded (${r.lifted}): --allow-config, reason "${opts["allow-reason"]}"`);
  }

  // CPU sets.
  const sets = { storefront: L.LIMITS.sfCpuset, tools: L.LIMITS.toolsCpuset, api: L.LIMITS.apiCpuset };
  const parsed = Object.fromEntries(Object.entries(sets).map(([k, v]) => [k, cpuset(v)]));
  if (Object.values(parsed).some((s) => !s)) add("FAIL", "cpusets", JSON.stringify(sets));
  else {
    const overlap = [];
    const names = Object.keys(parsed);
    for (let i = 0; i < names.length; i++) for (let j = i + 1; j < names.length; j++) {
      if ([...parsed[names[i]]].some((c) => parsed[names[j]].has(c))) overlap.push(`${names[i]}/${names[j]}`);
    }
    // The harness runs on the Docker host: its CPU count is the host's.
    const n = os.availableParallelism?.() ?? os.cpus().length;
    const beyond = Object.entries(parsed).filter(([, s]) => [...s].some((c) => c >= n)).map(([k]) => k);
    const bad = [...overlap.map((o) => `overlap ${o}`), ...beyond.map((k) => `${k} uses a CPU >= ${n} (this host has 0-${n - 1}; set S1_${k === "storefront" ? "SF" : k.toUpperCase()}_CPUSET)`)];
    add(bad.length ? "FAIL" : "PASS", "cpusets", `storefront ${sets.storefront}, tools ${sets.tools}, API ${sets.api}${bad.length ? `: ${bad.join("; ")}` : ""}`);
  }

  // Binaries.
  const binInfo = new Map();
  for (const [label, path] of bins) {
    if (!isAbsolute(path)) add("FAIL", "bin", `${label}=${path}: not an absolute path`);
    else if (!existsSync(path) || !statSync(path).isFile()) add("FAIL", "bin", `${label}=${path}: no such file`);
    else if (!(statSync(path).mode & 0o111)) add("FAIL", "bin", `${label}=${path}: not executable`);
    else {
      const bad = elf(path);
      if (bad) add("FAIL", "bin", `${label}=${path}: ${bad}`);
      else {
        const sha = createHash("sha256").update(readFileSync(path)).digest("hex");
        binInfo.set(label, sha);
        add("PASS", "bin", `${label}=${path} sha256 ${sha}`);
      }
    }
  }
  if (opts.loadgen && opts.loadgen !== true) {
    const p = opts.loadgen;
    if (!isAbsolute(p) || !existsSync(p)) add("FAIL", "loadgen", `${p}: not an absolute path to a file`);
    else add(elf(p) ? "FAIL" : "PASS", "loadgen", `${p}${elf(p) ? `: ${elf(p)}` : ""}`);
  }

  // Runtimes -> images; flags -> binaries.
  const assumed = new Set(H.list(opts["images-built"], ""));
  const flagCache = new Map();
  const flagsFor = (ref) => {
    if (!flagCache.has(ref)) {
      try {
        flagCache.set(ref, { flags: startFlags(cliSource(ref)) });
      } catch (error) {
        flagCache.set(ref, { error: String(error.message).split("\n")[0] });
      }
    }
    return flagCache.get(ref);
  };
  const images = new Map();
  for (const { variant: v } of plan) {
    const image = variantImage(v, app);
    images.set(v.label, image);
    if (app === "fault" && v.image && !v.faultImage) {
      add("FAIL", "runtime", `${v.label}: has an image override (${v.image}) but no faultImage for the fault app`);
      continue;
    }
    if (v.runtime === "node") add("PASS", "runtime", `${v.label}: node -> ${image}`);
    else if (v.runtime === "rc2" && app === "velasia") add("PASS", "runtime", `${v.label}: rc2 -> ${image} (npm @nextrs/cli 0.1.0-rc.2)`);
    else if (bins.has(v.runtime)) add("PASS", "runtime", `${v.label}: ${v.runtime} -> ${image} (from --bin ${v.runtime})`);
    else if (assumed.has(v.runtime) || v.image || v.faultImage) add("WARN", "runtime", `${v.label}: ${v.runtime} -> ${image}, assumed built (not checked without --check-images)`);
    else add("FAIL", "runtime", `${v.label}: runtime ${v.runtime} has no --bin ${v.runtime}=PATH and is not node${app === "velasia" ? " or rc2" : ""}`);
    if (v.runtime === "node" || (v.runtime === "rc2" && app === "velasia")) continue;
    const ref = commits.get(v.runtime);
    const f = flagsFor(ref ?? "HEAD");
    if (f.error) {
      add("FAIL", "flags", `${v.label}: cannot read the CLI at ${ref ?? "HEAD"}: ${f.error}`);
      continue;
    }
    const problems = checkArgs(v.args ?? "", f.flags);
    const startMissing = (app === "fault" ? FAULT_START_FLAGS : VELASIA_START_FLAGS).filter((x) => !f.flags[x]);
    if (startMissing.length) problems.push(`the image's start script passes ${startMissing.join(" ")}, unknown to this binary`);
    const where = ref ? `commit ${ref}` : "HEAD (no --commit for this runtime: the binary may differ)";
    if (problems.length) add(ref ? "FAIL" : "WARN", "flags", `${v.label} against ${where}: ${problems.join("; ")}`);
    else add(ref ? "PASS" : "WARN", "flags", `${v.label}: "${v.args ?? ""}" valid at ${where}`);
  }

  // Images (read-only docker image inspect), only when asked.
  if (opts["check-images"]) {
    const need = new Set([...images.values()]);
    if (app === "velasia") for (const i of [L.IMAGES.node, L.IMAGES.api]) need.add(i);
    for (const image of need) {
      const r = L.sh("docker", ["image", "inspect", "-f", "{{.Id}}", image], { allowFail: true });
      if (r.error || r.status === null) add("FAIL", "image", `${image}: docker not available (${r.error?.code ?? "no status"})`);
      else add(r.status === 0 ? "PASS" : "FAIL", "image", `${image}${r.status === 0 ? ` ${r.stdout.trim()}` : ": not present"}`);
    }
  }

  // Output directory.
  if (!opts.out || opts.out === true) add("FAIL", "out", "--out DIR is required (absolute)");
  else if (!isAbsolute(opts.out)) add("FAIL", "out", `--out ${opts.out}: not absolute (the runbook cds between steps)`);
  else if (existsSync(opts.out) && readdirSync(opts.out).some((f) => /\.(json|jsonl)$/.test(f) && !f.startsWith("plan"))) {
    add("WARN", "out", `${opts.out} already holds results: new runs are added beside them (use a fresh directory per series step)`);
  } else add("PASS", "out", opts.out);

  // Correctness gate (profile and faults: before any timing).
  if (command === "profile") {
    if (opts["no-gate-reason"] && opts["no-gate-reason"] !== true) {
      add("WARN", "gate", `skipped: "${opts["no-gate-reason"]}" (recorded in meta; results are not decisive)`);
    } else {
      const problems = H.checkGate(opts.gate, variants);
      if (problems.length) for (const p of problems) add("FAIL", "gate", p);
      else add("PASS", "gate", `${opts.gate} covers every variant`);
      // The fault app is not what the gate captured: its images must at
      // least serve one build (verify-build --name fault).
      if (app === "fault") {
        const f = opts["fault-build"];
        let ok = false;
        try {
          ok = JSON.parse(readFileSync(f, "utf8")).passed === true;
        } catch {}
        if (ok) add("PASS", "gate", `fault-app build identity ${f}`);
        else add("FAIL", "gate", `--fault-build FILE (fault-build-identity.json from build-images.sh fault) missing or not passed: ${f ?? "not given"}`);
      }
    }
  }

  const fail = checks.some((c) => c.status === "FAIL");
  return { checks, variants, plan, configs, workload: w, app, images, binInfo, fail };
}

export function printChecks(checks, out = console.log) {
  for (const c of checks) out(`${c.status.padEnd(4)}  ${c.name.padEnd(9)}  ${c.detail}`);
  const n = (s) => checks.filter((c) => c.status === s).length;
  out(`\n${n("PASS")} pass, ${n("WARN")} warn, ${n("FAIL")} fail, ${n("INFO")} info`);
}
