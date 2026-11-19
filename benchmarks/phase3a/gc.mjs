// V8 garbage collection per workload (Phase 3A): how much JavaScript garbage
// a request makes and what collecting it costs, per isolate, for NextRS at
// 1 and 4 isolates and with V8 young-generation (semi-space) settings.
//
//   node benchmarks/phase3a/gc.mjs [--nextrs target/release/nextrs]
//     [--isolates 1,4] [--semi default,16,32] [--only ssr,rsc,...]
//     [--rounds 2] [--duration 10] [--warmup 5] [--connections 32]
//     [--out benchmarks/results/phase3a/gc-<name>]
//
// The server runs with V8's --trace-gc-nvp (one line per collection on
// stdout, with its pause, type and the bytes allocated since the previous
// collection) and, for a semi-space setting N, --max-semi-space-size=N and
// --min-semi-space-size=N. Per workload and configuration: warm-up, then a
// measured closed-loop window during which every collection is attributed
// to its isolate. Reported per request: young-generation allocation
// (bytes allocated between collections), minor and major collections and
// their main-thread pauses, CPU; per isolate: heap used after GC, heap
// committed, external memory (executor metrics); per process: RSS and
// anonymous RSS (peak and at the end). Every response is checked by the
// load generator. The tracing itself costs a little (one line per GC):
// compare configurations within one run only.
import { mkdirSync, writeFileSync, appendFileSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import * as L from "./lib.mjs";
import { groups as allGroups } from "./workloads.mjs";

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) if (argv[i].startsWith("--")) args[argv[i].slice(2)] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
const ROOT = resolve(new URL("../..", import.meta.url).pathname);
const out = resolve(args.out ?? join(ROOT, "benchmarks/results/phase3a/gc"));
mkdirSync(join(out, "specs"), { recursive: true });
const bin = resolve(args.nextrs ?? join(ROOT, "target/release/nextrs"));
const loadgenBin = join(ROOT, "benchmarks/loadgen/target/release/nextrs-loadgen");
const isolateCounts = String(args.isolates ?? "1,4").split(",").map(Number);
const semis = String(args.semi ?? "default,16").split(",");
const rounds = Number(args.rounds ?? 2);
const duration = Number(args.duration ?? 10);
const warmup = Number(args.warmup ?? 5);
// Phase 3C: open loop at this many requests per second (0: closed loop).
const rate = Number(args.rate ?? 0);
const only = new Set(String(args.only ?? "ssr,rsc,isr-hit,action,pages-ssr,jwt-valid,mixed").split(","));
const r = L.rng(Number(args.seed ?? 3));
const log = (...m) => console.log(new Date().toISOString().slice(11, 19), ...m);
const groups = allGroups.map((g) => ({ ...g, workloads: g.workloads.filter((w) => only.has(w.id)) })).filter((g) => g.workloads.length);
const configs = [];
for (const n of isolateCounts) for (const semi of semis) configs.push({ id: `${semi === "default" ? "default" : `semi${semi}`}-i${n}`, isolates: n, semi });
const env = L.environment({ node: process.execPath, nextrs: bin, root: ROOT, loadgenBin, serverCpus: "0-3", loadCpus: "4-5" });
env.parameters = { rounds, duration, warmup, configs: configs.map((c) => c.id), workloads: [...only] };
writeFileSync(join(out, "environment.json"), JSON.stringify(env, null, 2).replaceAll(`${ROOT}/`, ""));

// One --trace-gc-nvp line (V8 14): "[pid:0xISOLATE:0]  12345 ms: GC: {json}",
// the JSON carrying pause (ms), gc (type), allocated and promoted (bytes).
function parseGc(line) {
  const m = /^\[\d+:(0x[0-9a-f]+)(?::\d+)?\]\s+[\d.]+ ms: GC: (\{.*\})\s*$/.exec(line);
  if (!m) return null;
  let j;
  try { j = JSON.parse(m[2]); } catch { return null; }
  return { isolate: m[1], type: j.gc, pause: Number(j.pause) || 0, allocated: Number(j.allocated) || 0, promoted: Number(j.promoted) || 0 };
}
const MINOR = new Set(["s", "mmc", "ms"]); // scavenge, minor mark-compact/sweep

let port = 6900;
const rowsFile = join(out, "rows.jsonl");
for (let round = 1; round <= rounds; round++) {
  for (const group of groups) {
    for (const config of L.shuffle(configs, r)) {
      const ctx = { group, config: { id: config.id, kind: "nextrs", isolates: config.isolates }, port: port++, root: ROOT, node: process.execPath, scratch: join(out, "specs"), round };
      const metricsPort = port++;
      ctx.services = group.services ? await group.services(ctx) : null;
      const flags = ["--trace-gc-nvp", ...(config.semi === "default" ? [] : [`--max-semi-space-size=${config.semi}`, `--min-semi-space-size=${config.semi}`])];
      log(`round ${round}/${rounds}`, group.id, config.id);
      const s = await L.startServer({
        kind: "nextrs", nextrs: bin, project: join(ROOT, group.fixture), dist: group.dist ?? ".next", port: ctx.port, metricsPort, isolates: config.isolates, cpus: "0-3",
        env: group.env?.(ctx) ?? {}, extraArgs: [...(group.nextrsArgs?.(ctx) ?? []), "--v8-flags", flags.join(" ")], readyPath: group.readyPath ?? "/", stdout: "pipe",
      });
      ctx.server = s;
      // Always drain stdout (a full pipe would block the server).
      let collecting = false;
      let pending = "";
      let events = [];
      s.child.stdout.on("data", (c) => {
        pending += c;
        const lines = pending.split("\n");
        pending = lines.pop();
        if (!collecting) return;
        for (const line of lines) {
          const e = parseGc(line);
          if (e) events.push(e);
        }
      });
      try {
        await group.setup?.(ctx);
        for (const w of group.workloads) {
          const spec = typeof w.spec === "function" ? await w.spec(ctx) : w.spec;
          const specPath = join(out, "specs", `${group.id}.${w.id}.${config.id}.r${round}.json`);
          writeFileSync(specPath, JSON.stringify(spec));
          const connections = w.connections ?? Number(args.connections ?? 32);
          const lg = (o) => L.loadgen(loadgenBin, { cpus: "4-5", target: `127.0.0.1:${ctx.port}`, spec: specPath, connections, ...o });
          await lg({ duration: warmup, warmup: 0, ...(rate ? { rate } : {}) });
          await w.beforeMeasure?.(ctx);
          const m0 = await L.metrics(metricsPort);
          const c0 = L.sample(s.pid);
          const stopMem = L.memorySampler(s.pid);
          events = [];
          collecting = true;
          const res = await lg({ duration, warmup: 0, ...(rate ? { rate } : {}) });
          await L.sleep(200); // let the last lines of the window arrive
          collecting = false;
          const mem = stopMem();
          const c1 = L.sample(s.pid);
          const m1 = await L.metrics(metricsPort);
          const post = w.postcheck ? await w.postcheck(ctx, res) : null;
          const n = res.completed || 1;
          const per = {};
          for (const e of events) {
            const p = (per[e.isolate] ??= { minor: 0, major: 0, minor_pause_ms: 0, major_pause_ms: 0, allocated: 0, promoted: 0 });
            if (MINOR.has(e.type)) {
              p.minor++;
              p.minor_pause_ms += e.pause;
            } else {
              p.major++;
              p.major_pause_ms += e.pause;
            }
            p.allocated += e.allocated;
            p.promoted += e.promoted;
          }
          const sum = (k) => Object.values(per).reduce((a, p) => a + p[k], 0);
          const cpu = c1.cpu_user_s + c1.cpu_sys_s - (c0.cpu_user_s + c0.cpu_sys_s);
          const mib = (x) => +(x / 1048576).toFixed(1);
          const row = {
            round, group: group.id, workload: w.id, config: config.id, isolates: config.isolates, semi: config.semi,
            valid: res.ok === res.completed && res.io_errors === 0 && res.timeouts === 0 && (!post || post.ok),
            rps: res.achieved_rps, completed: res.completed, p95_ms: res.latency?.p95_ms, p99_ms: res.latency?.p99_ms,
            cpu_ms_per_req: (cpu * 1000) / n,
            gc_events: events.length,
            young_alloc_kib_per_req: +(sum("allocated") / 1024 / n).toFixed(1),
            promoted_kib_per_req: +(sum("promoted") / 1024 / n).toFixed(2),
            minor_gcs_per_1k_req: +((sum("minor") * 1000) / n).toFixed(2),
            major_gcs_per_1k_req: +((sum("major") * 1000) / n).toFixed(3),
            minor_pause_ms_per_req: +(sum("minor_pause_ms") / n).toFixed(4),
            major_pause_ms_per_req: +(sum("major_pause_ms") / n).toFixed(4),
            gc_pause_share_of_cpu: cpu ? +(((sum("minor_pause_ms") + sum("major_pause_ms")) / 1000 / cpu) * 100).toFixed(1) : null,
            mean_minor_pause_ms: sum("minor") ? +(sum("minor_pause_ms") / sum("minor")).toFixed(3) : null,
            max_pause_ms: events.length ? +Math.max(...events.map((e) => e.pause)).toFixed(2) : 0,
            pauses_over_2ms: events.filter((e) => e.pause > 2).length,
            pauses_over_5ms: events.filter((e) => e.pause > 5).length,
            major_types: [...new Set(events.filter((e) => !MINOR.has(e.type)).map((e) => e.type))],
            p50_ms: res.latency?.p50_ms,
            per_isolate: Object.values(per).map((p) => ({ ...p, allocated_mib: mib(p.allocated) })),
            heap: (m1?.executors ?? []).map((e) => ({ used_after_gc_mib: mib(e.heap_used_after_gc_bytes), total_mib: mib(e.heap_total_bytes), external_mib: mib(e.external_bytes) })),
            rss_peak_mib: mem.peak.rss_mib, anon_peak_mib: mem.peak.anon_mib, rss_end_mib: c1.rss_mib, anon_end_mib: c1.anon_mib,
            anon_per_isolate_mib: +(c1.anon_mib / config.isolates).toFixed(1),
            executor_requests: m1 && m0 ? m1.requests_completed - m0.requests_completed : null,
          };
          appendFileSync(rowsFile, JSON.stringify(row) + "\n");
          log(`  ${w.id.padEnd(14)} ${row.valid ? "ok" : "INVALID"} ${String(Math.round(row.rps)).padStart(6)} rps cpu ${row.cpu_ms_per_req.toFixed(3)} ms young ${row.young_alloc_kib_per_req} KiB/req minor ${row.minor_gcs_per_1k_req}/1k (${row.mean_minor_pause_ms} ms) major ${row.major_gcs_per_1k_req}/1k pause ${row.gc_pause_share_of_cpu}% of CPU anon ${row.anon_end_mib} MiB`);
        }
      } finally {
        await L.stopServer(s);
        await ctx.services?.stop?.();
      }
    }
  }
}

// ---- summary: medians over rounds ----
const rows = readFileSync(rowsFile, "utf8").trim().split("\n").map((l) => JSON.parse(l)).filter((x) => x.valid);
const keys = ["rps", "cpu_ms_per_req", "p99_ms", "young_alloc_kib_per_req", "minor_gcs_per_1k_req", "mean_minor_pause_ms", "major_gcs_per_1k_req", "minor_pause_ms_per_req", "gc_pause_share_of_cpu", "anon_end_mib", "anon_per_isolate_mib"];
const md = [`# V8 GC per workload — ${out.replace(`${ROOT}/`, "")}`, "", `Medians of ${rounds} rounds (valid rows only). Closed loop, ${args.connections ?? 32} connections, ${duration} s windows. \`--trace-gc-nvp\` on in every configuration.`, "",
  `| Workload | config | ${keys.join(" | ")} | heap after GC MiB (per isolate) | committed MiB |`, `|---|---|${keys.map(() => "---:").join("|")}|---|---|`];
const by = new Map();
for (const x of rows) {
  const k = `${x.group}/${x.workload}|${x.config}`;
  if (!by.has(k)) by.set(k, []);
  by.get(k).push(x);
}
for (const [k, xs] of [...by].sort()) {
  const [wl, cfg] = k.split("|");
  const med = (f) => L.median(xs.map(f).filter((v) => v !== null && v !== undefined));
  const fmt = (v) => (v === null || v === undefined || Number.isNaN(v) ? "–" : Math.abs(v) >= 100 ? Math.round(v) : +v.toFixed(3));
  const heap = xs.at(-1).heap.map((h) => h.used_after_gc_mib).join("/");
  const total = xs.at(-1).heap.map((h) => h.total_mib).join("/");
  md.push(`| ${wl} | ${cfg} | ${keys.map((kk) => fmt(med((x) => x[kk]))).join(" | ")} | ${heap} | ${total} |`);
}
writeFileSync(join(out, "summary.md"), md.join("\n") + "\n");
log("done", out);
