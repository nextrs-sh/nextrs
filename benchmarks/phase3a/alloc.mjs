// Allocations per request (Rust and C/C++ heap; not V8's JavaScript heap)
// for NextRS builds, through the alloc-count.c preload:
//
//   cc -O2 -shared -fPIC -o target/alloc-count.so benchmarks/phase3a/alloc-count.c
//   node benchmarks/phase3a/alloc.mjs --nextrs BIN [--label L] [--nextrs-b BIN --label-b L]
//     [--only workload,...] [--requests 3000] [--out DIR]
//
// Per workload: start the server with the counter, warm up (the same
// number of requests), read the counters, send --requests requests (closed
// loop, 8 connections), read them again. Reports calls (malloc + calloc +
// realloc + aligned) and requested bytes per request. Counting slows
// malloc: these runs are never used for timing.
import { mkdirSync, readFileSync, writeFileSync, appendFileSync } from "node:fs";
import { join, resolve } from "node:path";
import * as L from "./lib.mjs";
import { groups } from "./workloads.mjs";

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) if (argv[i].startsWith("--")) args[argv[i].slice(2)] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
const ROOT = resolve(new URL("../..", import.meta.url).pathname);
const out = resolve(args.out ?? join(ROOT, "benchmarks/results/phase3a/alloc"));
mkdirSync(join(out, "specs"), { recursive: true });
const preload = resolve(args.preload ?? join(ROOT, "target/alloc-count.so"));
const requests = Number(args.requests ?? 3000);
const only = args.only ? new Set(String(args.only).split(",")) : null;
const builds = [{ label: args.label ?? "nextrs", bin: resolve(args.nextrs ?? join(ROOT, "target/release/nextrs")) }];
if (args["nextrs-b"]) builds.push({ label: args["label-b"] ?? "nextrs-b", bin: resolve(args["nextrs-b"]) });
const loadgenBin = join(ROOT, "benchmarks/loadgen/target/release/nextrs-loadgen");
const names = ["mallocs", "callocs", "reallocs", "frees", "aligned", "bytes", "realloc_bytes"];
const read = (file) => {
  const b = readFileSync(file);
  return Object.fromEntries(names.map((n, i) => [n, Number(b.readBigUInt64LE(i * 8))]));
};

let port = 6700;
for (const group of groups) {
  const workloads = group.workloads.filter((w) => !only || only.has(w.id));
  if (!workloads.length) continue;
  for (const b of builds) {
    const ctx = { group, config: { id: b.label, kind: "nextrs" }, port: port++, root: ROOT, node: process.execPath, scratch: join(out, "specs") };
    const metricsPort = port++;
    const counters = join(out, `counters-${b.label}-${group.id}.bin`);
    ctx.services = group.services ? await group.services(ctx) : null;
    const s = await L.startServer({
      kind: "nextrs", nextrs: b.bin, project: join(ROOT, group.fixture), dist: group.dist ?? ".next", port: ctx.port, metricsPort, isolates: 1, cpus: "0-3",
      env: { ...(group.env?.(ctx) ?? {}), LD_PRELOAD: preload, NEXTRS_ALLOC_COUNTERS: counters }, extraArgs: group.nextrsArgs?.(ctx) ?? [], readyPath: group.readyPath ?? "/",
    });
    ctx.server = s;
    try {
      await group.setup?.(ctx);
      for (const w of workloads) {
        const spec = typeof w.spec === "function" ? await w.spec(ctx) : w.spec;
        const specPath = join(out, "specs", `${group.id}.${w.id}.${b.label}.json`);
        writeFileSync(specPath, JSON.stringify(spec));
        const lg = (n) => L.loadgen(loadgenBin, { cpus: "4-5", target: `127.0.0.1:${ctx.port}`, spec: specPath, connections: 8, requests: n, duration: 600, warmup: 0 });
        await lg(Math.max(500, requests / 2));
        const a = read(counters);
        const res = await lg(requests);
        const z = read(counters);
        const n = res.completed;
        const d = Object.fromEntries(names.map((k) => [k, z[k] - a[k]]));
        const row = {
          group: group.id, workload: w.id, build: b.label, requests: n, valid: res.ok === n && res.io_errors === 0,
          allocations_per_req: +((d.mallocs + d.callocs + d.reallocs + d.aligned) / n).toFixed(1),
          frees_per_req: +(d.frees / n).toFixed(1),
          bytes_per_req: Math.round((d.bytes + d.realloc_bytes) / n),
          detail_per_req: Object.fromEntries(names.map((k) => [k, +(d[k] / n).toFixed(1)])),
        };
        appendFileSync(join(out, "alloc.jsonl"), JSON.stringify(row) + "\n");
        console.log(`${group.id}/${w.id.padEnd(22)} ${b.label.padEnd(10)} ${row.valid ? "ok" : "INVALID"} ${String(row.allocations_per_req).padStart(8)} allocs/req ${String(row.bytes_per_req).padStart(10)} B/req`);
      }
    } finally {
      await L.stopServer(s);
      await ctx.services?.stop?.();
    }
  }
}
