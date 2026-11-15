// Native allocation call sites per request (Phase 3A): which code makes
// the Rust/C++ heap allocations that alloc.mjs counts. Uses the sampling
// preload alloc-trace.c and a binary with line tables (the profiling
// build) for symbolisation.
//
//   cc -O2 -shared -fPIC -o target/alloc-trace.so benchmarks/phase3a/alloc-trace.c
//   node benchmarks/phase3a/alloc-sites.mjs --group pages --workload pages-ssr
//     [--nextrs target/profiling/release/nextrs] [--requests 3000] [--sample 16]
//     [--out benchmarks/results/phase3a/alloc-sites]
//
// Per workload: start the server (1 isolate) with the preload, warm up,
// switch recording on, send --requests requests (closed loop, 8
// connections, every response checked), stop. Every Nth allocation call
// is recorded with its backtrace. The report attributes each sample to
// its innermost frame outside the allocator and the Rust allocation
// plumbing ("site") and, separately, to the first NextRS frame on its
// stack ("nextrs frame"), and scales sample counts to calls per request.
import { mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import * as L from "./lib.mjs";
import { groups } from "./workloads.mjs";

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) if (argv[i].startsWith("--")) args[argv[i].slice(2)] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
const ROOT = resolve(new URL("../..", import.meta.url).pathname);
const out = resolve(args.out ?? join(ROOT, "benchmarks/results/phase3a/alloc-sites"));
mkdirSync(join(out, "specs"), { recursive: true });
const preload = resolve(args.preload ?? join(ROOT, "target/alloc-trace.so"));
const bin = resolve(args.nextrs ?? join(ROOT, "target/profiling/release/nextrs"));
const requests = Number(args.requests ?? 3000);
const sampleEvery = Number(args.sample ?? 16);
const label = args.label ?? "current";
const group = groups.find((g) => g.id === args.group);
const w = group?.workloads.find((x) => x.id === args.workload);
if (!w) throw new Error("unknown --group/--workload");
const loadgenBin = join(ROOT, "benchmarks/loadgen/target/release/nextrs-loadgen");
const tag = `${group.id}.${w.id}.${label}`;
const trace = join(out, `${tag}.trace.bin`);
for (const f of [trace, `${trace}.on`, `${trace}.maps`]) rmSync(f, { force: true });

const ctx = { group, config: { id: tag, kind: "nextrs" }, port: 6750, root: ROOT, node: process.execPath, scratch: join(out, "specs") };
ctx.services = group.services ? await group.services(ctx) : null;
const s = await L.startServer({
  kind: "nextrs", nextrs: bin, project: join(ROOT, group.fixture), dist: group.dist ?? ".next", port: ctx.port, metricsPort: 6751, isolates: 1, cpus: "0-3",
  env: { ...(group.env?.(ctx) ?? {}), LD_PRELOAD: preload, NEXTRS_ALLOC_TRACE: trace, NEXTRS_ALLOC_SAMPLE: String(sampleEvery) },
  extraArgs: group.nextrsArgs?.(ctx) ?? [], readyPath: group.readyPath ?? "/",
});
ctx.server = s;
let res;
try {
  await group.setup?.(ctx);
  const spec = typeof w.spec === "function" ? await w.spec(ctx) : w.spec;
  const specPath = join(out, "specs", `${tag}.json`);
  writeFileSync(specPath, JSON.stringify(spec));
  const lg = (n) => L.loadgen(loadgenBin, { cpus: "4-5", target: `127.0.0.1:${ctx.port}`, spec: specPath, connections: 8, requests: n, duration: 600, warmup: 0 });
  await lg(Math.max(500, requests / 2));
  writeFileSync(`${trace}.on`, "");
  // The preload polls for the switch every 65,536 allocation calls.
  while (!existsSync(`${trace}.maps`)) await lg(50);
  res = await lg(requests);
} finally {
  await L.stopServer(s);
  await ctx.services?.stop?.();
}
const valid = res.ok === res.completed && res.io_errors === 0;

// ---- decode ----
const raw = readFileSync(trace);
const words = new BigUint64Array(raw.buffer, raw.byteOffset, Math.floor(raw.length / 8));
const samples = [];
for (let i = 0; i + 1 < words.length; ) {
  const head = words[i];
  const k = Number(head >> 48n);
  const size = Number(head & 0xffffffffffffn);
  const tid = Number(words[i + 1]);
  const frames = Array.from(words.subarray(i + 2, i + 2 + k), Number);
  samples.push({ size, tid, frames });
  i += 2 + k;
}

// ---- symbolise: address → (object, offset) via the maps copy ----
const maps = readFileSync(`${trace}.maps`, "utf8").trim().split("\n").map((line) => {
  const [range, perms, offset, , , path = ""] = line.split(/\s+/);
  const [start, end] = range.split("-").map((x) => parseInt(x, 16));
  return { start, end, perms, offset: parseInt(offset, 16), path };
});
const base = new Map();
for (const m of maps) if (m.path && !base.has(m.path)) base.set(m.path, m.start - m.offset);
const locate = (a) => maps.find((m) => a >= m.start && a < m.end);
const unique = new Map();
for (const s of samples) for (const a of s.frames) if (!unique.has(a)) unique.set(a, null);
const byObject = new Map();
for (const a of unique.keys()) {
  const m = locate(a);
  const path = m?.path || "[anon]";
  if (!byObject.has(path)) byObject.set(path, []);
  byObject.get(path).push(a);
}
const names = new Map();
for (const [path, addrs] of byObject) {
  if (!path.startsWith("/") || !existsSync(path)) {
    for (const a of addrs) names.set(a, [`${path.includes("[anon]") ? "[jit]" : path}`]);
    continue;
  }
  const b = base.get(path);
  // Return addresses point after the call: look up a - 1.
  const input = addrs.map((a) => "0x" + (a - b - 1).toString(16)).join("\n");
  // -a prints each address before its (inlined) function/file pairs.
  const withAddr = execFileSync("addr2line", ["-a", "-f", "-C", "-i", "-e", path], { input, maxBuffer: 1 << 30 }).toString().split("\n");
  let cur = null, list = [];
  const short = path.split("/").pop();
  const flush = () => { if (cur !== null) names.set(cur, list.length ? list : [`${short}+?`]); };
  for (let i = 0; i < withAddr.length; i++) {
    const line = withAddr[i];
    if (/^0x[0-9a-f]+$/.test(line)) {
      flush();
      cur = parseInt(line, 16) + b + 1;
      list = [];
    } else if (line && i + 1 < withAddr.length) {
      const fn = line === "??" ? `${short}+0x${(cur - b).toString(16)}` : line;
      list.push(fn);
      i++; // skip the file:line
    }
  }
  flush();
}

// Inlined frames come innermost first; a stack becomes a flat list of
// function names, innermost first.
const stackOf = (s) => s.frames.flatMap((a) => names.get(a) ?? ["?"]);
const plumbing = /^(__rust_|alloc::|<alloc::|core::|<core::|std::alloc|hashbrown::|<hashbrown::|smallvec::|<smallvec::|bytes::|<bytes::|serde_v8::magic|<T as alloc::|<\[T\]|<str as alloc|<\[u8\]|<\[|<&|<T as core::|<T as |std::sys|__libc_|malloc|calloc|realloc|operator new|v8::internal::Malloced|v8::internal::AlignedAlloc|v8::internal::Malloc|v8::base::Malloc|std::__|__gnu_cxx|.*::reserve|.*with_capacity)/;
const nextrsFrame = /nextrs_|nextrs::/;
const rows = new Map(), nrows = new Map(), crates = new Map();
const add = (m, k, size) => { const r = m.get(k) ?? { n: 0, bytes: 0 }; r.n++; r.bytes += size; m.set(k, r); };
for (const s of samples) {
  const st = stackOf(s);
  const site = st.find((f) => !plumbing.test(f)) ?? st[0] ?? "?";
  add(rows, site, s.size);
  const nf = st.find((f) => nextrsFrame.test(f));
  add(nrows, nf ?? (st.some((f) => /^v8::|^Builtins_|^v8_/.test(f)) ? "(V8 internal, no NextRS frame)" : "(other, no NextRS frame)"), s.size);
  const top = site.replace(/^<+/, "").split("::")[0];
  add(crates, top, s.size);
}
const scale = sampleEvery / res.completed;
const table = (m, n = 40) => [...m].sort((a, b) => b[1].n - a[1].n).slice(0, n).map(([k, r]) => ({ site: k, calls_per_req: +(r.n * scale).toFixed(1), bytes_per_req: Math.round(r.bytes * scale), share: +(r.n / samples.length * 100).toFixed(1) }));
const report = {
  tag, binary: bin, sample_every: sampleEvery, requests: res.completed, valid, samples: samples.length,
  estimated_calls_per_req: +(samples.length * scale).toFixed(1),
  by_site: table(rows, 60), by_nextrs_frame: table(nrows, 40), by_crate: table(crates, 25),
};
writeFileSync(join(out, `${tag}.json`), JSON.stringify(report, null, 2));
// Folded stacks (outermost first) for a flamegraph of allocation calls.
const folded = new Map();
for (const s of samples) {
  const k = stackOf(s).slice().reverse().join(";");
  folded.set(k, (folded.get(k) ?? 0) + 1);
}
writeFileSync(join(out, `${tag}.folded`), [...folded].map(([k, v]) => `${k} ${v}`).join("\n") + "\n");
const fmt = (rs) => rs.map((r) => `${String(r.calls_per_req).padStart(8)} ${String(r.bytes_per_req).padStart(9)} B ${String(r.share).padStart(5)} %  ${r.site.slice(0, 160)}`).join("\n");
const txt = `${tag}: ${valid ? "valid" : "INVALID"}, ${res.completed} requests, ${samples.length} samples (1/${sampleEvery}), ~${report.estimated_calls_per_req} allocation calls/request\n\n` +
  `== by first NextRS frame ==\n calls/req     bytes  share\n${fmt(report.by_nextrs_frame)}\n\n== by crate of site ==\n${fmt(report.by_crate)}\n\n== by site ==\n${fmt(report.by_site)}\n`;
writeFileSync(join(out, `${tag}.txt`), txt);
console.log(txt);
rmSync(trace, { force: true });
