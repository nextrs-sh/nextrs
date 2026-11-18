#!/usr/bin/env node
// Does a faulted Velasia search ever render results instead of the error UI
// when an unfaulted search for the same date runs at the same time? Run
// against the official server and NextRS (same build, same API), to tell
// application caching from runtime behaviour (soak "unexpected" cases).
//
//   node benchmarks/phase3c/fault-cache-probe.mjs [--rounds 40]
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const argv = process.argv.slice(2);
const rounds = Number(argv.includes('--rounds') ? argv[argv.indexOf('--rounds') + 1] : 40);
const src = join(process.env.HOME, '.cache/nextrs-phase3b/velasia-src');
const manifest = JSON.parse(readFileSync(join(repo, 'benchmarks/certification/applications/velasia-airways/manifest.json'), 'utf8'));
const standalone = join(src, manifest.app.standaloneDir);
const WEB = 3870, API = 3880;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const env0 = { PATH: process.env.PATH, HOME: process.env.HOME, LANG: 'C.UTF-8' };
const webEnv = { ...manifest.web.env, SITE_URL: `http://127.0.0.1:${WEB}`, RETAILING_API_URL: `http://127.0.0.1:${API}` };
const TEXT = { malformed: 'Something went wrong loading flights', unavailable: 'load flights right now', timeout: 'taking too long' };
async function ready(port, path) { for (let i = 0; i < 240; i++) { try { if ((await fetch(`http://127.0.0.1:${port}${path}`)).ok) return; } catch {} await sleep(250); } throw new Error('not ready'); }
const db = mkdtempSync(join(tmpdir(), 'velasia-fcp-'));
const api = spawn(manifest.api.command[0], manifest.api.command.slice(1), { cwd: src, env: { ...env0, ...manifest.api.env, RETAILING_API_PORT: String(API), VELASIA_TODAY: manifest.today, TALADB_PATH: join(db, 'v.db'), LOG_LEVEL: 'error' }, stdio: 'ignore', detached: true });
const out = {};
try {
  await ready(API, '/health');
  for (const runtime of ['next', 'nextrs']) {
    const child = runtime === 'next'
      ? spawn('node', [join(standalone, 'server.js')], { cwd: src, env: { ...env0, NODE_ENV: 'production', ...webEnv, PORT: String(WEB), HOSTNAME: '127.0.0.1' }, stdio: 'ignore', detached: true })
      : spawn(join(repo, 'target/release/nextrs'), ['start', standalone, '--js-runtime', 'embedded-v8', '--port', String(WEB), '--isolates', '2', '--net-allow', `http://127.0.0.1:${API}`, '--net-allow-addr', '127.0.0.1', ...Object.keys(webEnv).flatMap((n) => ['--js-env', n])], { cwd: src, env: { ...env0, ...webEnv }, stdio: 'ignore', detached: true });
    try {
      await ready(WEB, '/api/internal/health/ready');
      const tally = { faulted: 0, missingErrorUi: 0, samples: [] };
      for (let i = 0; i < rounds; i++) {
        const fault = ['malformed', 'unavailable', 'timeout'][i % 3];
        const day = `2026-12-${String(5 + (i % 20)).padStart(2, '0')}`;
        const path = `/flights?trip=one-way&from=MNL&to=SIN&depart=${day}&adults=1&cabin=economy`;
        const [faulted] = await Promise.all([
          fetch(`http://127.0.0.1:${WEB}${path}`, { headers: { cookie: `velasia-fault=${fault}` } }).then((r) => r.text()),
          fetch(`http://127.0.0.1:${WEB}${path}`).then((r) => r.text()),
        ]);
        tally.faulted++;
        if (!faulted.includes(TEXT[fault])) {
          tally.missingErrorUi++;
          if (tally.samples.length < 3) tally.samples.push({ fault, day, hasResults: faulted.includes('NV5'), hasOtherError: Object.values(TEXT).filter((t) => faulted.includes(t)) });
        }
      }
      out[runtime] = tally;
      console.log(runtime, JSON.stringify(tally));
    } finally {
      try { process.kill(-child.pid, 'SIGTERM'); } catch {}
      await sleep(1500);
    }
  }
} finally {
  try { process.kill(-api.pid, 'SIGTERM'); } catch {}
  rmSync(db, { recursive: true, force: true });
}
