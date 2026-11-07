// OpenTelemetry through @vercel/otel (instrumentation.ts), compared between
// next start and NextRS on fixtures/next-otel; both export OTLP JSON to
// tests/compat/otel-collector.mjs.
//
//   node tests/compat/otel.mjs --baseline URL --candidate URL --collector URL [--isolates N] [--json out.json]
//
// Compared: the shape of each request's exported trace (span names, kinds,
// scopes and which span is whose parent). Asserted on both servers: the
// incoming traceparent is continued, the outgoing fetch carries it, spans
// nest across awaits and timers, concurrent traces never mix, export
// failures do not fail requests, and registration happens once per
// runtime instance.
import { randomBytes } from "node:crypto";
import { parseArgs, runScenarios, sleep } from "./routing-lib.mjs";

const args = parseArgs();
const { baseline, candidate, collector } = args;
if (!baseline || !candidate || !collector) {
  console.error("usage: otel.mjs --baseline URL --candidate URL --collector URL [--json FILE]");
  process.exit(2);
}
const only = args.only ? new Set(args.only.split(",")) : null;

const hex = (n) => randomBytes(n).toString("hex");
const traceparent = () => {
  const trace = hex(16);
  const span = hex(8);
  return { trace, span, header: `00-${trace}-${span}-01` };
};

// The spans of one trace, once its last expected span has arrived.
async function spansOf(traceId, until = (spans) => spans.some((s) => s.name === "work-inner"), timeout = 10000) {
  const deadline = Date.now() + timeout;
  let spans = [];
  while (Date.now() < deadline) {
    spans = await (await fetch(`${collector}/spans?traceId=${traceId}`)).json();
    if (until(spans)) {
      await sleep(300);
      return (await fetch(`${collector}/spans?traceId=${traceId}`)).json();
    }
    await sleep(100);
  }
  return spans;
}
// The tree as names: each span with its parent's name (or "remote" for
// the incoming context, "root" for none), sorted.
function shape(spans, remoteSpanId) {
  const byId = new Map(spans.map((s) => [s.spanId, s]));
  return spans
    .map((s) => {
      const parent = s.parentSpanId === null ? "root" : s.parentSpanId === remoteSpanId ? "remote" : byId.get(s.parentSpanId)?.name ?? "missing";
      return `${s.scope ?? "?"} ${s.kind} ${s.name} <- ${parent}`;
    })
    .sort();
}
// Spans of the server around the application, not of the application:
// next start's router-server root span ("GET", around everything when a
// trace comes in) and base-server's component loading; NextRS routes in
// Rust and loads route modules itself, and a fresh NextRS isolate
// registers instrumentation inside its first request (next start does it
// at startup). The application's tree (proxy, route, render, fetch and its
// own spans) is compared with these taken out, each child re-parented to
// the nearest span that stays; the full trees are a documented difference
// (scenario "server-level-spans").
const SERVER_ONLY = /^(GET|POST|resolve page components|load route module|load app route module|register instrumentation|load instrumentation module)$/;
function appShape(spans, remoteSpanId) {
  const byId = new Map(spans.map((s) => [s.spanId, s]));
  const kept = (s) => !(s.scope === "next.js" && SERVER_ONLY.test(s.name));
  const parentOf = (s) => {
    let id = s.parentSpanId;
    for (;;) {
      if (id === null) return "root";
      if (id === remoteSpanId) return "remote";
      const p = byId.get(id);
      if (!p) return "missing";
      if (kept(p)) return p.name;
      id = p.parentSpanId;
    }
  };
  // root and remote are one: next start's router span stands between
  // the incoming context and the rest; without one both start a trace.
  return spans.filter(kept).map((s) => `${s.scope ?? "?"} ${s.kind} ${s.name} <- ${parentOf(s).replace(/^(root|remote)$/, "outside")}`).sort();
}
const workChecks = (o, incoming) => {
  const b = o.body;
  if (o.status !== 200) return `status ${o.status}`;
  if (incoming && b.traceId !== incoming.trace) return `trace ${b.traceId}, expected the incoming ${incoming.trace}`;
  if (!b.upstreamTraceparent?.includes(b.traceId)) return `the upstream saw traceparent ${b.upstreamTraceparent}`;
  if (b.proxyTrace !== b.traceId) return `the proxy's span was in trace ${b.proxyTrace}`;
  const work = o.spans.find((s) => s.name === "work");
  const inner = o.spans.find((s) => s.name === "work-inner");
  if (!work || !inner) return `exported spans lack work/work-inner: ${JSON.stringify(o.spanNames)}`;
  if (work.spanId !== b.workSpanId || inner.spanId !== b.innerSpanId) return "exported span ids differ from the ones the route saw";
  if (inner.parentSpanId !== work.spanId) return "work-inner is not a child of work (context lost across the timer)";
  if (b.rootSpanId && work.parentSpanId !== b.rootSpanId) return "work is not a child of the request's span";
  if (!o.spans.some((s) => s.parentSpanId === work.spanId && s.scope?.includes("fetch"))) return "no fetch span under work";
  if (incoming && !o.spans.some((s) => s.parentSpanId === incoming.span)) return "no span continues the incoming context";
  if (o.spans.some((s) => s.traceId !== b.traceId)) return "a span of another trace";
  return null;
};

const scenarios = {};
async function work(base, { incoming, label = "a" } = {}) {
  const headers = incoming ? { traceparent: incoming.header } : {};
  const r = await fetch(`${base}/api/work?label=${label}`, { headers });
  const body = await r.json().catch(() => ({}));
  const spans = body.traceId ? await spansOf(body.traceId) : [];
  return { status: r.status, body, spans, spanNames: spans.map((s) => s.name) };
}
const observed = (o, incoming) => ({ status: o.status, appShape: appShape(o.spans, incoming?.span) });

scenarios["registration-once-per-instance"] = {
  kind: "compare",
  expect: (o) => (o.registrations.every((n) => n === 1) ? null : `registrations ${o.registrations}`),
  run: async (base) => {
    const seen = [];
    for (let i = 0; i < 12; i++) seen.push((await (await fetch(`${base}/api/otel-state`)).json()).registrations);
    return { registrations: [...new Set(seen)] };
  },
};
scenarios["continues-incoming-trace"] = {
  kind: "compare",
  run: async (base) => {
    const incoming = traceparent();
    const o = await work(base, { incoming });
    return { ...observed(o, incoming), wrong: workChecks(o, incoming) };
  },
  expect: (o) => o.wrong,
};
scenarios["starts-a-trace"] = {
  kind: "compare",
  run: async (base) => {
    const o = await work(base);
    return { ...observed(o), wrong: workChecks(o), fresh: /^[0-9a-f]{32}$/.test(o.body.traceId ?? "") };
  },
  expect: (o) => o.wrong ?? (o.fresh ? null : "no trace id"),
};
scenarios["page-render"] = {
  kind: "compare",
  expect: (o) => (o.status === 200 && o.continued && o.hasRenderWork ? null : JSON.stringify(o)),
  run: async (base) => {
    const incoming = traceparent();
    const r = await fetch(`${base}/page-traced`, { headers: { traceparent: incoming.header } });
    const html = await r.text();
    const info = JSON.parse((/id="info">([^<]*)</.exec(html)?.[1] ?? "{}").replace(/&quot;/g, '"'));
    const spans = await spansOf(incoming.trace, (s) => s.some((x) => x.name === "render-work") && s.some((x) => x.parentSpanId === incoming.span));
    return {
      status: r.status,
      continued: info.traceId === incoming.trace && info.proxyTrace === incoming.trace,
      hasRenderWork: spans.some((s) => s.name === "render-work"),
      appShape: appShape(spans, incoming.span),
    };
  },
};
scenarios["server-level-spans"] = {
  kind: "known-diff",
  reason:
    "next start's router-server and base-server spans (a root GET around an incoming trace, resolve page components, load route module) have no NextRS counterpart; a fresh NextRS isolate registers instrumentation inside its first request",
  expectCandidate: (o) => (o.status === 200 && o.full.length > 0 ? null : JSON.stringify(o)),
  run: async (base) => {
    const incoming = traceparent();
    const o = await work(base, { incoming, label: "server-spans" });
    return { status: o.status, full: shape(o.spans, incoming.span) };
  },
};
scenarios["concurrent-traces"] = {
  kind: "compare",
  expect: (o) => (o.total === 40 && o.wrong === 0 && o.mixed === 0 ? null : JSON.stringify(o)),
  run: async (base) => {
    const jobs = Array.from({ length: 40 }, (_, i) => ({ incoming: traceparent(), label: `c${i}` }));
    const results = await Promise.all(jobs.map((job) => fetch(`${base}/api/work?label=${job.label}`, { headers: { traceparent: job.incoming.header } }).then((r) => r.json())));
    let wrong = 0;
    let mixed = 0;
    for (const [i, job] of jobs.entries()) {
      if (results[i].traceId !== job.incoming.trace || results[i].label !== job.label) wrong++;
      const spans = await spansOf(job.incoming.trace);
      const labels = spans.filter((s) => s.name === "work").map((s) => s.attributes["fixture.label"]);
      if (labels.length !== 1 || labels[0] !== job.label) mixed++;
    }
    return { total: jobs.length, wrong, mixed };
  },
};
scenarios["exporter-failure"] = {
  kind: "compare",
  expect: (o) => (o.statuses.every((s) => s === 200) && o.recovered ? null : JSON.stringify(o)),
  run: async (base) => {
    await fetch(`${collector}/admin/fail?on=1`, { method: "POST" });
    const statuses = [];
    try {
      for (let i = 0; i < 10; i++) statuses.push((await fetch(`${base}/api/work?label=f${i}`)).status);
      await sleep(1500);
    } finally {
      await fetch(`${collector}/admin/fail?on=0`, { method: "POST" });
    }
    const after = await work(base, { incoming: traceparent(), label: "after-failure" });
    return { statuses: [...new Set(statuses)], recovered: after.spans.some((s) => s.name === "work-inner") };
  },
};
// NextRS restarts an executor that stalls past its limit; the new one
// registers again (once) and exports. next start only runs the checks.
scenarios["after-executor-restart"] = {
  kind: "compare",
  expect: (o) => o.wrong ?? (o.registrations.every((n) => n === 1) ? null : `registrations ${o.registrations}`),
  run: async (base, side) => {
    if (side === "candidate") {
      await fetch(`${base}/api/stall?ms=6000`).then((r) => r.arrayBuffer()).catch(() => null);
      await sleep(800);
    }
    const incoming = traceparent();
    const o = await work(base, { incoming, label: "restart" });
    const registrations = [];
    for (let i = 0; i < 8; i++) registrations.push((await (await fetch(`${base}/api/otel-state`)).json()).registrations);
    return { ...observed(o, incoming), wrong: workChecks(o, incoming), registrations: [...new Set(registrations)] };
  },
};

const failures = await runScenarios("otel (@vercel/otel)", scenarios, { baseline, candidate, json: args.json, only });
process.exit(failures ? 1 : 0);
