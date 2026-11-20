// README benchmark infographic from a summarize.mjs result directory: one
// workload, next start against NextRS, as a desktop SVG and a separate
// narrow-screen SVG.
//
//   node benchmarks/phase3a/chart.mjs RESULT_DIR [--workload app/ssr]
//     [--configs next-start,nextrs-i1,nextrs-i4] [--out .github/assets]
//     [--raw-url PATH] [--name nextrs-benchmarks] [--path /request/path]
//     [--connections N]   (default: read from RESULT_DIR/rows.jsonl)
//     [--version 0.1.0]   (label NextRS by release version instead of the
//                          commit. Use it only when the measured binary
//                          reported that version.)
//
// The first configuration is the baseline (next start), the second is
// NextRS with the same number of JavaScript threads (one isolate), and an
// optional third is NextRS with more isolates. The four headline metrics
// compare the first two; the third appears only in the isolate-scaling
// panel, with throughput, because extra isolates are extra parallel
// JavaScript within the same CPU allocation, not an equal-worker comparison.
//
// Every number drawn comes from summary.json (medians and the environment
// recorded in it) and rows.jsonl (the connection count). Improvements are
// computed from the medians, not typed in. Bars start at zero. The output is
// deterministic. Tests: node --test benchmarks/phase3a/chart.test.mjs
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

// ---------------------------------------------------------------- numbers

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WORDS = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve"];

export const esc = (s) =>
  String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");

// Locale-independent thousands separators, so output is the same everywhere.
export const group = (n) => String(Math.round(n)).replace(/\B(?=(\d{3})+(?!\d))/g, ",");

export function fmt(v, unit) {
  if (unit === "req/s") return `${group(v)} req/s`;
  if (unit === "ms") return `${v >= 100 ? group(v) : v >= 10 ? v.toFixed(1) : v.toFixed(2)} ms`;
  if (unit === "MiB") return `${group(v)} MiB`;
  throw new Error(`unknown unit ${unit}`);
}

export const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;
export const workloadCount = (n) => plural(n, "workload");
export const numberWord = (n) => WORDS[n] ?? String(n);
export const capitalize = (s) => s.charAt(0).toUpperCase() + s.slice(1);

// "2026-11-18T06:21:34Z" -> "Nov 18, 2026" (UTC, no locale data).
export function dateLabel(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) throw new Error(`invalid date ${iso}`);
  return `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()}, ${d.getUTCFullYear()}`;
}

// "Intel(R) Core(TM) i5-9400F CPU @ 2.90GHz" -> "Intel Core i5-9400F".
export const cpuLabel = (s) =>
  String(s ?? "unknown CPU").replace(/\((R|TM|C)\)/gi, "").replace(/\s+CPU\b.*$/i, "").replace(/\s*@.*$/, "").replace(/\s+/g, " ").trim();

// "0-3" or "0,1,2,3" -> 4.
export function cpuCount(list) {
  if (list == null) return null;
  let n = 0;
  for (const part of String(list).split(",")) {
    const [a, b] = part.split("-").map(Number);
    if (Number.isNaN(a) || (b != null && Number.isNaN(b))) return null;
    n += b == null ? 1 : b - a + 1;
  }
  return n;
}

// Relative change of `value` against `base`, oriented so that a positive
// percentage is always an improvement. `better` is "higher" or "lower".
// Rounding is to whole percent, half away from zero.
export function improvement(base, value, better) {
  if (!(base > 0) || !(value >= 0)) throw new Error(`cannot compare ${value} with ${base}`);
  const ratio = value / base;
  const pct = better === "higher" ? (ratio - 1) * 100 : better === "lower" ? (1 - ratio) * 100 : NaN;
  if (Number.isNaN(pct)) throw new Error(`better must be "higher" or "lower", not ${better}`);
  // toFixed first, so that 0.5 computed as 0.49999999999 still rounds up.
  const rounded = Math.sign(pct) * Math.round(Number(Math.abs(pct).toFixed(9)));
  const status = rounded > 0 ? "better" : rounded < 0 ? "worse" : "equal";
  return { ratio, pct, rounded, status };
}

export const factor = (ratio) => `${ratio >= 100 ? group(ratio) : ratio.toFixed(2)}×`;

// ------------------------------------------------------------------- model

const METRICS = [
  {
    key: "throughput", label: "Throughput", mode: "closed", field: "rps", unit: "req/s", better: "higher",
    caption: { better: "Higher throughput", worse: "Lower throughput", equal: "Throughput" },
    word: { better: "", worse: "", equal: "" },
    note: "Closed loop · higher is better",
  },
  {
    key: "latency", label: "Latency", mode: "open", field: "p95_ms", unit: "ms", better: "lower",
    caption: { better: "p95 response latency", worse: "p95 response latency", equal: "p95 response latency" },
    word: { better: "lower", worse: "higher", equal: "" },
    note: "Open loop · lower is better",
  },
  {
    key: "memory", label: "Memory", mode: "closed", field: "anon_end_mib", unit: "MiB", better: "lower",
    caption: { better: "Anonymous memory", worse: "Anonymous memory", equal: "Anonymous memory" },
    word: { better: "less", worse: "more", equal: "" },
    note: "After load · lower is better",
  },
  {
    key: "cpu", label: "CPU", mode: "closed", field: "cpu_ms_per_req", unit: "ms", better: "lower",
    caption: { better: "CPU time per request", worse: "CPU time per request", equal: "CPU time per request" },
    word: { better: "less", worse: "more", equal: "" },
    note: "User + system · lower is better",
  },
];

export function isolatesOf(W, config) {
  const n = W.scaling?.[config]?.isolates ?? Number(config.match(/-i(\d+)$/)?.[1]);
  if (!Number.isInteger(n) || n < 1) throw new Error(`cannot tell the isolate count of ${config}`);
  return n;
}

export function configName(W, config) {
  if (config === "next-start") return "next start";
  return `NextRS · ${plural(isolatesOf(W, config), "isolate")}`;
}

// Connections used by the closed-loop rows of one workload (one value).
export function connectionsFrom(rowsText, groupName, workload, configs) {
  const seen = new Set();
  for (const line of rowsText.split("\n")) {
    if (!line.trim()) continue;
    const row = JSON.parse(line);
    if (row.group === groupName && row.workload === workload && row.mode === "closed" && configs.includes(row.config) && row.connections != null) seen.add(row.connections);
  }
  if (seen.size !== 1) throw new Error(`expected one closed-loop connection count, found ${[...seen].join(", ") || "none"}`);
  return [...seen][0];
}

// How much of its run the chart shows: one workload, or one of several.
export const runScope = (n) =>
  n === 1 ? "Its run measured this one workload. " : `This chart shows 1 of the ${workloadCount(n)} in its run; each is reported separately. `;

export function buildModel(S, o = {}) {
  const [groupName, workload] = (o.workload ?? "app/ssr").split("/");
  const W = S.workloads?.find((w) => w.group === groupName && w.workload === workload);
  if (!W) throw new Error(`no workload ${groupName}/${workload} in summary`);
  const configs = o.configs ?? ["next-start", "nextrs-i1", "nextrs-i4"];
  if (configs.length < 2 || configs.length > 3) throw new Error("--configs needs a baseline, NextRS, and optionally one scaled NextRS");
  const [base, main, scaled] = configs;
  if (base !== "next-start") throw new Error(`the baseline must be next-start, not ${base}`);
  for (const c of configs) if (!W.configs?.[c]) throw new Error(`no configuration ${c} for ${groupName}/${workload}`);
  const env = S.environment ?? {};
  const get = (c, mode, field, what) => {
    const v = W.configs[c]?.[mode]?.[field];
    if (typeof v !== "number" || !Number.isFinite(v)) throw new Error(`missing ${what} for ${c} (${mode}.${field})`);
    return v;
  };
  const rounds = W.configs[base].closed?.rounds;
  if (!Number.isInteger(rounds)) throw new Error(`missing round count for ${base}`);
  const offered = configs.map((c) => get(c, "open", "offered_rate", "open-loop offered rate"));
  if (new Set(offered).size !== 1) throw new Error(`open-loop offered rates differ: ${offered.join(", ")}`);
  if (o.connections == null) throw new Error("missing closed-loop connection count");
  const nextVersion = env.fixtures?.[groupName]?.next_version;
  if (!nextVersion) throw new Error(`missing Next.js version for fixture ${groupName}`);
  if (!env.node) throw new Error("missing Node.js version");
  if (!env.date) throw new Error("missing measurement date");
  const commit = (env.nextrs_commit ?? "").slice(0, 7);
  if (!commit) throw new Error("missing NextRS commit");
  const shown = o.version ?? commit;
  const label = W.label ?? `${groupName}/${workload}`;
  const path = o.path ?? label.match(/\(([^)]+)\)\s*$/)?.[1] ?? `${groupName}/${workload}`;
  const title = label.replace(/\s*\([^)]*\)\s*$/, "");
  const date = dateLabel(env.date);
  const mainName = configName(W, main);

  const metrics = METRICS.map((m) => {
    const what = `${m.label.toLowerCase()} (${m.field})`;
    const b = get(base, m.mode, m.field, what);
    const v = get(main, m.mode, m.field, what);
    const imp = improvement(b, v, m.better);
    // The summary's own bootstrap test: a difference inside the noise is
    // not drawn as an advantage.
    const sig = W.ratios?.[main]?.[m.mode]?.[m.field]?.significant;
    const status = sig === false && imp.status !== "equal" ? "noise" : imp.status;
    const shownPct = Math.abs(imp.rounded);
    const headline =
      imp.status === "equal" ? { value: "≈", suffix: "same" }
      : m.better === "higher" ? { value: `${imp.rounded > 0 ? "+" : "−"}${shownPct}%`, suffix: "" }
      // A reduction that rounds to 100 % is still not all of it.
      : { value: imp.status === "better" && shownPct >= 100 && v > 0 ? ">99%" : `${shownPct}%`, suffix: m.word[imp.status] };
    return {
      key: m.key, label: m.label, unit: m.unit, better: m.better, note: m.note,
      caption: m.caption[imp.status] + (status === "noise" ? " (within noise)" : ""),
      headline, status, ratio: imp.ratio, pct: imp.pct, rounded: imp.rounded,
      rows: [
        { config: main, name: mainName, value: v, text: fmt(v, m.unit), nextrs: true },
        { config: base, name: "next start", value: b, text: fmt(b, m.unit), nextrs: false },
      ],
    };
  });

  let scaling = null;
  if (scaled) {
    const n = isolatesOf(W, scaled);
    const rows = [base, main, scaled].map((c) => {
      const v = get(c, "closed", "rps", "throughput (rps)");
      return { config: c, name: configName(W, c), value: v, text: fmt(v, "req/s"), nextrs: c !== base, scaled: c === scaled };
    });
    const ratio = rows[2].value / rows[0].value;
    const cpus = cpuCount(env.server_cpus);
    const allocation = cpus ? `the same ${numberWord(cpus)}-CPU server allocation` : "the same server CPU allocation";
    scaling = {
      isolates: n, rows, ratio, factor: factor(ratio),
      headline: "measured throughput",
      versus: `${configName(W, scaled)} vs next start`,
      note: `${capitalize(plural(n, "isolate").replace(/^\d+/, numberWord(n)))} ${n === 1 ? "uses" : "share"} ${allocation}; the Node baseline is one server process.`,
    };
  }

  const nodeVersion = String(env.node).replace(/^v/, "");
  const host = String(env.host_kind ?? "").startsWith("single-host") ? "Single host" : "Separate hosts";
  const items = [
    `Same Next.js build · ${plural(rounds, "interleaved round")}`,
    `${o.connections} connections for throughput/CPU/memory`,
    `Open-loop p95 at ${group(offered[0])} req/s`,
    `${cpuLabel(env.cpu_model)} · ${host}`,
  ];
  const raw = o.rawUrl ?? "benchmarks/results";
  const sha = env.nextrs_binary_sha256 ? `binary sha256 ${env.nextrs_binary_sha256.slice(0, 12)}…` : null;
  const provenance = [
    `NextRS ${commit}${env.nextrs_dirty ? " (working tree had uncommitted changes)" : ""}`,
    sha,
    o.version ? null : "pre-release build",
  ].filter(Boolean).join(" · ");

  const pctText = (m) => (m.status === "equal" ? "about the same" : `${m.headline.value}${m.headline.suffix ? ` ${m.headline.suffix}` : ""}`);
  const desc =
    `${title} (${path}), measured ${date} on one machine: NextRS ${shown} against next start, ` +
    `Next.js ${nextVersion}, Node.js ${nodeVersion}, the same Next.js build, median of ${plural(rounds, "interleaved round")}. ` +
    runScope(S.workloads.length) +
    metrics.map((m) => `${m.label}: ${pctText(m)} (${m.caption}), ${m.rows.map((row) => `${row.name} ${row.text}`).join(" against ")}.`).join(" ") +
    (scaling ? ` Scale across V8 isolates: ${scaling.rows.map((row) => `${row.name} ${row.text}`).join(", ")}; ${scaling.factor} ${scaling.headline} (${scaling.versus}). ${scaling.note}` : "") +
    ` Throughput, CPU and memory: closed loop with ${o.connections} connections. p95: open loop at ${group(offered[0])} req/s for every server. ${cpuLabel(env.cpu_model)}, ${host.toLowerCase()}.` +
    ` Results vary by workload and hardware; not a production performance guarantee. ${provenance}. Raw data: ${raw}.`;

  return {
    eyebrow: `EXPERIMENTAL BENCHMARK · ${date.toUpperCase()}`,
    title: { lead: "NextRS vs ", code: "next start" },
    workloadTitle: title,
    subtitle: `${title} · Same Next.js build · Measured results`,
    meta: `Next.js ${nextVersion} · Node.js ${nodeVersion} · NextRS ${shown} · ${path}`,
    legend: [{ name: mainName, nextrs: true }, { name: "next start", nextrs: false }],
    metrics, scaling, items,
    disclaimer: "Results vary by workload and hardware. Not a production performance guarantee.",
    source: `Raw data: ${raw}`,
    provenance,
    desc, date, commit, shown, nextVersion, nodeVersion, path, rounds, connections: o.connections, offered: offered[0],
    workloads: S.workloads.length,
  };
}

// ------------------------------------------------------------------ render

export const COLORS = {
  bg: "#0D1117", border: "#30363D",
  card: "#161B22", cardBorder: "#30363D",
  scale: "#11161D", scaleBorder: "#262C36",
  text: "#F0F6FC", soft: "#C9D1D9", muted: "#8B949E",
  accent: "#FF7B45", // headline figures and the eyebrow
  nextrs: "#E8622F", // NextRS bars
  nextrsStripe: "#FFB08A",
  next: "#6E7681", // next start bars: a neutral baseline
  track: "#21262D",
};
const C = COLORS;
const SANS = `-apple-system,BlinkMacSystemFont,"Segoe UI","Noto Sans",Helvetica,Arial,sans-serif`;
const MONO = `ui-monospace,SFMono-Regular,"SF Mono",Menlo,Consolas,"Liberation Mono",monospace`;

// A conservative width estimate (system fonts differ), used to wrap text,
// to fit labels, and by the layout test.
export function textWidth(text, size, { mono = false, weight = 400, spacing = 0 } = {}) {
  let w = 0;
  for (const ch of String(text)) {
    let k;
    if (mono) k = 0.62;
    else if (/[0-9]/.test(ch)) k = 0.6;
    else if (/[ il.,:;'|!·()]/.test(ch)) k = 0.32;
    else if (/[A-Z%×≈+−]/.test(ch)) k = 0.7;
    else if (/[mwMW]/.test(ch)) k = 0.85;
    else k = 0.56;
    w += k * size + spacing;
  }
  return w * (weight >= 600 && !mono ? 1.06 : 1);
}

// Greedy wrap that prefers to break at " · " separators, then at spaces.
export function wrap(text, size, max, opts) {
  const parts = String(text).split(" · ");
  if (parts.length > 1) {
    const lines = [];
    for (const part of parts) {
      const last = lines[lines.length - 1];
      if (last != null && textWidth(`${last} · ${part}`, size, opts) <= max) lines[lines.length - 1] = `${last} · ${part}`;
      else lines.push(...wrap(part, size, max, opts));
    }
    return lines;
  }
  const lines = [];
  for (const word of String(text).split(" ")) {
    const last = lines[lines.length - 1];
    if (last != null && textWidth(`${last} ${word}`, size, opts) <= max) lines[lines.length - 1] = `${last} ${word}`;
    else lines.push(word);
  }
  return lines;
}

const r = (v) => (Math.round(v * 10) / 10).toString();

function canvas() {
  const out = [];
  const boxes = [];
  // Segments: [{ text, size, weight, mono, fill, spacing, dx }] in one <text>.
  const text = (x, y, segs, { anchor = "start" } = {}) => {
    if (!Array.isArray(segs)) segs = [segs];
    let w = 0;
    let size = 0;
    for (const s of segs) {
      w += (s.dx ?? 0) + textWidth(s.text, s.size, s);
      size = Math.max(size, s.size);
    }
    const x0 = anchor === "end" ? x - w : anchor === "middle" ? x - w / 2 : x;
    boxes.push({ kind: "text", x0, x1: x0 + w, top: y - size * 0.78, bottom: y + size * 0.22, text: segs.map((s) => s.text).join("") });
    const attrs = (s) =>
      `${s.mono ? ` class="m"` : ""} font-size="${s.size}"${s.weight && s.weight !== 400 ? ` font-weight="${s.weight}"` : ""}${s.spacing ? ` letter-spacing="${s.spacing}"` : ""} fill="${s.fill ?? C.text}"`;
    const [first, ...rest] = segs;
    let s = `<text x="${r(x)}" y="${r(y)}"${anchor !== "start" ? ` text-anchor="${anchor}"` : ""}${attrs(first)}>${esc(first.text)}`;
    for (const t of rest) s += `<tspan${t.dx ? ` dx="${t.dx}"` : ""}${attrs(t)}>${esc(t.text)}</tspan>`;
    out.push(`${s}</text>`);
    return w;
  };
  // `mark` registers a data mark (bar, swatch) for the overlap test.
  const rect = (x, y, w, h, attrs, mark) => {
    if (mark) boxes.push({ kind: mark, x0: x, x1: x + w, top: y, bottom: y + h });
    out.push(`<rect x="${r(x)}" y="${r(y)}" width="${r(w)}" height="${r(h)}"${attrs}/>`);
  };
  return { out, boxes, text, rect };
}

// Type sizes. The desktop image is shown at about 0.65x in a GitHub README
// and the narrow one at about 0.5x on a phone, so neither goes small.
const SIZES = {
  wide: {
    width: 1280, margin: 48, gap: 20, cols: 4, pad: 22,
    eyebrow: 14, title: 48, sub: 21, meta: 16, legend: 16,
    label: 14, head: 60, caption: 19, row: 16, rowGap: 50, barH: 10, note: 14,
    factor: 44, scaleHead: 18, scaleRow: 17, scaleNote: 15,
    foot: 16, disclaimer: 15, source: 14,
  },
  narrow: {
    width: 720, margin: 28, gap: 16, cols: 2, pad: 22,
    eyebrow: 15, title: 44, sub: 21, meta: 17, legend: 18,
    label: 15, head: 64, caption: 20, row: 17, rowGap: 52, barH: 12, note: 15,
    factor: 50, scaleHead: 20, scaleRow: 19, scaleNote: 17,
    foot: 19, disclaimer: 17, source: 15,
  },
};

// Label and value on one line above each bar; both shrink if they would touch.
function barRows(cv, x, y, w, rows, Z) {
  const max = Math.max(...rows.map((row) => row.value));
  rows.forEach((row, i) => {
    const yy = y + i * Z.rowGap;
    let fs = Z.row;
    const fits = (s) => textWidth(row.name, s, { mono: !row.nextrs }) + 12 + textWidth(row.text, s, { weight: 700 }) <= w;
    while (!fits(fs) && fs > 9) fs -= 0.5;
    cv.text(x, yy, { text: row.name, size: fs, mono: !row.nextrs, fill: C.soft });
    cv.text(x + w, yy, { text: row.text, size: fs, weight: 700, fill: C.text }, { anchor: "end" });
    cv.rect(x, yy + 9, w, Z.barH, ` rx="${Z.barH / 2}" fill="${C.track}"`);
    cv.rect(x, yy + 9, Math.max(Z.barH, (row.value / max) * w), Z.barH, ` rx="${Z.barH / 2}" fill="${row.nextrs ? C.nextrs : C.next}"`, "bar");
  });
}

// Vertical positions inside a metric card, relative to its top.
function cardLayout(Z, captionLines) {
  const label = 38;
  const head = label + Math.round(Z.head * 1.08);
  const caption = head + Z.caption + 14;
  const bars = caption + (captionLines - 1) * (Z.caption + 5) + Z.row + 28;
  const note = bars + Z.rowGap + 9 + Z.barH + Z.note + 22;
  return { label, head, caption, bars, note, height: note + 22 };
}

function metricCard(cv, x, y, w, L, m, Z) {
  const inner = w - 2 * Z.pad;
  const px = x + Z.pad;
  cv.rect(x, y, w, L.height, ` rx="16" fill="${C.card}" stroke="${C.cardBorder}"`);
  const good = m.status === "better";
  cv.rect(px, y, 36, 3, ` fill="${good ? C.accent : C.muted}"`);
  cv.text(px, y + L.label, { text: m.label.toUpperCase(), size: Z.label, weight: 600, spacing: 1.6, fill: C.muted });
  // The headline shrinks rather than overflow the card.
  const headline = (size) => {
    const segs = [{ text: m.headline.value, size, weight: 700, fill: good ? C.accent : C.soft }];
    if (m.headline.suffix) segs.push({ text: m.headline.suffix, size: Math.round(size * 0.45), weight: 600, fill: good ? C.accent : C.soft, dx: 8 });
    return segs;
  };
  const span = (segs) => segs.reduce((a, s) => a + (s.dx ?? 0) + textWidth(s.text, s.size, s), 0);
  let hs = Z.head;
  while (span(headline(hs)) > inner && hs > 20) hs -= 2;
  cv.text(px, y + L.head, headline(hs));
  wrap(m.caption, Z.caption, inner).forEach((l, i) => cv.text(px, y + L.caption + i * (Z.caption + 5), { text: l, size: Z.caption, weight: 500, fill: C.text }));
  barRows(cv, px, y + L.bars, inner, m.rows, Z);
  cv.text(px, y + L.note, { text: m.note, size: Z.note, fill: C.muted });
}

function scalePanel(cv, x, y, w, sc, narrow, Z) {
  const pad = Z.pad + 4;
  const max = Math.max(...sc.rows.map((row) => row.value));
  const panel = (h) => cv.out.push(`<rect x="${r(x)}" y="${r(y)}" width="${r(w)}" height="${r(h)}" rx="16" fill="${C.scale}" stroke="${C.scaleBorder}"/>`);
  const bar = (bx, by, bw, bh, row) => {
    const fill = row.scaled ? "url(#stripes)" : row.nextrs ? C.nextrs : C.next;
    cv.rect(bx, by, Math.max(bh, (row.value / max) * bw), bh, ` rx="${bh / 2}" fill="${fill}"`, "bar");
  };
  const inner = w - 2 * pad;
  const lines = wrap(sc.note, Z.scaleNote, inner);
  if (!narrow) {
    const leftW = 340;
    const rowsX = x + pad + leftW;
    const labelW = Math.max(...sc.rows.map((row) => textWidth(row.name, Z.scaleRow, { mono: !row.nextrs }))) + 20;
    const valueW = Math.max(...sc.rows.map((row) => textWidth(row.text, Z.scaleRow, { weight: 700 }))) + 14;
    const barW = x + w - pad - rowsX - labelW - valueW;
    const rowGap = 42;
    const rowsTop = 30;
    const noteY = Math.max(152, rowsTop + 2 * rowGap + 22) + 34;
    const h = noteY + (lines.length - 1) * (Z.scaleNote + 5) + 24;
    panel(h);
    cv.text(x + pad, y + 40, { text: "SCALE ACROSS V8 ISOLATES", size: Z.label, weight: 600, spacing: 1.6, fill: C.muted });
    cv.text(x + pad, y + 92, { text: sc.factor, size: Z.factor, weight: 700, fill: C.accent });
    cv.text(x + pad, y + 122, { text: sc.headline, size: Z.scaleHead, weight: 500, fill: C.text });
    cv.text(x + pad, y + 146, { text: sc.versus, size: Z.note, fill: C.muted });
    sc.rows.forEach((row, i) => {
      const yy = y + rowsTop + i * rowGap;
      cv.text(rowsX, yy + 16, { text: row.name, size: Z.scaleRow, mono: !row.nextrs, fill: C.soft });
      bar(rowsX + labelW, yy, barW, 22, row);
      cv.text(rowsX + labelW + Math.max(22, (row.value / max) * barW) + 12, yy + 16, { text: row.text, size: Z.scaleRow, weight: 700, fill: C.text });
    });
    lines.forEach((l, i) => cv.text(x + pad, y + noteY + i * (Z.scaleNote + 5), { text: l, size: Z.scaleNote, fill: C.muted }));
    return h;
  }
  const rowGap = 56;
  const rowsTop = 158;
  const noteY = rowsTop + 2 * rowGap + 10 + 16 + 34;
  const h = noteY + (lines.length - 1) * (Z.scaleNote + 5) + 24;
  panel(h);
  cv.text(x + pad, y + 38, { text: "SCALE ACROSS V8 ISOLATES", size: Z.label, weight: 600, spacing: 1.6, fill: C.muted });
  cv.text(x + pad, y + 92, [
    { text: sc.factor, size: Z.factor, weight: 700, fill: C.accent },
    { text: sc.headline, size: Z.scaleHead, weight: 500, fill: C.text, dx: 12 },
  ]);
  cv.text(x + pad, y + 120, { text: sc.versus, size: Z.note, fill: C.muted });
  sc.rows.forEach((row, i) => {
    const yy = y + rowsTop + i * rowGap;
    cv.text(x + pad, yy, { text: row.name, size: Z.scaleRow, mono: !row.nextrs, fill: C.soft });
    cv.text(x + w - pad, yy, { text: row.text, size: Z.scaleRow, weight: 700, fill: C.text }, { anchor: "end" });
    bar(x + pad, yy + 10, inner, 16, row);
  });
  lines.forEach((l, i) => cv.text(x + pad, y + noteY + i * (Z.scaleNote + 5), { text: l, size: Z.scaleNote, fill: C.muted }));
  return h;
}

export function render(model, layout = "wide") {
  const narrow = layout === "narrow";
  const Z = SIZES[narrow ? "narrow" : "wide"];
  const { width, margin: m } = Z;
  const inner = width - 2 * m;
  const cv = canvas();
  const { text } = cv;

  // Header
  let y = narrow ? 58 : 66;
  text(m, y, { text: model.eyebrow, size: Z.eyebrow, weight: 600, mono: true, spacing: 2, fill: C.accent });
  y += Z.title + 10;
  text(m, y, [
    { text: model.title.lead, size: Z.title, weight: 700, fill: C.text },
    { text: model.title.code, size: Z.title, weight: 600, mono: true, fill: C.text },
  ]);
  y += 4;
  for (const l of wrap(model.subtitle, Z.sub, inner)) {
    y += Z.sub + 12;
    text(m, y, { text: l, size: Z.sub, fill: C.soft });
  }
  y += 2;
  for (const l of wrap(model.meta, Z.meta, inner)) {
    y += Z.meta + 10;
    text(m, y, { text: l, size: Z.meta, fill: C.muted });
  }
  // Legend: top right on desktop, below the meta line on mobile.
  const legendW = model.legend.reduce((a, it) => a + 24 + textWidth(it.name, Z.legend, { mono: !it.nextrs }) + 26, -26);
  let lx = narrow ? m : width - m - legendW;
  const ly = narrow ? (y += Z.legend + 22) : 66;
  for (const item of model.legend) {
    cv.rect(lx, ly - Z.legend * 0.75, 15, 15, ` rx="3" fill="${item.nextrs ? C.nextrs : C.next}"`, "swatch");
    lx += 24 + text(lx + 24, ly, { text: item.name, size: Z.legend, mono: !item.nextrs, fill: C.soft }) + 26;
  }

  // Metric cards
  y += narrow ? 30 : 36;
  const cw = (inner - (Z.cols - 1) * Z.gap) / Z.cols;
  const captionLines = Math.max(...model.metrics.map((mm) => wrap(mm.caption, Z.caption, cw - 2 * Z.pad).length));
  const L = cardLayout(Z, captionLines);
  model.metrics.forEach((mm, i) => metricCard(cv, m + (i % Z.cols) * (cw + Z.gap), y + Math.floor(i / Z.cols) * (L.height + Z.gap), cw, L, mm, Z));
  y += Math.ceil(model.metrics.length / Z.cols) * (L.height + Z.gap) - Z.gap;

  // Isolate scaling: secondary, below the headline metrics.
  if (model.scaling) {
    y += Z.gap;
    y += scalePanel(cv, m, y, inner, model.scaling, narrow, Z);
  }

  // Footer
  y += Z.foot + 22;
  const bullet = (bx, by) => cv.out.push(`<circle cx="${bx + 3}" cy="${r(by - Z.foot * 0.33)}" r="2.5" fill="${C.muted}"/>`);
  const lineH = Z.foot + 9;
  if (narrow) {
    for (const item of model.items) {
      wrap(item, Z.foot, inner - 16).forEach((l, i) => {
        if (i === 0) bullet(m, y);
        text(m + 16, y, { text: l, size: Z.foot, fill: C.soft });
        y += lineH;
      });
    }
  } else {
    const colW = inner / 2;
    model.items.forEach((item, i) => {
      const fx = m + (i % 2) * colW;
      const fy = y + Math.floor(i / 2) * lineH;
      bullet(fx, fy);
      text(fx + 16, fy, { text: item, size: Z.foot, fill: C.soft });
    });
    y += Math.ceil(model.items.length / 2) * lineH;
  }
  y += 8;
  for (const l of wrap(model.disclaimer, Z.disclaimer, inner)) {
    text(m, y, { text: l, size: Z.disclaimer, fill: C.muted });
    y += Z.disclaimer + 8;
  }
  for (const l of wrap(model.source, Z.source, inner, { mono: true })) {
    text(m, y, { text: l, size: Z.source, mono: true, fill: C.muted });
    y += Z.source + 7;
  }
  for (const l of wrap(model.provenance, Z.source, inner)) {
    text(m, y, { text: l, size: Z.source, fill: C.muted });
    y += Z.source + 7;
  }
  const height = Math.round(y + 12);

  const head =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" role="img" aria-labelledby="bench-title bench-desc">` +
    `<title id="bench-title">NextRS vs next start: ${esc(model.workloadTitle)} benchmark, ${esc(model.date)}</title>` +
    `<desc id="bench-desc">${esc(model.desc)}</desc>` +
    `<defs><pattern id="stripes" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="8" height="8" fill="${C.nextrs}"/><rect width="3" height="8" fill="${C.nextrsStripe}" fill-opacity="0.45"/></pattern></defs>` +
    `<style>text{font-family:${SANS}}.m{font-family:${MONO}}</style>` +
    `<rect x="1" y="1" width="${width - 2}" height="${height - 2}" rx="24" fill="${C.bg}" stroke="${C.border}" stroke-width="2"/>`;
  return { svg: `${head}${cv.out.join("")}</svg>\n`, boxes: cv.boxes, width, height, margin: m };
}

// -------------------------------------------------------------------- main

function main() {
  const argv = process.argv.slice(2);
  const dir = argv[0];
  if (!dir || dir.startsWith("--")) throw new Error("usage: chart.mjs RESULT_DIR [--workload group/name] [--configs a,b,c] [--out DIR]");
  const opt = {};
  for (let i = 1; i < argv.length; i++) {
    if (!argv[i].startsWith("--")) throw new Error(`unexpected argument ${argv[i]}`);
    opt[argv[i].slice(2)] = argv[++i];
  }
  const S = JSON.parse(readFileSync(join(dir, "summary.json"), "utf8"));
  const workload = opt.workload ?? "app/ssr";
  const configs = (opt.configs ?? "next-start,nextrs-i1,nextrs-i4").split(",");
  const [g, w] = workload.split("/");
  const rowsPath = join(dir, "rows.jsonl");
  const connections =
    opt.connections != null ? Number(opt.connections)
    : existsSync(rowsPath) ? connectionsFrom(readFileSync(rowsPath, "utf8"), g, w, configs)
    : null;
  const model = buildModel(S, {
    workload, configs, connections, version: opt.version, path: opt.path,
    rawUrl: opt["raw-url"] ?? dir.replace(/\/+$/, ""),
  });
  const outDir = opt.out ?? ".github/assets";
  const name = opt.name ?? "nextrs-benchmarks";
  writeFileSync(join(outDir, `${name}.svg`), render(model, "wide").svg);
  writeFileSync(join(outDir, `${name}-mobile.svg`), render(model, "narrow").svg);
  console.log(model.desc);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
