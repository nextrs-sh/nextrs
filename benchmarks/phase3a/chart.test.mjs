// Tests for the README benchmark chart generator (chart.mjs):
//
//   node --test benchmarks/phase3a/chart.test.mjs
//
// They use the committed Phase 3C result that the README chart is drawn
// from, and check that the committed SVGs are exactly what the generator
// produces from it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  buildModel, render, improvement, configName, connectionsFrom, runScope, workloadCount, plural,
  dateLabel, cpuLabel, cpuCount, esc, fmt, group, wrap, textWidth,
} from "./chart.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");
const RUN = "benchmarks/results/phase3c/final-bench-f5c480d/readme";
const SUMMARY = JSON.parse(readFileSync(join(ROOT, RUN, "summary.json"), "utf8"));
const ROWS = readFileSync(join(ROOT, RUN, "rows.jsonl"), "utf8");
const CONFIGS = ["next-start", "nextrs-i1", "nextrs-i4"];
const clone = () => structuredClone(SUMMARY);
const model = (S = SUMMARY, o = {}) => buildModel(S, { workload: "app/ssr", configs: CONFIGS, connections: 32, rawUrl: RUN, ...o });
const metric = (M, key) => M.metrics.find((m) => m.key === key);

// Minimal XML well-formedness: balanced tags and no bare "&" or "<".
function assertWellFormed(svg) {
  const stack = [];
  const body = svg.replace(/<tspan\b/g, "<tspan");
  for (const m of body.matchAll(/<(\/?)([a-zA-Z][\w:-]*)([^>]*?)(\/?)>/g)) {
    const [, close, name, , self] = m;
    if (self) continue;
    if (close) assert.equal(stack.pop(), name, `unbalanced </${name}>`);
    else stack.push(name);
  }
  assert.deepEqual(stack, [], "unclosed tags");
  const text = body.replace(/<[^>]*>/g, "");
  assert.doesNotMatch(text, /&(?!(amp|lt|gt|quot|#39);)/, "bare & in text");
  assert.doesNotMatch(text, /[<>]/, "bare < or > in text");
}

// Every text inside the canvas margins, no text overlapping other text or a
// bar (estimated widths; see textWidth).
function assertLayout({ boxes, width, height, margin }) {
  for (const b of boxes) {
    assert.ok(b.x0 >= margin - 0.5 && b.x1 <= width - margin + 0.5, `outside the margins: ${b.text ?? b.kind} [${b.x0.toFixed(1)}, ${b.x1.toFixed(1)}]`);
    assert.ok(b.top >= 0 && b.bottom <= height, `clipped vertically: ${b.text ?? b.kind}`);
  }
  const overlap = (a, b) => a.x0 < b.x1 && b.x0 < a.x1 && a.top < b.bottom && b.top < a.bottom;
  for (let i = 0; i < boxes.length; i++)
    for (let j = i + 1; j < boxes.length; j++) {
      const [a, b] = [boxes[i], boxes[j]];
      if (a.kind !== "text" && b.kind !== "text") continue;
      assert.ok(!overlap(a, b), `overlap: "${a.text ?? a.kind}" and "${b.text ?? b.kind}"`);
    }
}

test("improvement: higher is better", () => {
  const t = improvement(611.1207840183803, 735.8812405418764, "higher");
  assert.equal(t.status, "better");
  assert.equal(t.rounded, 20);
  assert.ok(Math.abs(t.pct - 20.415) < 0.001);
  assert.equal(improvement(100, 80, "higher").rounded, -20);
  assert.equal(improvement(100, 80, "higher").status, "worse");
});

test("improvement: lower is better", () => {
  assert.equal(improvement(6.263, 3.297, "lower").rounded, 47);
  assert.equal(improvement(375.6, 140.7, "lower").rounded, 63); // 62.54 %
  assert.equal(improvement(1.922447643979057, 1.7458333333333338, "lower").rounded, 9);
  const worse = improvement(4.12, 5.07, "lower");
  assert.equal(worse.status, "worse");
  assert.equal(worse.rounded, -23);
  assert.equal(improvement(10, 10.04, "lower").status, "equal");
  assert.equal(improvement(100, 100.5, "higher").rounded, 1); // half away from zero
  assert.throws(() => improvement(0, 1, "lower"));
  assert.throws(() => improvement(1, 1, "sideways"));
});

test("the four headline metrics, from the medians", () => {
  const M = model();
  const W = SUMMARY.workloads[0];
  const expect = {
    throughput: ["+20%", "", "Higher throughput", "736 req/s", "611 req/s", ["closed", "rps"]],
    latency: ["47%", "lower", "p95 response latency", "3.30 ms", "6.26 ms", ["open", "p95_ms"]],
    memory: ["63%", "less", "Anonymous memory", "141 MiB", "376 MiB", ["closed", "anon_end_mib"]],
    cpu: ["9%", "less", "CPU time per request", "1.75 ms", "1.92 ms", ["closed", "cpu_ms_per_req"]],
  };
  for (const [key, [value, suffix, caption, nextrs, next, [mode, field]]] of Object.entries(expect)) {
    const m = metric(M, key);
    assert.equal(m.headline.value, value, key);
    assert.equal(m.headline.suffix, suffix, key);
    assert.equal(m.caption, caption, key);
    assert.equal(m.status, "better", key);
    assert.equal(m.rows[0].text, nextrs, key);
    assert.equal(m.rows[1].text, next, key);
    // The summary's own ratio (median / median) agrees with the one drawn.
    assert.ok(Math.abs(W.ratios["nextrs-i1"][mode][field].ratio - m.ratio) < 1e-12, key);
  }
  assert.equal(M.scaling.factor, "3.11×");
  assert.ok(Math.abs(W.ratios["nextrs-i4"].closed.rps.ratio - M.scaling.ratio) < 1e-12);
  assert.deepEqual(M.scaling.rows.map((r) => r.text), ["611 req/s", "736 req/s", "1,898 req/s"]);
});

test("a worse result or one within noise is not drawn as an advantage", () => {
  const S = clone();
  const W = S.workloads[0];
  W.configs["nextrs-i1"].open.p95_ms = 7.5;
  W.ratios["nextrs-i1"].closed.cpu_ms_per_req.significant = false;
  const M = model(S);
  assert.equal(metric(M, "latency").headline.value, "20%");
  assert.equal(metric(M, "latency").headline.suffix, "higher");
  assert.equal(metric(M, "latency").status, "worse");
  assert.equal(metric(M, "cpu").status, "noise");
  assert.match(metric(M, "cpu").caption, /within noise/);
  const { svg } = render(M);
  assert.ok(!svg.includes(`fill="#FF7B45">20%`), "a regression must not wear the accent colour");
});

test("missing values are errors, not zeros", () => {
  const cases = [
    [(W) => delete W.configs["nextrs-i1"].open.p95_ms, /missing latency \(p95_ms\) for nextrs-i1/],
    [(W) => (W.configs["next-start"].closed.anon_end_mib = null), /missing memory \(anon_end_mib\) for next-start/],
    [(W) => delete W.configs["nextrs-i4"].closed.rps, /missing throughput \(rps\) for nextrs-i4/],
    [(W) => (W.configs["nextrs-i4"].open.offered_rate = 400), /offered rates differ/],
    [(W) => delete W.configs["nextrs-i4"], /no configuration nextrs-i4/],
  ];
  for (const [mutate, re] of cases) {
    const S = clone();
    mutate(S.workloads[0]);
    assert.throws(() => model(S), re);
  }
  assert.throws(() => model(SUMMARY, { connections: null }), /connection count/);
  assert.throws(() => model(SUMMARY, { workload: "app/nope" }), /no workload/);
  const S = clone();
  delete S.environment.fixtures.app.next_version;
  assert.throws(() => model(S), /Next.js version/);
});

test("one- and four-isolate labels", () => {
  const W = SUMMARY.workloads[0];
  assert.equal(configName(W, "next-start"), "next start");
  assert.equal(configName(W, "nextrs-i1"), "NextRS · 1 isolate");
  assert.equal(configName(W, "nextrs-i4"), "NextRS · 4 isolates");
  assert.equal(configName({}, "current-i2"), "NextRS · 2 isolates");
  const M = model();
  assert.deepEqual(M.legend.map((l) => l.name), ["NextRS · 1 isolate", "next start"]);
  assert.equal(M.scaling.note, "Four isolates share the same four-CPU server allocation; the Node baseline is one server process.");
  assert.equal(M.scaling.versus, "NextRS · 4 isolates vs next start");
  // Without a third configuration there is no scaling panel, and nothing
  // in the chart mentions four isolates.
  const two = model(SUMMARY, { configs: ["next-start", "nextrs-i1"] });
  assert.equal(two.scaling, null);
  assert.ok(!render(two).svg.includes("4 isolates"));
  // Memory is only ever shown for one isolate.
  assert.deepEqual(metric(M, "memory").rows.map((r) => r.config), ["nextrs-i1", "next-start"]);
});

test("singular and plural wording", () => {
  assert.equal(workloadCount(1), "1 workload");
  assert.equal(workloadCount(24), "24 workloads");
  assert.equal(runScope(1), "Its run measured this one workload. ");
  assert.match(runScope(24), /1 of the 24 workloads/);
  assert.equal(plural(1, "interleaved round"), "1 interleaved round");
  const S = clone();
  S.workloads[0].configs["next-start"].closed.rounds = 1;
  assert.ok(model(S).items.includes("Same Next.js build · 1 interleaved round"));
  assert.ok(model().items.includes("Same Next.js build · 3 interleaved rounds"));
});

test("source metadata comes from the run", () => {
  const M = model();
  assert.equal(M.eyebrow, "EXPERIMENTAL BENCHMARK · NOV 18, 2026");
  assert.equal(M.subtitle, "Dynamic App Router SSR · Same Next.js build · Measured results");
  assert.equal(M.meta, "Next.js 16.4.6 · Node.js 24.18.0 · NextRS f5c480d · /books/[id]");
  assert.deepEqual(M.items, [
    "Same Next.js build · 3 interleaved rounds",
    "32 connections for throughput/CPU/memory",
    "Open-loop p95 at 305 req/s",
    "Intel Core i5-9400F · Single host",
  ]);
  assert.equal(M.source, `Raw data: ${RUN}`);
  // The measured tree was not clean: the commit alone does not identify the
  // binary, so the chart names the binary's checksum too.
  assert.equal(SUMMARY.environment.nextrs_dirty, true);
  assert.equal(M.provenance, "NextRS f5c480d (working tree had uncommitted changes) · binary sha256 3597a25176ab… · pre-release build");
  const S = clone();
  S.environment.nextrs_dirty = false;
  assert.ok(!model(S).provenance.includes("uncommitted"));
  assert.equal(model(SUMMARY, { version: "9.9.9" }).meta, "Next.js 16.4.6 · Node.js 24.18.0 · NextRS 9.9.9 · /books/[id]");
  assert.equal(connectionsFrom(ROWS, "app", "ssr", CONFIGS), 32);
  assert.throws(() => connectionsFrom(ROWS, "app", "nope", CONFIGS), /connection count/);
  assert.equal(dateLabel("2026-11-18T06:21:34.065Z"), "Nov 18, 2026");
  assert.equal(cpuLabel("Intel(R) Core(TM) i5-9400F CPU @ 2.90GHz"), "Intel Core i5-9400F");
  assert.equal(cpuCount("0-3"), 4);
  assert.equal(cpuCount("0,2,4-5"), 4);
  assert.equal(group(1898.45), "1,898");
  assert.equal(fmt(1234567, "req/s"), "1,234,567 req/s");
});

test("SVG escaping", () => {
  assert.equal(esc(`<a href="x">&'`), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
  const S = clone();
  S.workloads[0].label = `SSR <script>alert("x")</script> & co (/a?b=1&c=<2>)`;
  const M = model(S, { rawUrl: "results/<dir>&x" });
  for (const layout of ["wide", "narrow"]) {
    const { svg } = render(M, layout);
    assertWellFormed(svg);
    assert.ok(!svg.includes("<script"));
    assert.ok(svg.includes("&lt;script&gt;"));
  }
});

test("the committed SVGs are reproducible from the data", () => {
  const M = model(SUMMARY, { connections: connectionsFrom(ROWS, "app", "ssr", CONFIGS) });
  const wide = render(M, "wide").svg;
  const narrow = render(M, "narrow").svg;
  assert.equal(render(model(), "wide").svg, wide, "deterministic");
  assert.equal(readFileSync(join(ROOT, ".github/assets/nextrs-benchmarks.svg"), "utf8"), wide);
  assert.equal(readFileSync(join(ROOT, ".github/assets/nextrs-benchmarks-mobile.svg"), "utf8"), narrow);
});

test("accessible, well-formed, self-contained SVG", () => {
  for (const layout of ["wide", "narrow"]) {
    const { svg } = render(model(), layout);
    assertWellFormed(svg);
    assert.match(svg, /^<svg xmlns="http:\/\/www.w3.org\/2000\/svg" width="(1280|720)" height="\d+" viewBox="0 0 \1 \d+" role="img" aria-labelledby="bench-title bench-desc">/);
    assert.match(svg, /<title id="bench-title">NextRS vs next start: Dynamic App Router SSR benchmark, Nov 18, 2026<\/title>/);
    const desc = svg.match(/<desc id="bench-desc">([^<]*)<\/desc>/)[1];
    for (const s of ["736 req/s", "611 req/s", "3.30 ms", "6.26 ms", "141 MiB", "376 MiB", "1.75 ms", "1.92 ms", "1,898 req/s", "3.11×", "f5c480d", "16.4.6", "24.18.0"]) assert.ok(desc.includes(s), s);
    assert.doesNotMatch(svg, /<script|<image|<foreignObject|<animate|@import|url\((?!#)|xlink:href|href=/);
  }
});

test("layout: no clipping or overlap, bars from zero and proportional", () => {
  for (const layout of ["wide", "narrow"]) {
    const out = render(model(), layout);
    assertLayout(out);
    const bars = out.boxes.filter((b) => b.kind === "bar");
    // Four cards of two bars, then three scaling bars.
    assert.equal(bars.length, 11);
    const M = model();
    M.metrics.forEach((m, i) => {
      const [a, b] = [bars[2 * i], bars[2 * i + 1]];
      assert.equal(a.x0, b.x0, `${m.key}: bars share a zero baseline`);
      const max = Math.max(m.rows[0].value, m.rows[1].value);
      const full = Math.max(a.x1 - a.x0, b.x1 - b.x0);
      assert.ok(Math.abs((a.x1 - a.x0) / full - m.rows[0].value / max) < 0.005, `${m.key} proportional`);
      assert.ok(Math.abs((b.x1 - b.x0) / full - m.rows[1].value / max) < 0.005, `${m.key} proportional`);
    });
    const sc = bars.slice(8);
    assert.ok(Math.abs((sc[0].x1 - sc[0].x0) / (sc[2].x1 - sc[2].x0) - 611.1207840183803 / 1898.4521730158112) < 0.005);
  }
});

test("layout: long values do not overflow", () => {
  const S = clone();
  const W = S.workloads[0];
  W.configs["nextrs-i4"].closed.rps = 123456789;
  W.configs["nextrs-i1"].closed.rps = 98765432;
  W.configs["next-start"].closed.anon_end_mib = 1234567;
  W.configs["nextrs-i1"].open.p95_ms = 12345.6;
  W.configs["next-start"].open.p95_ms = 23456.7;
  delete W.ratios;
  const M = model(S);
  assert.equal(metric(M, "memory").headline.value, ">99%", "a 99.99 % reduction is not shown as 100 %");
  assert.equal(M.scaling.factor, "202,017×");
  for (const layout of ["wide", "narrow"]) assertLayout(render(M, layout));
});

test("wrap keeps every line within its width", () => {
  const t = "NextRS f5c480d (working tree had uncommitted changes) · binary sha256 3597a25176ab… · pre-release build";
  for (const max of [200, 400, 640]) for (const l of wrap(t, 15, max)) assert.ok(textWidth(l, 15) <= max || !l.includes(" "), l);
});
