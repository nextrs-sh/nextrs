#!/usr/bin/env node
// Browser checks for the independent applications (Phase 3C): each
// application's spec (benchmarks/certification/browser/<app>.spec.ts) runs
// against `next start` and against NextRS on the same unchanged build
// (prepared by compare-app.mjs prepare).
//
//   browser-apps.mjs --apps a,b [--isolates 1] [--work DIR] [--nextrs BIN] [--velasia CLONE]
//
// Playwright comes from the Velasia clone's node_modules, as in Phase 3B.
// Results: benchmarks/certification/results/independent/<app>/browser-i<N>/.
import { spawn, spawnSync } from 'node:child_process';
import { createWriteStream, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../..');
const argv = process.argv.slice(2);
const opt = (n, d) => (argv.includes(`--${n}`) ? argv[argv.indexOf(`--${n}`) + 1] : d);
const apps = opt('apps', '').split(',').filter(Boolean);
const isolates = Number(opt('isolates', 1));
const work = resolve(opt('work', join(process.env.HOME, '.cache/nextrs-phase3b/independent')));
const nextrsBin = resolve(opt('nextrs', join(repo, 'target/release/nextrs')));
const velasia = resolve(opt('velasia', join(process.env.HOME, '.cache/nextrs-phase3b/velasia-src')));
const config = join(repo, 'benchmarks/certification/browser/playwright.config.ts');
const PORT = 3430;
const log = (...a) => console.error('[browser-apps]', ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function up(port, child, health) {
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`server exited ${child.exitCode}`);
    try {
      await (await fetch(`http://127.0.0.1:${port}${health}`, { redirect: 'manual' })).arrayBuffer();
      return;
    } catch {}
    await sleep(250);
  }
  throw new Error('server did not start');
}
const stop = (child) => new Promise((done) => {
  if (child.exitCode !== null) return done();
  child.once('exit', done);
  try { process.kill(-child.pid, 'SIGTERM'); } catch { done(); }
  setTimeout(() => { try { process.kill(-child.pid, 'SIGKILL'); } catch {} }, 10_000);
});

const summary = {};
for (const app of apps) {
  const manifest = JSON.parse(readFileSync(join(repo, 'benchmarks/certification/applications', app, 'manifest.json'), 'utf8'));
  const appDir = join(work, app, manifest.subdir ?? '');
  const out = join(repo, 'benchmarks/certification/results/independent', app, `browser-i${isolates}`);
  mkdirSync(out, { recursive: true });
  if (!existsSync(join(appDir, '.next/BUILD_ID'))) throw new Error(`${app} is not built (compare-app.mjs prepare --app ${app})`);
  summary[app] = {};
  for (const server of ['next', 'nextrs']) {
    const env = { PATH: process.env.PATH, HOME: process.env.HOME, NEXT_TELEMETRY_DISABLED: '1', ...(manifest.runEnv ?? {}) };
    let child;
    const logFile = createWriteStream(join(out, `${server}.log`));
    if (server === 'next') {
      child = spawn('node', [join(appDir, 'node_modules/next/dist/bin/next'), 'start', '-p', String(PORT), '-H', '127.0.0.1'], { cwd: appDir, env: { ...env, NODE_ENV: 'production' }, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    } else {
      const a = ['start', appDir, '--js-runtime', 'embedded-v8', '--port', String(PORT), '--isolates', String(isolates), ...(manifest.nextrsArgs ?? [])];
      for (const name of Object.keys(manifest.runEnv ?? {})) a.push('--js-env', name);
      child = spawn(nextrsBin, a, { cwd: appDir, env: { ...env, NO_COLOR: '1' }, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
    }
    child.stdout.pipe(logFile);
    child.stderr.pipe(logFile);
    try {
      await up(PORT, child, manifest.health ?? '/');
      const json = join(out, `${server}.json`);
      const r = spawnSync('pnpm', ['exec', 'playwright', 'test', '--config', config], {
        cwd: velasia,
        env: { ...process.env, NODE_PATH: join(velasia, 'node_modules'), BASE_URL: `http://127.0.0.1:${PORT}`, APP_SPEC: app, BROWSER_JSON: json },
        encoding: 'utf8',
      });
      writeFileSync(join(out, `${server}.txt`), `${r.stdout}\n${r.stderr}`);
      let passed = 0, failed = 0;
      try {
        const report = JSON.parse(readFileSync(json, 'utf8'));
        passed = report.stats?.expected ?? 0;
        failed = (report.stats?.unexpected ?? 0) + (report.stats?.flaky ?? 0);
      } catch {}
      summary[app][server] = { exit: r.status, passed, failed };
      log(`${app} on ${server}: ${passed} passed, ${failed} failed`);
    } catch (error) {
      summary[app][server] = { error: String(error.message ?? error) };
      log(`${app} on ${server}: ${error.message}`);
    } finally {
      await stop(child);
    }
  }
}
writeFileSync(join(repo, 'benchmarks/certification/results/independent', `browser-summary-i${isolates}.json`), JSON.stringify(summary, null, 2));
console.log(JSON.stringify(summary, null, 2));
