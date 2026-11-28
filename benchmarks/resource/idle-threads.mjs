// Idle CPU per thread: where an idle server spends its CPU.
//
//   node benchmarks/resource/idle-threads.mjs --variants FILE [--only LABEL,...]
//        [--config 512m:1] [--settle 60] [--window 300] --out DIR
//
// Per variant: a fresh storefront (same stack and limits as run.mjs), no
// load at all after readiness (only the image's own health check, every
// 10 s), `--settle` seconds to let startup work finish, then the CPU time
// of every thread of the main process (utime + stime from
// /proc/PID/task/*/stat, read from the probe container, which shares the
// host PID namespace) and of the whole container (cgroup cpu.stat) at the
// start and end of a `--window`-second window. Writes DIR/<label>.json and
// prints a table of millicores per thread name (threads with the same name
// are summed: Tokio workers, V8 platform workers).
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import * as L from "../series1/lib.mjs";

const PROBE = process.env.RE_PROBE ?? "re-probe";
const argv = process.argv.slice(2);
const opts = {};
for (let i = 0; i < argv.length; i++) {
  const k = argv[i].replace(/^--/, "");
  const v = argv[i + 1];
  if (v === undefined || v.startsWith("--")) opts[k] = true;
  else (opts[k] = v), i++;
}
const log = (...a) => console.error(new Date().toISOString().slice(11, 19), ...a);
const TICK_US = 10_000; // USER_HZ = 100

function inspect(name) {
  const r = L.docker(["inspect", "-f", "{{.Id}} {{.State.Pid}}", name], { allowFail: true });
  if (r.status !== 0) return null;
  const [id, pid] = r.stdout.trim().split(" ");
  return { id, pid: Number(pid) };
}

function cgDir(id) {
  for (const d of [`docker/${id}`, `system.slice/docker-${id}.scope`]) {
    if (L.docker(["exec", PROBE, "test", "-d", `/cg/${d}`], { allowFail: true }).status === 0) return `/cg/${d}`;
  }
  throw new Error(`no cgroup directory for ${id}`);
}

/** Per-thread CPU (µs) of `pid`, keyed by tid, with the thread's name. */
function threads(pid) {
  const out = L.docker(["exec", PROBE, "sh", "-c", `for t in /proc/${pid}/task/*; do cat $t/stat 2>/dev/null; echo; done`]).stdout;
  const map = {};
  for (const line of out.split("\n")) {
    const open = line.indexOf("(");
    const close = line.lastIndexOf(")");
    if (open < 0 || close < 0) continue;
    const tid = line.slice(0, open).trim();
    const name = line.slice(open + 1, close);
    const f = line.slice(close + 2).split(" "); // f[0] = state (field 3)
    map[tid] = { name, us: (Number(f[11]) + Number(f[12])) * TICK_US };
  }
  return map;
}

function containerUsec(dir) {
  const stat = L.docker(["exec", PROBE, "cat", `${dir}/cpu.stat`]).stdout;
  return Number(/usage_usec (\d+)/.exec(stat)[1]);
}

const variants = JSON.parse(readFileSync(resolve(opts.variants), "utf8"))
  .filter((v) => !opts.only || String(opts.only).split(",").includes(v.label));
const [memory, cpus] = String(opts.config ?? "512m:1").split(":");
const settle = Number(opts.settle ?? 60);
const window = Number(opts.window ?? 300);
const out = resolve(opts.out);
mkdirSync(out, { recursive: true });

for (const variant of variants) {
  log(`${variant.label} ${memory}/${cpus}`);
  const ready = await L.storefrontUp(variant.runtime, {
    isolates: variant.isolates ?? 1, extraArgs: variant.args ?? "", env: variant.env ?? [], image: variant.image,
    memory, cpus, readyTimeoutMs: 120_000,
  });
  const info = inspect(L.SF);
  const dir = cgDir(info.id);
  await L.sleep(settle * 1000);
  const t0 = Date.now(), c0 = containerUsec(dir), th0 = threads(info.pid);
  await L.sleep(window * 1000);
  const t1 = Date.now(), c1 = containerUsec(dir), th1 = threads(info.pid);
  const seconds = (t1 - t0) / 1000;
  const byName = {};
  for (const [tid, t] of Object.entries(th1)) {
    const before = th0[tid]?.us ?? 0; // a thread started inside the window counts from 0
    const e = (byName[t.name] ??= { threads: 0, us: 0 });
    e.threads++;
    e.us += t.us - before;
  }
  const processUs = Object.values(byName).reduce((a, e) => a + e.us, 0);
  const result = {
    variant, memory, cpus, readyMs: ready.readyMs, settle, seconds,
    containerMillicores: (c1 - c0) / 1000 / seconds,
    processMillicores: processUs / 1000 / seconds,
    threads: Object.fromEntries(Object.entries(byName).sort((a, b) => b[1].us - a[1].us)
      .map(([name, e]) => [name, { threads: e.threads, millicores: e.us / 1000 / seconds }])),
    contention: L.contention(),
  };
  writeFileSync(join(out, `${variant.label}.json`), JSON.stringify(result, null, 1));
  L.remove(L.SF);
  console.log(`\n### ${variant.label}: container ${result.containerMillicores.toFixed(1)} millicores, main process ${result.processMillicores.toFixed(1)} (window ${seconds.toFixed(0)} s, ticks of 10 ms)\n`);
  console.log("| Thread | Count | Millicores |\n|---|---:|---:|");
  for (const [name, e] of Object.entries(result.threads)) {
    if (e.millicores >= 0.05) console.log(`| \`${name}\` | ${e.threads} | ${e.millicores.toFixed(2)} |`);
  }
}
