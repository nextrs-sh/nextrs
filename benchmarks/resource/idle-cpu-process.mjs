// RE-5F: idle CPU and wakeups of a NextRS process, per thread, process
// level (not a container measurement; the container campaign keeps
// idle-threads.mjs). For each isolate count (0 = no executor) and traffic
// pattern, start NextRS on fixtures/next-app, let startup settle, then
// over --window seconds read every thread's CPU time (utime + stime,
// /proc/PID/task/TID/stat), the times it was scheduled (schedstat, field
// 3: each is a wakeup), and its context switches. The probe runs in this
// Node.js process, so its own CPU is not in the server's numbers; its
// requests' server-side cost is.
//
//   node benchmarks/resource/idle-cpu-process.mjs --bin target/re5-cand-rel/release/nextrs
//     [--isolates 0,1,2,4] [--traffic none,probe,low] [--settle 15] [--window 60]
//     [--args "..."] [--label L] [--out DIR]
//
// traffic: none; probe = GET /readyz on the metrics listener every 10 s
// (a container health check); low = also 1 request/s (an SSR page with an
// executor, a prerendered page without).
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, appendFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) {
  if (!argv[i].startsWith("--")) continue;
  const k = argv[i].slice(2);
  if (k === "args") { args[k] = argv[++i]; continue; }
  args[k] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
}
const ROOT = resolve(new URL("../..", import.meta.url).pathname);
const bin = resolve(args.bin ?? join(ROOT, "target/release/nextrs"));
const label = args.label ?? "nextrs";
const isolateCounts = String(args.isolates ?? "0,1,2,4").split(",").map(Number);
const traffics = String(args.traffic ?? "none,probe,low").split(",");
const settle = Number(args.settle ?? 15);
const windowS = Number(args.window ?? 60);
const extra = args.args ? String(args.args).split(" ").filter(Boolean) : [];
const out = resolve(args.out ?? join(ROOT, "engineering/performance/resource-efficiency/results/re5/idle-cpu"));
mkdirSync(out, { recursive: true });
const project = join(ROOT, "fixtures/next-app");
const TICK_MS = 10; // USER_HZ = 100
let port = 7900 + Math.floor(Math.random() * 300);

function threads(pid) {
  const out = {};
  for (const tid of readdirSync(`/proc/${pid}/task`)) {
    try {
      const comm = readFileSync(`/proc/${pid}/task/${tid}/comm`, "utf8").trim();
      const stat = readFileSync(`/proc/${pid}/task/${tid}/stat`, "utf8").split(") ")[1].split(" ");
      const sched = readFileSync(`/proc/${pid}/task/${tid}/schedstat`, "utf8").trim().split(" ");
      const status = readFileSync(`/proc/${pid}/task/${tid}/status`, "utf8");
      out[tid] = {
        comm,
        cpu_ticks: Number(stat[11]) + Number(stat[12]),
        run_ns: Number(sched[0]),
        slices: Number(sched[2]),
        vcsw: Number(/^voluntary_ctxt_switches:\s+(\d+)/m.exec(status)?.[1] ?? 0),
      };
    } catch {}
  }
  return out;
}

function delta(a, b, seconds) {
  const byName = {};
  let total = { cpu_ms: 0, run_ms: 0, wakeups: 0, vcsw: 0 };
  for (const [tid, t] of Object.entries(b)) {
    const s = a[tid] ?? { cpu_ticks: 0, run_ns: 0, slices: 0, vcsw: 0 };
    const d = {
      cpu_ms: (t.cpu_ticks - s.cpu_ticks) * TICK_MS,
      run_ms: (t.run_ns - s.run_ns) / 1e6,
      wakeups: t.slices - s.slices,
      vcsw: t.vcsw - s.vcsw,
    };
    // Tokio workers, V8 platform workers, isolates: one row per name.
    const name = t.comm.replace(/[-_ ]?\d+$/, "");
    const row = (byName[name] ??= { threads: 0, cpu_ms: 0, run_ms: 0, wakeups: 0, vcsw: 0 });
    row.threads++;
    for (const k of ["cpu_ms", "run_ms", "wakeups", "vcsw"]) { row[k] += d[k]; total[k] += d[k]; }
  }
  const per = (x) => +(x / seconds).toFixed(2);
  for (const row of Object.values(byName)) {
    row.wakeups_per_s = per(row.wakeups);
    row.run_ms = +row.run_ms.toFixed(1);
  }
  return {
    total: { ...total, run_ms: +total.run_ms.toFixed(1), millicores_run: per(total.run_ms), wakeups_per_s: per(total.wakeups), vcsw_per_s: per(total.vcsw) },
    threads: byName,
  };
}

async function hit(url) {
  try { const r = await fetch(url); await r.arrayBuffer(); return r.status; } catch { return 0; }
}

async function measure(isolates, traffic) {
  const p = port++, mp = port++;
  const runtime = isolates === 0 ? ["--js-runtime", "none", "--allow-unsupported"] : ["--js-runtime", "embedded-v8", "--isolates", String(isolates)];
  const child = spawn(bin, ["start", join(project, ".next"), "--port", String(p), "--metrics-addr", `127.0.0.1:${mp}`, ...runtime, ...extra], {
    cwd: project, env: { ...process.env, NODE_ENV: "production", RUST_LOG: "warn" }, stdio: ["ignore", "ignore", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", (c) => (stderr = (stderr + c).slice(-4000)));
  const page = isolates === 0 ? "/" : "/books/7";
  for (let i = 0; ; i++) {
    if ((await hit(`http://127.0.0.1:${p}${page}`)) === 200) break;
    if (child.exitCode !== null || i > 2400) throw new Error(`not ready: ${stderr}`);
    await sleep(25);
  }
  await sleep(settle * 1000);
  let stop = false, requests = 0, failed = 0;
  const loops = [];
  if (traffic !== "none") {
    loops.push((async () => {
      while (!stop) {
        if ((await hit(`http://127.0.0.1:${mp}/readyz`)) !== 200) failed++;
        requests++;
        await sleep(10_000);
      }
    })());
  }
  if (traffic === "low") {
    loops.push((async () => {
      while (!stop) {
        if ((await hit(`http://127.0.0.1:${p}${page}`)) !== 200) failed++;
        requests++;
        await sleep(1000);
      }
    })());
  }
  // Process-level CPU too (includes threads that exit in the window, such
  // as idle blocking-pool threads; review F13).
  const procCpu = () => {
    const f = readFileSync(`/proc/${child.pid}/stat`, "utf8").split(") ")[1].split(" ");
    return (Number(f[11]) + Number(f[12])) * TICK_MS;
  };
  const a = threads(child.pid);
  const pa = procCpu();
  const t0 = performance.now();
  await sleep(windowS * 1000);
  const b = threads(child.pid);
  const pb = procCpu();
  const seconds = (performance.now() - t0) / 1000;
  stop = true;
  child.kill("SIGTERM");
  await new Promise((r) => child.once("exit", r));
  await Promise.allSettled(loops);
  const d = delta(a, b, seconds);
  d.total.process_cpu_ms = pb - pa;
  const row = { label, isolates, traffic, window_s: +seconds.toFixed(1), requests, failed, ...d };
  appendFileSync(join(out, `idle-cpu-${label}.jsonl`), JSON.stringify(row) + "\n");
  const top = Object.entries(d.threads).sort((x, y) => y[1].wakeups - x[1].wakeups).slice(0, 4)
    .map(([n, r]) => `${n}×${r.threads}:${r.wakeups_per_s}/s`).join(" ");
  console.log(`${label.padEnd(10)} isolates ${isolates} ${traffic.padEnd(5)} run ${String(d.total.millicores_run).padStart(6)} mcore  cpu ${String(d.total.cpu_ms).padStart(5)} ms/${Math.round(seconds)}s  wakeups ${String(d.total.wakeups_per_s).padStart(7)}/s  vcsw ${d.total.vcsw_per_s}/s  req ${requests} fail ${failed} | ${top}`);
}

console.log(`bin ${bin} ${extra.join(" ")}`);
for (const isolates of isolateCounts) for (const traffic of traffics) await measure(isolates, traffic);
