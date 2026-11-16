#!/usr/bin/env node
// Per-test outcome matrix across certification runs.
//
//   matrix.mjs [--app velasia-airways] [run-label ...]
//
// Reads results/<app>/<label>/summary.json for each label (all runs when
// none are given) and prints a Markdown table: one row per test, one column
// per run (pass, FAIL, skip), with totals.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const appAt = args.indexOf('--app');
const app = appAt === -1 ? 'velasia-airways' : args.splice(appAt, 2)[1];
const root = join(here, '../results', app);
const labels = args.length ? args : readdirSync(root).filter((d) => existsSync(join(root, d, 'summary.json')));
const runs = labels.map((label) => ({ label, summary: JSON.parse(readFileSync(join(root, label, 'summary.json'), 'utf8')) }));

const key = (t) => `${t.file} › ${t.title} [${t.project}]`;
const rows = new Map();
for (const { label, summary } of runs) {
  for (const t of summary.tests ?? []) {
    if (!rows.has(key(t))) rows.set(key(t), {});
    rows.get(key(t))[label] = t.status;
  }
}
const cell = (s) => ({ expected: 'pass', unexpected: '**FAIL**', flaky: 'flaky', skipped: 'skip' })[s] ?? '–';
console.log(`| Test | ${labels.join(' | ')} |`);
console.log(`|---|${labels.map(() => '---').join('|')}|`);
for (const [name, byRun] of [...rows].sort()) {
  console.log(`| ${name.replace(/\|/g, '\\|')} | ${labels.map((l) => cell(byRun[l])).join(' | ')} |`);
}
console.log(
  `| **Passed / total** | ${runs.map(({ summary }) => `${summary.passed}/${summary.total} (${summary.failed} failed, ${summary.skipped} skipped)`).join(' | ')} |`,
);
