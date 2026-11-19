#!/usr/bin/env node
// Phase 3D: during a retailing API outage, does a cold PPR destination page
// (/destinations/<slug>, never requested before the API went down) ever
// answer 500, on the official standalone server and on NextRS alike?
// Release regression saw 4 x 500 of 1,015 on NextRS (faults.mjs api-outage);
// Phase 3C saw 0 of 452.
//
//   node benchmarks/phase3d/outage-destination.mjs [--nextrs BIN] [--trials 4] [--isolates 2]
//     [--out benchmarks/results/phase3d/outage-destination]
import http from 'node:http';
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const argv = process.argv.slice(2);
const opt = (n, d) => (argv.includes(`--${n}`) ? argv[argv.indexOf(`--${n}`) + 1] : d);
const src = resolve(opt('src', join(process.env.HOME, '.cache/nextrs-phase3b/velasia-src')));
const nextrsBin = resolve(opt('nextrs', join(repo, 'target/release/nextrs')));
const trials = Number(opt('trials', 4));
const isolates = Number(opt('isolates', 2));
const out = resolve(opt('out', join(repo, 'benchmarks/results/phase3d/outage-destination')));
mkdirSync(out, { recursive: true });
const manifest = JSON.parse(readFileSync(join(repo, 'benchmarks/certification/applications/velasia-airways/manifest.json'), 'utf8'));
const standalone = join(src, manifest.app.standaloneDir);
const WEB = 3940, API = 3950, METRICS = 3941;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const env0 = { PATH: process.env.PATH, HOME: process.env.HOME, LANG: 'C.UTF-8' };
const webEnv = { ...manifest.web.env, SITE_URL: `http://127.0.0.1:${WEB}`, RETAILING_API_URL: `http://127.0.0.1:${API}` };
const SLUGS = ['manila', 'tokyo', 'seoul', 'osaka', 'cebu', 'bangkok'];

function get(port, path) {
  return new Promise((res) => {
    const req = http.get({ host: '127.0.0.1', port, path, agent: false }, (r) => {
      const c = []; r.on('data', (d) => c.push(d)); r.on('end', () => res({ status: r.statusCode, headers: r.headers, body: Buffer.concat(c).toString() }));
    });
    req.setTimeout(20000, () => req.destroy());
    req.on('error', (e) => res({ status: 0, error: e.code }));
  });
}
async function ready(port, path) { for (let i = 0; i < 240; i++) { if ((await get(port, path)).status === 200) return; await sleep(250); } throw new Error(`not ready ${port}`); }
const kill = (c, s = 'SIGTERM') => { try { process.kill(-c.pid, s); } catch {} };
function startApi(db) {
  return spawn(manifest.api.command[0], manifest.api.command.slice(1), { cwd: src, env: { ...env0, ...manifest.api.env, RETAILING_API_PORT: String(API), VELASIA_TODAY: manifest.today, TALADB_PATH: join(db, 'v.db'), LOG_LEVEL: 'error' }, stdio: 'ignore', detached: true });
}
function startWeb(runtime, logFile) {
  const fd = openSync(logFile, 'w');
  return runtime === 'next'
    ? spawn('node', [join(standalone, 'server.js')], { cwd: src, env: { ...env0, NODE_ENV: 'production', ...webEnv, PORT: String(WEB), HOSTNAME: '127.0.0.1' }, stdio: ['ignore', fd, fd], detached: true })
    : spawn(nextrsBin, ['start', standalone, '--js-runtime', 'embedded-v8', '--port', String(WEB), '--isolates', String(isolates), '--metrics-addr', `127.0.0.1:${METRICS}`, '--net-allow', `http://127.0.0.1:${API}`, '--net-allow-addr', '127.0.0.1', ...Object.keys(webEnv).flatMap((n) => ['--js-env', n])], { cwd: src, env: { ...env0, ...webEnv, NO_COLOR: '1' }, stdio: ['ignore', fd, fd], detached: true });
}

const summary = { nextrsBin, isolates, trials, runs: [] };
for (const runtime of ['next', 'nextrs']) {
  for (let t = 0; t < trials; t++) {
    const db = mkdtempSync(join(tmpdir(), 'velasia-od-'));
    const api = startApi(db);
    const logFile = join(out, `${runtime}-t${t}.log`);
    const web = startWeb(runtime, logFile);
    const run = { runtime, trial: t, statuses: {}, firstByPath: {}, samples: [] };
    try {
      await ready(API, '/health');
      await ready(WEB, '/api/internal/health/ready');
      // Warm what faults.mjs warms (home and a search), not the destinations.
      for (let i = 0; i < 20; i++) await get(WEB, '/');
      kill(api, 'SIGKILL');
      await sleep(300);
      // Cold destination pages under 6 concurrent clients for 10 s.
      const until = Date.now() + 10000; let n = 0;
      await Promise.all(Array.from({ length: 6 }, async (_, w) => {
        while (Date.now() < until) {
          const slug = SLUGS[(n++ + w) % SLUGS.length];
          const path = `/destinations/${slug}`;
          const r = await get(WEB, path);
          const k = `${r.status}`;
          run.statuses[k] = (run.statuses[k] ?? 0) + 1;
          run.firstByPath[slug] ??= r.status;
          if (r.status !== 200 && run.samples.length < 4) run.samples.push({ path, status: r.status, at: new Date().toISOString(), cache: r.headers?.['x-nextjs-cache'], nextrsError: r.headers?.['x-nextrs-error'], body: r.body?.slice(0, 4000) });
        }
      }));
    } catch (e) { run.error = String(e); }
    finally { kill(web); kill(api, 'SIGKILL'); await sleep(1200); rmSync(db, { recursive: true, force: true }); }
    summary.runs.push(run);
    console.log(runtime, t, JSON.stringify(run.statuses), JSON.stringify(run.firstByPath));
  }
}
writeFileSync(join(out, 'summary.json'), JSON.stringify(summary, null, 2));
