// Phase 3A benchmark harness: process control, resource sampling,
// environment capture and statistics. Stdlib only.
import http from "node:http";
import { spawn, execFileSync } from "node:child_process";
import { readFileSync, readdirSync, existsSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";
import os from "node:os";

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const sh = (cmd, args, opts = {}) => {
  try {
    return execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], ...opts }).trim();
  } catch (e) {
    return null;
  }
};

// ---------------------------------------------------------------- HTTP

export function request(port, { method = "GET", path = "/", headers = {}, body } = {}) {
  return new Promise((resolve) => {
    const req = http.request({ host: "127.0.0.1", port, method, path, headers: { "accept-encoding": "identity", ...headers }, agent: false }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      res.on("error", () => resolve({ status: 0, headers: {}, body: Buffer.alloc(0) }));
    });
    req.setTimeout(30000, () => req.destroy());
    req.on("error", () => resolve({ status: 0, headers: {}, body: Buffer.alloc(0) }));
    if (body) req.write(body);
    req.end();
  });
}

// ---------------------------------------------------------------- /proc

function childrenMap() {
  const children = new Map();
  for (const d of readdirSync("/proc")) {
    if (!/^\d+$/.test(d)) continue;
    try {
      const f = readFileSync(`/proc/${d}/stat`, "utf8").split(") ")[1].split(" ");
      const ppid = Number(f[1]);
      if (!children.has(ppid)) children.set(ppid, []);
      children.get(ppid).push(Number(d));
    } catch {}
  }
  return children;
}

/** The process and all of its descendants. */
export function tree(pid) {
  const children = childrenMap();
  const out = [];
  const walk = (p) => {
    out.push(p);
    for (const c of children.get(p) ?? []) walk(c);
  };
  walk(pid);
  return out;
}

const CLK = Number(sh("getconf", ["CLK_TCK"]) ?? 100);

/** CPU seconds (user, system) and memory of a process tree. */
export function sample(pid) {
  let user = 0, system = 0, rss = 0, anon = 0, file = 0, swap = 0, threads = 0;
  for (const p of tree(pid)) {
    try {
      const f = readFileSync(`/proc/${p}/stat`, "utf8").split(") ")[1].split(" ");
      user += Number(f[11]) / CLK;
      system += Number(f[12]) / CLK;
      const st = readFileSync(`/proc/${p}/status`, "utf8");
      const kb = (k) => Number(new RegExp(`${k}:\\s+(\\d+)`).exec(st)?.[1] ?? 0);
      rss += kb("VmRSS");
      anon += kb("RssAnon");
      file += kb("RssFile");
      swap += kb("VmSwap");
      threads += kb("Threads");
    } catch {}
  }
  const mib = (kb) => +(kb / 1024).toFixed(1);
  return { cpu_user_s: user, cpu_sys_s: system, rss_mib: mib(rss), anon_mib: mib(anon), file_mib: mib(file), swap_mib: mib(swap), threads };
}

/** Samples memory every `ms` until stopped; returns peaks and the last sample. */
export function memorySampler(pid, ms = 250) {
  let peak = { rss_mib: 0, anon_mib: 0, swap_mib: 0 };
  let last = null;
  const t = setInterval(() => {
    last = sample(pid);
    for (const k of Object.keys(peak)) peak[k] = Math.max(peak[k], last[k]);
  }, ms);
  return () => {
    clearInterval(t);
    return { peak, last };
  };
}

/** perf stat (user-space instructions and cycles) over a process tree. */
export function perfStat(pid) {
  const pids = tree(pid).join(",");
  const child = spawn("perf", ["stat", "-x", ",", "-e", "instructions:u,cycles:u", "-p", pids], { stdio: ["ignore", "ignore", "pipe"] });
  let err = "";
  child.stderr.on("data", (c) => (err += c));
  return async () => {
    child.kill("SIGINT");
    await new Promise((r) => child.on("close", r));
    const out = {};
    for (const line of err.split("\n")) {
      const f = line.split(",");
      if (f.length > 3 && /instructions|cycles/.test(f[2])) out[f[2].replace(":u", "")] = Number(f[0]) || null;
    }
    return out;
  };
}

// ---------------------------------------------------------------- servers

export async function waitReady(port, path = "/", timeoutMs = 60000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const r = await request(port, { path });
    if (r.status) return Date.now() - t0;
    await sleep(100);
  }
  throw new Error(`server on ${port} did not start`);
}

/**
 * Starts a server pinned to `cpus` (taskset list). kind: "next" or "nextrs".
 * Returns { child, pid, port, metricsPort, startup_ms }.
 */
export async function startServer({ kind, node, nextrs, project, dist = ".next", port, metricsPort, isolates = 1, cpus, env = {}, extraArgs = [], readyPath = "/" }) {
  let cmd, args;
  const fullEnv = { ...process.env, NODE_ENV: "production", NEXT_TELEMETRY_DISABLED: "1", ...env };
  if (kind === "next") {
    cmd = node;
    args = [join(project, "node_modules/next/dist/bin/next"), "start", "-H", "0.0.0.0", "-p", String(port)];
  } else {
    cmd = nextrs;
    args = ["start", join(project, dist), "--port", String(port), "--js-runtime", "embedded-v8", "--isolates", String(isolates), "--metrics-addr", `127.0.0.1:${metricsPort}`, ...extraArgs];
  }
  const t0 = Date.now();
  const child = spawn("taskset", ["-c", cpus, cmd, ...args], { cwd: project, env: fullEnv, stdio: ["ignore", "ignore", "pipe"], detached: true });
  let stderr = "";
  child.stderr.on("data", (c) => (stderr = (stderr + c).slice(-4000)));
  try {
    await waitReady(port, readyPath);
  } catch (e) {
    try { process.kill(-child.pid, "SIGKILL"); } catch {}
    throw new Error(`${kind} failed to start: ${stderr}`);
  }
  return { child, pid: child.pid, port, metricsPort: kind === "nextrs" ? metricsPort : null, startup_ms: Date.now() - t0, stderr: () => stderr };
}

export async function stopServer(s) {
  try { process.kill(-s.pid, "SIGTERM"); } catch {}
  for (let i = 0; i < 100; i++) {
    if (!existsSync(`/proc/${s.pid}`)) break;
    try { if (readFileSync(`/proc/${s.pid}/stat`, "utf8").includes(") Z ")) break; } catch { break; }
    await sleep(100);
  }
  try { process.kill(-s.pid, "SIGKILL"); } catch {}
  // The port must be free before the next server binds it.
  for (let i = 0; i < 50 && (await request(s.port, { path: "/" })).status; i++) await sleep(100);
}

export async function metrics(port) {
  if (!port) return null;
  const r = await request(port, { path: "/metrics.json" });
  try { return JSON.parse(r.body.toString()); } catch { return null; }
}

// ---------------------------------------------------------------- load

/** Runs the load generator; returns its parsed JSON output. */
export function loadgen(bin, { cpus, target, spec, connections, rate = 0, duration, warmup = 0, requests = 0, threads = 2 }) {
  return new Promise((resolve, reject) => {
    const args = ["-c", cpus, bin, "--target", target, "--spec", spec, "--connections", String(connections), "--rate", String(rate), "--duration", String(duration), "--warmup", String(warmup), "--requests", String(requests), "--threads", String(threads)];
    const child = spawn("taskset", args, { stdio: ["ignore", "pipe", "pipe"] });
    let out = "", err = "";
    child.stdout.on("data", (c) => (out += c));
    child.stderr.on("data", (c) => (err += c));
    child.on("close", (code) => {
      if (code !== 0) return reject(new Error(`loadgen exit ${code}: ${err}`));
      try { resolve(JSON.parse(out)); } catch (e) { reject(e); }
    });
  });
}

// ---------------------------------------------------------------- environment

export function fixtureIdentity(project, dist = ".next") {
  const d = join(project, dist);
  const h = createHash("sha256");
  const files = [];
  for (const f of ["BUILD_ID", "build-manifest.json", "routes-manifest.json", "prerender-manifest.json", "app-path-routes-manifest.json", "server/middleware-manifest.json", "server/app-paths-manifest.json", "server/pages-manifest.json", "required-server-files.json"]) {
    const p = join(d, f);
    if (existsSync(p)) {
      h.update(f).update(readFileSync(p));
      files.push(f);
    }
  }
  let next = null;
  try { next = JSON.parse(readFileSync(join(project, "node_modules/next/package.json"), "utf8")).version; } catch {}
  return { project, dist, build_id: existsSync(join(d, "BUILD_ID")) ? readFileSync(join(d, "BUILD_ID"), "utf8").trim() : null, manifests_sha256: h.digest("hex"), manifests: files, next_version: next };
}

export function environment({ node, nextrs, root, loadgenBin, serverCpus, loadCpus }) {
  const cpu = os.cpus();
  const meminfo = readFileSync("/proc/meminfo", "utf8");
  const kb = (k) => Number(new RegExp(`${k}:\\s+(\\d+)`).exec(meminfo)?.[1] ?? 0);
  const governors = [...new Set(cpu.map((_, i) => sh("cat", [`/sys/devices/system/cpu/cpu${i}/cpufreq/scaling_governor`])))];
  return {
    date: new Date().toISOString(),
    host_kind: "single-host (load generator and server on the same machine, pinned to disjoint CPUs)",
    cpu_model: cpu[0]?.model,
    logical_cpus: cpu.length,
    cpu_governors: governors,
    cpu_max_mhz: Number(sh("cat", ["/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq"]) ?? 0) / 1000,
    turbo_disabled: sh("cat", ["/sys/devices/system/cpu/intel_pstate/no_turbo"]),
    mem_total_mib: Math.round(kb("MemTotal") / 1024),
    mem_available_mib_at_start: Math.round(kb("MemAvailable") / 1024),
    swap_total_mib: Math.round(kb("SwapTotal") / 1024),
    swap_used_mib_at_start: Math.round((kb("SwapTotal") - kb("SwapFree")) / 1024),
    loadavg_at_start: os.loadavg(),
    os: sh("sh", ["-c", ". /etc/os-release && echo \"$PRETTY_NAME\""]),
    kernel: os.release(),
    rustc: sh(join(os.homedir(), ".cargo/bin/rustc"), ["-vV"]),
    node: sh(node, ["--version"]),
    nextrs_binary: nextrs,
    nextrs_binary_sha256: createHash("sha256").update(readFileSync(nextrs)).digest("hex"),
    nextrs_commit: sh("git", ["-C", root, "rev-parse", "HEAD"]),
    nextrs_dirty: (sh("git", ["-C", root, "status", "--porcelain", "--untracked-files=no"]) ?? "") !== "",
    loadgen: loadgenBin,
    loadgen_sha256: createHash("sha256").update(readFileSync(loadgenBin)).digest("hex"),
    server_cpus: serverCpus,
    load_cpus: loadCpus,
    memory_limit: "none (no cgroup limit applied); RSS is reported per process tree",
  };
}

// ---------------------------------------------------------------- statistics

export function median(xs) {
  const s = [...xs].sort((a, b) => a - b);
  const n = s.length;
  return n === 0 ? null : n % 2 ? s[(n - 1) / 2] : (s[n / 2 - 1] + s[n / 2]) / 2;
}

// Deterministic PRNG so reported intervals are reproducible from raw data.
export function rng(seed) {
  let s = seed >>> 0;
  return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
}

/** 95% percentile-bootstrap interval of the median. */
export function bootstrapMedianCI(xs, iters = 2000, seed = 42) {
  if (xs.length < 2) return [xs[0] ?? null, xs[0] ?? null];
  const r = rng(seed);
  const meds = [];
  for (let i = 0; i < iters; i++) {
    const s = xs.map(() => xs[Math.floor(r() * xs.length)]);
    meds.push(median(s));
  }
  meds.sort((a, b) => a - b);
  return [meds[Math.floor(0.025 * iters)], meds[Math.floor(0.975 * iters)]];
}

/**
 * Ratio candidate/baseline of medians with a bootstrap interval over the
 * paired rounds. A difference is "significant" only when the interval
 * excludes 1.
 */
export function ratioCI(base, cand, iters = 2000, seed = 7) {
  const r = rng(seed);
  const vals = [];
  for (let i = 0; i < iters; i++) {
    const b = base.map(() => base[Math.floor(r() * base.length)]);
    const c = cand.map(() => cand[Math.floor(r() * cand.length)]);
    vals.push(median(c) / median(b));
  }
  vals.sort((a, b) => a - b);
  const ratio = median(cand) / median(base);
  const lo = vals[Math.floor(0.025 * iters)], hi = vals[Math.floor(0.975 * iters)];
  return { ratio, ci95: [lo, hi], significant: lo > 1 || hi < 1 };
}

export function shuffle(xs, r) {
  const a = [...xs];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(r() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}
