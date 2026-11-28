// Summarizes `run.mjs profile --workload faults-*` directories: one row per
// (case, variant, configuration), every round's classification
// (prevented / contained / process-fatal) and what each fault records.
//
//   node benchmarks/resource/fault-summary.mjs DIR [DIR...] [--json OUT.json]
//
// Every run is counted; a round's outcome is never replaced by another's.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const args = process.argv.slice(2);
const jsonOut = args.includes("--json") ? args[args.indexOf("--json") + 1] : null;
const dirs = args.filter((a, i) => a !== "--json" && args[i - 1] !== "--json");
const f1 = (x, d = 1) => (Number.isFinite(x) ? x.toFixed(d) : "–");

const all = [];
for (const dir of dirs) {
  for (const f of readdirSync(dir).filter((f) => f.endsWith(".json") && !f.startsWith("meta") && !f.startsWith("summary"))) {
    const run = JSON.parse(readFileSync(join(dir, f), "utf8"));
    if (run.kind !== "fault") continue;
    all.push(run);
  }
}

/** What the fault table shows besides the outcome. */
function record(run) {
  const d = run.derived ?? {};
  const s = d.statuses ? Object.entries(d.statuses).map(([k, n]) => `${k}:${n}`).join(" ") : "";
  switch (run.fault) {
    case "faults-native-memory":
      return `held ${f1(d.heldMiB, 0)} MiB; first failure ${d.firstFailure ? `#${d.firstFailure.i} ${d.firstFailure.status || d.firstFailure.error}` : "none"}`;
    case "faults-js-allocation":
      return d.response ? `${d.response.status ? `HTTP ${d.response.status}` : `no response (${d.response.error ?? "?"})`}${d.response.body ? ` ${d.response.body.slice(0, 40)}` : ""}` : "–";
    case "faults-concurrent-ssr":
    case "faults-large-uploads":
      return `${s}${d.share503 != null ? `; 503 ${(100 * d.share503).toFixed(1)} %` : ""}`;
    case "faults-slow-streams":
      return `${f1(d.stalledMiB)} MiB stalled, ${f1(d.perStreamKiB, 0)} KiB/stream`;
    case "faults-queue-saturation":
      return `sat ${f1(d.saturationOkRps)}/s, offered ${f1(d.offeredRps)}/s; ${s}; max queued ${d.maxQueued ?? "–"} (${d.maxQueuedBytes ?? "–"} B); recovered ${d.recovered ?? "–"}`;
    case "faults-isolates":
      return `${(d.heapWarnings ?? []).length} heap warning(s)`;
    case "faults-pressure-cycles":
      return `memory returns: ${d.memoryReturns ?? "–"}; end-of-idle anon ${(d.cycleEnds ?? []).map((e) => f1(e.anonMiB, 0)).join(" ")} MiB`;
    default:
      return s;
  }
}

const lines = [];
for (const fault of [...new Set(all.map((r) => r.fault))].sort()) {
  lines.push(`## ${fault}`, "");
  lines.push("| Case | Variant | Config | Round | Outcome | Detail | Peak MiB (anon) | Records | Reasons |");
  lines.push("|---|---|---|---:|---|---|---:|---|---|");
  const rows = all.filter((r) => r.fault === fault)
    .sort((a, b) => a.case.localeCompare(b.case) || a.variant.label.localeCompare(b.variant.label) || a.round - b.round);
  for (const r of rows) {
    const c = r.classification ?? {};
    lines.push(`| ${r.case} | ${r.variant.label} | ${r.memory}:${r.cpus} | ${r.round} | **${c.outcome ?? "?"}** | ${c.detail ?? ""} | ${f1(r.derived?.peakMiB)} (${f1(r.derived?.peakAnonMiB)}) | ${record(r)} | ${(c.reasons ?? []).slice(0, 3).join("; ")} |`);
  }
  lines.push("");
  const tally = new Map();
  for (const r of rows) {
    const k = `${r.variant.label} ${r.memory}:${r.cpus}`;
    const t = tally.get(k) ?? { prevented: 0, contained: 0, "process-fatal": 0 };
    t[r.classification?.outcome] = (t[r.classification?.outcome] ?? 0) + 1;
    tally.set(k, t);
  }
  lines.push("| Variant | Prevented | Contained | Process-fatal |", "|---|---:|---:|---:|");
  for (const [k, t] of tally) lines.push(`| ${k} | ${t.prevented} | ${t.contained} | ${t["process-fatal"]} |`);
  lines.push("");
}
console.log(lines.join("\n") || "no fault runs");
if (jsonOut) {
  writeFileSync(jsonOut, JSON.stringify(all.map((r) => ({
    fault: r.fault, case: r.case, label: r.variant.label, memory: r.memory, cpus: r.cpus, round: r.round,
    classification: r.classification, derived: r.derived, final: r.final,
  })), null, 1));
}
