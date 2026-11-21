#!/usr/bin/env node
// Turn Playwright's JSON report into PASS / FAIL / BLOCKED / NOT TESTED per test and project.
//   node smoke/summarize.mjs results/smoke/results.json > results/smoke/summary.md
// BLOCKED = skipped because an earlier step it depends on failed (test.skip with a reason, or a
// serial group stopped after a failure). NOT TESTED = project/browser not run.
import { readFileSync } from 'node:fs';

const report = JSON.parse(readFileSync(process.argv[2], 'utf8'));
const rows = [];
const walk = (suite, trail) => {
  const here = suite.title && !suite.title.endsWith('.ts') ? [...trail, suite.title] : trail;
  for (const spec of suite.specs ?? []) {
    for (const t of spec.tests) {
      const r = t.results.at(-1);
      let status;
      if (!r) status = 'NOT TESTED';
      else if (r.status === 'passed') status = 'PASS';
      else if (r.status === 'skipped') status = t.annotations?.some((a) => a.type === 'skip' && a.description) || t.results.length ? 'BLOCKED' : 'NOT TESTED';
      else status = 'FAIL';
      if (r?.status === 'skipped' && !t.annotations?.some((a) => a.type === 'skip')) status = 'BLOCKED';
      const error = r?.errors?.[0]?.message ?? r?.error?.message ?? '';
      rows.push({ project: t.projectName, group: here.join(' › '), title: spec.title, status, ms: r?.duration ?? 0,
        error: error.replace(/\u001b\[[0-9;]*m/g, '').split('\n').filter(Boolean).slice(0, 4).join(' ⏎ ').slice(0, 400) });
    }
  }
  for (const s of suite.suites ?? []) walk(s, here);
};
for (const s of report.suites) walk(s, []);

const projects = [...new Set(rows.map((r) => r.project))];
const count = (p, s) => rows.filter((r) => r.project === p && r.status === s).length;
console.log(`# Smoke test summary\n\nGenerated ${new Date().toISOString()} from ${process.argv[2]}\n`);
console.log('| Browser | PASS | FAIL | BLOCKED | NOT TESTED |\n|---|---|---|---|---|');
for (const p of projects) console.log(`| ${p} | ${count(p, 'PASS')} | ${count(p, 'FAIL')} | ${count(p, 'BLOCKED')} | ${count(p, 'NOT TESTED')} |`);
for (const p of projects) {
  console.log(`\n## ${p}\n\n| Result | Area | Test | Time | Detail |\n|---|---|---|---|---|`);
  for (const r of rows.filter((x) => x.project === p))
    console.log(`| ${r.status} | ${r.group} | ${r.title} | ${(r.ms / 1000).toFixed(1)}s | ${r.error.replace(/\|/g, '\\|')} |`);
}
let probes = 0;
const countProbes = (suite) => {
  for (const spec of suite.specs ?? []) for (const t of spec.tests) for (const r of t.results)
    probes += (r.annotations ?? t.annotations ?? []).filter((a) => a.type === 'app-csp-eval-probe').length;
  for (const s of suite.suites ?? []) countProbes(s);
};
report.suites.forEach(countProbes);
console.log(`\n## Known app-level console messages (not failures)\n\n` +
  `- zod v4 \`Function("")\` eval probe blocked by Velasia's CSP (logged by Firefox only): **${probes}** occurrence(s) annotated. ` +
  `Client-side behaviour of the \`next build\` output; independent of the server runtime.`);
const failed = rows.filter((r) => r.status === 'FAIL').length;
process.exitCode = failed ? 1 : 0;
