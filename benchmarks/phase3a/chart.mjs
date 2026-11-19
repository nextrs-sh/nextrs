// README benchmark chart from a summarize.mjs summary: four panels
// (throughput, p95 latency, memory, CPU per request) for one workload,
// next start against NextRS, desktop and narrow-screen SVGs. Every number
// drawn comes from summary.json; bars start at zero.
//
//   node benchmarks/phase3a/chart.mjs RESULT_DIR --workload app/ssr
//     [--configs next-start,nextrs-i1,nextrs-i4] [--cpu-config nextrs-i1]
//     [--out .github/assets] [--raw-url URL] [--name nextrs-benchmarks] [--path /request/path]
//     [--version 0.6.0]   (label NextRS by release version; the commit stays in the description)
//
// Throughput shows every listed configuration (isolates are parallelism
// within the same 4 CPUs); latency, memory and CPU compare the first two
// (next start and NextRS with the same number of JavaScript threads).
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const argv = process.argv.slice(2);
const dir = argv[0];
const opt = {};
for (let i = 1; i < argv.length; i++) if (argv[i].startsWith("--")) opt[argv[i].slice(2)] = argv[++i];
const S = JSON.parse(readFileSync(join(dir, "summary.json"), "utf8"));
const [group, workload] = (opt.workload ?? "app/ssr").split("/");
const W = S.workloads.find((w) => w.group === group && w.workload === workload);
if (!W) throw new Error(`no workload ${opt.workload}`);
const configs = (opt.configs ?? "next-start,nextrs-i1,nextrs-i4").split(",");
const [base, main] = configs;
const env = S.environment;
const outDir = opt.out ?? ".github/assets";
const raw = opt["raw-url"] ?? `benchmarks/results/phase3a/${dir.split("/").pop()}`;
const commit = (env.nextrs_commit ?? "").slice(0, 7);
const date = env.date.slice(0, 10);
const release = opt.version ? `${opt.version} (${commit})` : commit;
const shown = opt.version ?? commit;
const path = opt.path ?? W.label?.match(/\(([^)]+)\)/)?.[1] ?? workload;
const title = opt.path ? W.label : W.label.replace(/\s*\([^)]*\)\s*$/, "");

const C = { bg: "#0D1117", border: "#21262D", panel: "#161B22", panelBorder: "#30363D", text: "#F0F6FC", muted: "#8B949E", soft: "#C9D1D9", accent: "#E8762D", nextrs: "#CE422B", nextrsLight: "#E0705A", next: "#8B949E" };
const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
const name = (c) => (c === "next-start" ? "next start" : `NextRS · ${c.split("-i").pop()} isolate${c.endsWith("-i1") ? "" : "s"}`);
const color = (c, i) => (c === "next-start" ? C.next : i <= 1 ? C.nextrs : C.nextrsLight);
const fmt = (v, unit) => (unit === "req/s" ? `${Math.round(v).toLocaleString("en-US")} req/s` : unit === "ms" ? `${v >= 10 ? v.toFixed(1) : v.toFixed(2)} ms` : `${Math.round(v)} MiB`);
const get = (c, mode, k) => W.configs[c]?.[mode]?.[k];

const panels = [
  { title: "Throughput", sub: "requests per second · higher is better", unit: "req/s", rows: configs.map((c) => [c, get(c, "closed", "rps")]) },
  { title: "p95 latency", sub: `open loop at ${Math.round(get(base, "open", "offered_rate") ?? 0)} req/s for both · lower is better`, unit: "ms", rows: [base, main].map((c) => [c, get(c, "open", "p95_ms")]) },
  { title: "Anonymous memory", sub: "process-tree anonymous RSS after load · lower is better", unit: "MiB", rows: [base, main].map((c) => [c, get(c, "closed", "anon_end_mib")]) },
  { title: "CPU per request", sub: "server CPU time (user + system) · lower is better", unit: "ms", rows: [base, main].map((c) => [c, get(c, "closed", "cpu_ms_per_req")]) },
];
for (const p of panels) for (const [c, v] of p.rows) if (v == null) throw new Error(`missing ${p.title} for ${c}`);

const summaryText = panels.map((p) => `${p.title}: ${p.rows.map(([c, v]) => `${name(c)} ${fmt(v, p.unit)}`).join(", ")}`).join(". ");
const desc = `Measured ${date} on NextRS ${release}, Next.js ${env.fixtures?.[group]?.next_version ?? "16"}: ${title} (${path}); throughput, CPU and memory in a closed loop with 32 connections, p95 latency in an open loop at an equal offered rate; median of ${W.configs[base].closed.rounds} interleaved rounds on one machine. ${summaryText}.`;

function panel(x, y, w, h, p, labelW) {
  const max = Math.max(...p.rows.map(([, v]) => v));
  const barMax = w - labelW - 56 - 150;
  const rowH = p.rows.length > 2 ? 40 : 56;
  let s = `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="16" fill="${C.panel}" stroke="${C.panelBorder}" stroke-width="1.5"/>`;
  s += `<text x="${x + 28}" y="${y + 46}" class="sans" font-size="25" font-weight="600" fill="${C.text}">${esc(p.title)}</text>`;
  s += `<text x="${x + 28}" y="${y + 76}" class="sans" font-size="16" fill="${C.muted}">${esc(p.sub)}</text>`;
  p.rows.forEach(([c, v], i) => {
    const yy = y + 108 + i * rowH;
    const bw = Math.max(2, (v / max) * barMax);
    const isNext = c === "next-start";
    s += `<text x="${x + 28}" y="${yy + 21}" class="${isNext ? "mono" : "sans"}" font-size="${isNext ? 17 : 18}" fill="${C.soft}">${esc(name(c))}</text>`;
    s += `<rect x="${x + 28 + labelW}" y="${yy}" width="${bw.toFixed(1)}" height="28" rx="7" fill="${color(c, i)}"/>`;
    s += `<text x="${(x + 28 + labelW + bw + 12).toFixed(1)}" y="${yy + 21}" class="sans" font-size="20" font-weight="700" fill="${C.text}">${esc(fmt(v, p.unit))}</text>`;
  });
  return s;
}

function svg(width, layout) {
  const narrow = layout === "narrow";
  const pw = narrow ? width - 64 : (width - 96 - 32) / 2;
  const ph = 250;
  const top = narrow ? 268 : 210;
  const dy = narrow ? 38 : 0; // the narrow title takes two lines
  const positions = panels.map((_, i) => (narrow ? [32, top + i * (ph + 24)] : [48 + (i % 2) * (pw + 32), top + Math.floor(i / 2) * (ph + 24)]));
  const footY = (narrow ? top + 4 * (ph + 24) : top + 2 * (ph + 24)) + 16;
  const labelW = narrow ? 210 : 200;
  const isolates = configs.filter((c) => c !== base).map((c) => c.split("-i").pop()).join(" and ");
  const foot = [
    `Workload: ${title} — ${path} · ${S.workloads.length} workloads measured, each reported separately.`,
    `Next.js ${env.fixtures?.[group]?.next_version ?? "16"} on Node ${env.node} · NextRS ${shown} · ${isolates} V8 isolate(s) · ${env.cpu_model}, server on CPUs ${env.server_cpus}, load generator on ${env.load_cpus} (single host)`,
    `${date} · same .next build · closed loop, 32 connections; p95 open loop at an equal offered rate · ${W.configs[base].closed.rounds} interleaved rounds, medians · raw data: ${raw}`,
    `Results are specific to the tested workloads, hardware, and runtime configurations. They are not guarantees of production performance.`,
  ];
  const wrap = (t, n) => (t.length <= n ? [t] : t.split(" ").reduce((ls, w) => ((ls[ls.length - 1] + " " + w).length > n ? ls.push(w) : (ls[ls.length - 1] += " " + w), ls), [""]).map((l) => l.trim()));
  const footLines = foot.flatMap((t) => wrap(t, narrow ? 82 : 150));
  const height = footY + 24 + footLines.length * 24 + 32;
  let s = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="bench-title bench-desc">`;
  s += `<title id="bench-title">NextRS vs next start — Reproducible Benchmark Results</title><desc id="bench-desc">${esc(desc)}</desc>`;
  s += `<style>.sans{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","Noto Sans",Helvetica,Arial,sans-serif}.mono{font-family:ui-monospace,SFMono-Regular,"SF Mono",Menlo,Consolas,"Liberation Mono",monospace}</style>`;
  s += `<rect x="1" y="1" width="${width - 2}" height="${height - 2}" rx="24" fill="${C.bg}" stroke="${C.border}" stroke-width="2"/>`;
  s += `<text x="${narrow ? 32 : 48}" y="70" class="mono" font-size="15" font-weight="600" letter-spacing="2.4" fill="${C.accent}">PHASE 3A · REPRODUCIBLE BENCHMARK · EXPERIMENTAL</text>`;
  s += `<text x="${narrow ? 32 : 48}" y="112" class="sans" font-size="${narrow ? 26 : 32}" font-weight="700" fill="${C.text}">NextRS vs <tspan class="mono">next start</tspan> —${narrow ? `</text><text x="32" y="${112 + dy}" class="sans" font-size="26" font-weight="700" fill="${C.text}">` : " "}Reproducible Benchmark Results</text>`;
  s += `<text x="${narrow ? 32 : 48}" y="${148 + dy}" class="sans" font-size="18" fill="${C.muted}">${esc(title)} · <tspan class="mono" font-size="17">${esc(path)}</tspan></text>`;
  s += `<text x="${narrow ? 32 : 48}" y="${176 + dy}" class="sans" font-size="16" fill="${C.muted}">Next.js ${esc(env.fixtures?.[group]?.next_version ?? "16")} · NextRS ${esc(shown)} · ${esc(date)} · median of ${W.configs[base].closed.rounds} interleaved rounds</text>`;
  panels.forEach((p, i) => (s += panel(positions[i][0], positions[i][1], pw, ph, p, labelW)));
  footLines.forEach((l, i) => (s += `<text x="${narrow ? 32 : 48}" y="${footY + 24 + i * 24}" class="sans" font-size="${i === footLines.length - 1 ? 15 : 15}" fill="${i >= footLines.length - wrap(foot[3], narrow ? 82 : 150).length ? C.muted : C.soft}">${esc(l)}</text>`));
  return s + "</svg>\n";
}

const fileName = opt.name ?? "nextrs-benchmarks";
writeFileSync(join(outDir, `${fileName}.svg`), svg(1280, "wide"));
writeFileSync(join(outDir, `${fileName}-mobile.svg`), svg(720, "narrow"));
console.log(desc);
