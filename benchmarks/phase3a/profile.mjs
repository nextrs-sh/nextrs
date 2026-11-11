// CPU profile of one workload: NextRS (a frame-pointer build with line
// tables) under closed-loop load, sampled by `perf record` for a fixed
// window. JIT frames are named through V8's --perf-basic-prof map.
//
//   node benchmarks/phase3a/profile.mjs --group app --workload ssr
//     [--nextrs target/profiling/release/nextrs] [--isolates 1]
//     [--seconds 15] [--connections 32] [--freq 999] [--out DIR] [--next]
//
// --next profiles `next start` instead (Node's --perf-basic-prof).
// Writes DIR/<group>.<workload>.<server>.perf.data, a folded-stack file and
// `perf report` summaries (by symbol, by shared object).
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawn, execFileSync } from "node:child_process";
import * as L from "./lib.mjs";
import { groups } from "./workloads.mjs";

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) if (argv[i].startsWith("--")) args[argv[i].slice(2)] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
const ROOT = resolve(new URL("../..", import.meta.url).pathname);
const out = resolve(args.out ?? join(ROOT, "docs/evidence/phase-3a/profiles"));
mkdirSync(out, { recursive: true });
const group = groups.find((g) => g.id === args.group);
const w = group?.workloads.find((x) => x.id === args.workload);
if (!w) throw new Error("unknown --group/--workload");
const isNext = Boolean(args.next);
const isolates = Number(args.isolates ?? 1);
const seconds = Number(args.seconds ?? 15);
const tag = `${group.id}.${w.id}.${isNext ? "next" : `nextrs-i${isolates}`}`;
const port = 6300, metricsPort = 6301;
const ctx = { group, config: { id: tag, kind: isNext ? "next" : "nextrs" }, port, root: ROOT, node: process.execPath, scratch: out };
ctx.services = group.services ? await group.services(ctx) : null;
const env = { ...(group.env?.(ctx) ?? {}) };
if (isNext) env.NODE_OPTIONS = "--perf-basic-prof";
const s = await L.startServer({
  kind: ctx.config.kind, node: process.execPath, nextrs: resolve(args.nextrs ?? join(ROOT, "target/profiling/release/nextrs")),
  project: join(ROOT, group.fixture), dist: group.dist ?? ".next", port, metricsPort, isolates, cpus: args["server-cpus"] ?? "0-3",
  env, extraArgs: [...(group.nextrsArgs?.(ctx) ?? []), "--v8-flags=--perf-basic-prof --no-logfile-per-isolate --logfile=/dev/null"], readyPath: group.readyPath ?? "/",
});
ctx.server = s;
try {
  await group.setup?.(ctx);
  const spec = typeof w.spec === "function" ? await w.spec(ctx) : w.spec;
  const specPath = join(out, `${tag}.spec.json`);
  writeFileSync(specPath, JSON.stringify(spec));
  const lgBin = join(ROOT, "benchmarks/loadgen/target/release/nextrs-loadgen");
  const connections = Number(args.connections ?? w.connections ?? 32);
  const lg = (o) => L.loadgen(lgBin, { cpus: args["load-cpus"] ?? "4-5", target: `127.0.0.1:${port}`, spec: specPath, connections, ...o });
  await lg({ duration: 5, warmup: 0 });
  const data = join(out, `${tag}.perf.data`);
  const pids = L.tree(s.pid).join(",");
  // User-space samples and frame-pointer call chains only: with
  // perf_event_paranoid=2 kernel samples and kernel call chains are refused.
  const perf = spawn("perf", ["record", "-e", "cycles:u", "-F", String(args.freq ?? 999), "-g", "--user-callchains", "-p", pids, "-o", data], { stdio: ["ignore", "ignore", "pipe"] });
  let perfErr = "";
  perf.stderr.on("data", (c) => (perfErr += c));
  const perfClosed = new Promise((r) => perf.on("close", r));
  const early = await Promise.race([perfClosed.then(() => true), L.sleep(500).then(() => false)]);
  if (early) throw new Error(`perf record exited: ${perfErr}`);
  const res = await lg({ duration: seconds, warmup: 0 });
  perf.kill("SIGINT");
  await perfClosed;
  writeFileSync(join(out, `${tag}.load.json`), JSON.stringify(res, null, 2));
  const run = (a) => execFileSync("perf", a, { maxBuffer: 1 << 30, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
  writeFileSync(join(out, `${tag}.by-dso.txt`), run(["report", "-i", data, "--no-inline", "--no-children", "--sort", "dso", "--stdio", "-g", "none", "--percent-limit", "0.5"]));
  writeFileSync(join(out, `${tag}.by-symbol.txt`), run(["report", "-i", data, "--no-inline", "--no-children", "--sort", "dso,sym", "--stdio", "-g", "none", "--percent-limit", "0.3"]));
  writeFileSync(join(out, `${tag}.by-symbol-children.txt`), run(["report", "-i", data, "--no-inline", "--children", "--sort", "sym", "--stdio", "-g", "none", "--percent-limit", "1"]));
  writeFileSync(join(out, `${tag}.folded`), fold(run(["script", "-i", data, "--no-inline", "-F", "comm,tid,ip,sym,dso"])));
  console.log(`${tag}: ${Math.round(res.achieved_rps)} rps, ok ${res.ok}/${res.completed}; ${data}`);
} finally {
  await L.stopServer(s);
  await ctx.services?.stop?.();
}

/** perf script output -> folded stacks ("thread;outer;...;inner count"). */
function fold(text) {
  const counts = new Map();
  for (const block of text.split("\n\n")) {
    const lines = block.split("\n").filter(Boolean);
    if (!lines.length) continue;
    const head = lines[0].trim().split(/\s+/);
    const thread = head[0].replace(/[-0-9]+$/, "").replace(/;/g, ":") || "?";
    const frames = lines.slice(1).map((l) => {
      const m = /^\s*[0-9a-f]+\s+(.*?)\s+\(([^)]*)\)\s*$/.exec(l);
      if (!m) return null;
      let sym = m[1] === "[unknown]" ? `[${m[2].split("/").pop()}]` : m[1];
      sym = sym.replace(/\+0x[0-9a-f]+$/, "").replace(/;/g, ":");
      return sym;
    }).filter(Boolean).reverse();
    const key = [thread, ...frames].join(";");
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts].map(([k, v]) => `${k} ${v}`).join("\n") + "\n";
}
