// Shared helpers for the Series 1 local reproduction (benchmarks/series1).
//
// Everything runs in Docker on one host: the Velasia Airways API, one
// storefront at a time (the runtime under test) and a tools container that
// runs the load generator and the HTTP scripts. The storefront, the tools
// and the API get disjoint CPU sets, so the load generator never competes
// with the server for a core. All three share one bridge network.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO = resolve(HERE, "../..");

export const NET = process.env.S1_NET ?? "nextrs-s1";
export const SUBNET = process.env.S1_SUBNET ?? "172.31.77.0/24";
export const API = "s1-api";
// Container names: overridable so that a second measurement lane (its own
// storefront and tools on other CPUs) can share the network and the API.
export const SF = process.env.S1_SF ?? "s1-sf";
export const TOOLS = process.env.S1_TOOLS ?? "s1-tools";

// Railway Series 1 gave each storefront 2 vCPU and 1 GB: the same here,
// pinned to two CPUs of the Docker VM so that runs see the same cores.
export const LIMITS = {
  sfCpus: process.env.S1_SF_CPUS ?? "2",
  sfCpuset: process.env.S1_SF_CPUSET ?? "2,3",
  sfMemory: process.env.S1_SF_MEMORY ?? "1g",
  toolsCpuset: process.env.S1_TOOLS_CPUSET ?? "4,5",
  apiCpuset: process.env.S1_API_CPUSET ?? "6",
};

// The application build every runtime serves: Velasia Airways at d04127a
// (the Series 1 commit; Next.js 16.4.6), built with its Dockerfile.node
// and Dockerfile.nextrs, whose identical build stages share one
// `next build` through the Docker layer cache (README.md: images). NextRS
// builds other than rc.2 are the NextRS image with only
// /usr/local/bin/nextrs replaced (`image` below), so every runtime serves
// byte-identical build output.
export const IMAGES = {
  api: process.env.S1_API_IMAGE ?? "velasia-api:s1",
  node: process.env.S1_NODE_IMAGE ?? "velasia-node:s1",
  nextrsBase: process.env.S1_NEXTRS_IMAGE ?? "velasia-nextrs:s1",
};

export function sh(cmd, args, { allowFail = false, input } = {}) {
  const r = spawnSync(cmd, args, { encoding: "utf8", input, maxBuffer: 256 << 20 });
  if (r.status !== 0 && !allowFail) {
    throw new Error(`${cmd} ${args.join(" ")} failed (${r.status}): ${r.stderr || r.stdout}`);
  }
  return r;
}

export const docker = (args, opts) => sh("docker", args, opts);

export function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

/** The image for a runtime label: `node`, `rc2`, or a NextRS binary label. */
export function image(runtime) {
  if (runtime === "node") return IMAGES.node;
  if (runtime === "rc2") return IMAGES.nextrsBase;
  return `nextrs-s1:${runtime}`;
}

/**
 * Builds `nextrs-s1:<label>`: the rc.2 Velasia image with `bin` as its
 * nextrs binary (and nothing else changed).
 */
export function buildRuntimeImage(label, bin) {
  const dir = dirname(resolve(bin));
  const file = resolve(bin).slice(dir.length + 1);
  const dockerfile = `FROM ${IMAGES.nextrsBase}\nCOPY --chmod=0755 ${file} /usr/local/bin/nextrs\n`;
  docker(["build", "-q", "-t", `nextrs-s1:${label}`, "-f", "-", dir], { input: dockerfile });
}

export function networkUp() {
  if (docker(["network", "inspect", NET], { allowFail: true }).status !== 0) {
    docker(["network", "create", "--subnet", SUBNET, NET]);
  }
}

export function remove(name) {
  docker(["rm", "-f", "-v", name], { allowFail: true });
}

const HARDENING = ["--cap-drop", "ALL", "--security-opt", "no-new-privileges:true"];

export async function apiUp() {
  if (running(API)) return;
  remove(API);
  docker(["volume", "create", "nextrs-s1-api-data"]);
  docker([
    "run", "-d", "--name", API, "--network", NET, ...HARDENING,
    "--cpuset-cpus", LIMITS.apiCpuset, "--cpus", "1", "--memory", "768m",
    "-e", "NODE_ENV=production", "-e", "RETAILING_API_HOST=0.0.0.0",
    "-e", "RETAILING_API_PORT=4010", "-e", "TALADB_PATH=/data/velasia.db",
    "-e", "ACCOUNT_REGISTRATION=disabled",
    "-v", "nextrs-s1-api-data:/data", IMAGES.api,
  ]);
  await waitHttp(`http://${API}:4010/health/ready`, 120_000, [200]);
}

/** The tools container: Node.js 24 (from the Node image) and the load generator. */
export function toolsUp(loadgen) {
  if (running(TOOLS)) return;
  remove(TOOLS);
  docker([
    "run", "-d", "--name", TOOLS, "--network", NET, ...HARDENING,
    "--cpuset-cpus", LIMITS.toolsCpuset,
    "-v", `${resolve(loadgen)}:/usr/local/bin/nextrs-loadgen:ro`,
    "-v", `${HERE}:/s1:ro`,
    "-v", "nextrs-s1-work:/work",
    "--user", "root",
    "--entrypoint", "sleep", IMAGES.node, "infinity",
  ]);
}

export function running(name) {
  const r = docker(["inspect", "-f", "{{.State.Running}}", name], { allowFail: true });
  return r.status === 0 && r.stdout.trim() === "true";
}

/** Runs `node /s1/<script>` in the tools container; returns stdout. */
export function toolsNode(script, args = []) {
  return docker(["exec", TOOLS, "node", `/s1/${script}`, ...args]).stdout;
}

export async function waitHttp(url, timeoutMs, statuses = [200]) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const r = docker(
      ["exec", TOOLS, "node", "-e",
        `fetch(${JSON.stringify(url)}).then(r=>{console.log(r.status)}).catch(()=>console.log(0))`],
      { allowFail: true },
    );
    if (statuses.includes(Number(r.stdout.trim()))) return;
    if (Date.now() > deadline) throw new Error(`${url} not ready`);
    await sleep(500);
  }
}

/**
 * Starts the storefront under test. `isolates` and `extraArgs` apply to
 * NextRS only (NEXTRS_ISOLATES, NEXTRS_EXTRA_ARGS of the image's start
 * script). NextRS's operations listener is opened on the network so the
 * tools container can read /metrics.
 *
 * `noJs` (NextRS only): `nextrs start` without an executor (no V8 isolate
 * at all), for the static-only memory baseline. The image's start script
 * always enables the executor, so nextrs is started directly; Velasia's
 * readiness route needs JavaScript (as do its prerendered pages and its
 * `robots.txt`, a route handler), so readiness is the first static asset
 * of the discovered manifest (`discover.mjs` must have run). Routes that
 * need JavaScript answer 501 there.
 *
 * `memory`, `cpus` and `cpuset` override the container limits (default
 * `LIMITS`); `env` adds `NAME=value` entries (allocator and runtime
 * experiments); `image` replaces the runtime's image; `readyTimeoutMs`
 * bounds the wait for readiness.
 */
export async function storefrontUp(runtime, {
  isolates = 1, extraArgs = "", noJs = false, memory = LIMITS.sfMemory, cpus = LIMITS.sfCpus,
  cpuset = LIMITS.sfCpuset, env: extraEnv = [], image: imageOverride, readyTimeoutMs = 180_000,
} = {}) {
  remove(SF);
  const limits = ["--cpus", cpus, "--cpuset-cpus", cpuset, "--memory", memory, "--memory-swap", memory];
  if (noJs) {
    const started = Date.now();
    docker([
      "run", "-d", "--name", SF, "--network", NET, ...HARDENING,
      ...limits,
      "-e", "NEXTRS_LOG_FORMAT=json", ...extraEnv.flatMap((e) => ["-e", e]),
      "--entrypoint", "/usr/local/bin/nextrs", imageOverride ?? image(runtime),
      "start", "/app/apps/web", "--host", "0.0.0.0", "--port", "8080",
      "--metrics-addr", "0.0.0.0:9100", "--allow-unsupported",
    ]);
    const manifest = JSON.parse(docker(["exec", TOOLS, "cat", "/work/specs/manifest.json"]).stdout);
    await waitHttp(`http://${SF}:8080${manifest.assets[0].path}`, 120_000, [200]);
    return { readyMs: Date.now() - started };
  }
  const env = [
    "NODE_ENV=production", "PORT=8080",
    `RETAILING_API_URL=http://${API}:4010`, "RETAILING_API_TIMEOUT_MS=5000",
    "SITE_URL=http://localhost:8080",
  ];
  if (runtime !== "node") {
    env.push(
      `NEXTRS_NET_ALLOW_ADDR=${SUBNET}`,
      `NEXTRS_ISOLATES=${isolates}`,
      "NEXTRS_METRICS_ADDR=0.0.0.0:9100",
    );
    if (extraArgs) env.push(`NEXTRS_EXTRA_ARGS=${extraArgs}`);
  }
  env.push(...extraEnv);
  const started = Date.now();
  docker([
    "run", "-d", "--name", SF, "--network", NET, ...HARDENING, ...limits,
    ...env.flatMap((e) => ["-e", e]),
    imageOverride ?? image(runtime),
  ]);
  await waitHttp(`http://${SF}:8080/api/internal/health/ready`, readyTimeoutMs, [200]);
  return { readyMs: Date.now() - started };
}

/**
 * One reading of the storefront container: its cgroup (CPU and memory,
 * the whole container) and its main process (PID 1 in both images: Node.js
 * runs `node apps/web/server.js`, NextRS `exec`s nextrs).
 */
export function sample() {
  const files = [
    "/sys/fs/cgroup/cpu.stat", "/sys/fs/cgroup/memory.current",
    "/sys/fs/cgroup/memory.peak", "/sys/fs/cgroup/memory.stat", "/proc/1/status",
  ];
  const r = docker(
    ["exec", SF, "sh", "-c", files.map((f) => `echo "== ${f}"; cat ${f} 2>/dev/null`).join("; ")],
    { allowFail: true },
  );
  const out = { t: Date.now() };
  let section = "";
  for (const line of r.stdout.split("\n")) {
    if (line.startsWith("== ")) {
      section = line.slice(3).trim();
      continue;
    }
    if (!line.trim()) continue;
    if (section.endsWith("memory.current")) out.memory_current = Number(line);
    else if (section.endsWith("memory.peak")) out.memory_peak = Number(line);
    else if (section.endsWith("cpu.stat") || section.endsWith("memory.stat")) {
      const [k, v] = line.split(/\s+/);
      const keep = ["usage_usec", "user_usec", "system_usec", "nr_periods", "nr_throttled",
        "throttled_usec", "anon", "file", "kernel", "sock", "shmem", "file_mapped", "slab"];
      if (keep.includes(k)) out[(section.endsWith("cpu.stat") ? "cpu_" : "mem_") + k] = Number(v);
    } else if (section === "/proc/1/status") {
      const m = line.match(/^(VmRSS|VmHWM|RssAnon|RssFile|RssShmem|Threads|voluntary_ctxt_switches|nonvoluntary_ctxt_switches):\s+(\d+)/);
      if (m) out[`proc_${m[1]}`] = Number(m[2]) * (m[1].startsWith("V") || m[1].startsWith("R") ? 1024 : 1);
    }
  }
  return out;
}

/** NextRS's /metrics.json (null for Node.js). */
export function nextrsMetrics(runtime) {
  if (runtime === "node") return null;
  const r = docker(
    ["exec", TOOLS, "node", "-e",
      `fetch("http://${SF}:9100/metrics.json").then(r=>r.text()).then(t=>process.stdout.write(t)).catch(()=>{})`],
    { allowFail: true },
  );
  try {
    return JSON.parse(r.stdout);
  } catch {
    return null;
  }
}

/** Runs the load generator in the tools container; returns its JSON output. */
export function loadgen(spec, { rate = 0, connections = 8, duration = 30, warmup = 0, requests = 0, threads = 2 } = {}) {
  const r = docker([
    "exec", TOOLS, "nextrs-loadgen", "--target", `${SF}:8080`, "--spec", spec,
    "--rate", String(rate), "--connections", String(connections),
    "--duration", String(duration), "--warmup", String(warmup),
    "--requests", String(requests), "--threads", String(threads),
  ]);
  return JSON.parse(r.stdout);
}

export function gitCommit() {
  try {
    return execFileSync("git", ["-C", REPO, "rev-parse", "--short", "HEAD"], { encoding: "utf8" }).trim();
  } catch {
    return "unknown";
  }
}

export function hostInfo() {
  const info = (args) => docker(args, { allowFail: true }).stdout.trim();
  return {
    docker: info(["version", "--format", "{{.Server.Version}}"]),
    dockerCpus: info(["info", "--format", "{{.NCPU}}"]),
    dockerMemory: info(["info", "--format", "{{.MemTotal}}"]),
    kernel: info(["info", "--format", "{{.KernelVersion}}"]),
    // OS release and architecture only: no host name.
    host: sh("uname", ["-srm"], { allowFail: true }).stdout.trim(),
    hostLoad: sh("uptime", [], { allowFail: true }).stdout.trim(),
  };
}

/**
 * Contention around a measurement: the macOS host's load average and the
 * Docker VM's (`/proc/loadavg` is not namespaced) plus the VM's CPU
 * counters (`/proc/stat` cpu line, in USER_HZ ticks), read from the tools
 * container.
 */
export function contention() {
  const host = sh("sysctl", ["-n", "vm.loadavg"], { allowFail: true }).stdout.trim();
  const vm = docker(["exec", TOOLS, "sh", "-c", "cat /proc/loadavg; head -1 /proc/stat"], { allowFail: true }).stdout.split("\n");
  const ticks = (vm[1] ?? "").trim().split(/\s+/).slice(1).map(Number);
  return {
    hostLoad1: Number(host.replace(/[{}]/g, "").trim().split(/\s+/)[0]),
    vmLoad1: Number((vm[0] ?? "").split(" ")[0]),
    vmBusyTicks: ticks.length ? ticks.reduce((a, b) => a + b, 0) - (ticks[3] ?? 0) - (ticks[4] ?? 0) : null,
    vmTotalTicks: ticks.length ? ticks.reduce((a, b) => a + b, 0) : null,
    // Time the hypervisor ran something else while a VM CPU wanted to run.
    vmStealTicks: ticks.length > 7 ? ticks[7] : null,
  };
}

export function exists(path) {
  return existsSync(path);
}

export { join };
