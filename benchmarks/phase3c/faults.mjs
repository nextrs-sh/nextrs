#!/usr/bin/env node
// Phase 3C.4 end-to-end failure scenarios on the unchanged Velasia build,
// served by NextRS (the cases the Rust test suites cannot cover alone):
//
//   api-outage       the retailing API is killed under load, then restarted
//   sigterm-drain    SIGTERM with slow requests in flight
//   sigkill-cache    SIGKILL with a persistent --cache-dir, then a restart on it
//   cache-unwritable the cache directory becomes read-only while serving
//
//   node benchmarks/phase3c/faults.mjs [--src clone] [--nextrs bin] [--only a,b] [--out file.json]
import http from 'node:http';
import { spawn } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const argv = process.argv.slice(2);
const opt = (n, d) => (argv.includes(`--${n}`) ? argv[argv.indexOf(`--${n}`) + 1] : d);
const src = resolve(opt('src', join(process.env.HOME, '.cache/nextrs-phase3b/velasia-src')));
const nextrs = resolve(opt('nextrs', join(repo, 'target/release/nextrs')));
const only = opt('only', 'api-outage,sigterm-drain,sigkill-cache,cache-unwritable').split(',');
const manifest = JSON.parse(readFileSync(join(repo, 'benchmarks/certification/applications/velasia-airways/manifest.json'), 'utf8'));
const standalone = join(src, manifest.app.standaloneDir);
const WEB = 3850, METRICS = 3851, API = 3860;
const apiUrl = `http://127.0.0.1:${API}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const cleanEnv = { PATH: process.env.PATH, HOME: process.env.HOME, LANG: 'C.UTF-8' };
const webEnv = { ...manifest.web.env, SITE_URL: `http://127.0.0.1:${WEB}`, RETAILING_API_URL: apiUrl };
const SEARCH = '/flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-15&adults=1&cabin=economy';
const log = (...a) => console.error('[faults]', ...a);

function send({ port = WEB, path, headers = {}, method = 'GET', timeoutMs = 20000, newConnection = false }) {
  return new Promise((res) => {
    const t0 = Date.now();
    const req = http.request({ host: '127.0.0.1', port, path, method, headers, agent: newConnection ? false : undefined }, (r) => {
      const chunks = [];
      r.on('data', (c) => chunks.push(c));
      r.on('end', () => res({ status: r.statusCode, headers: r.headers, body: Buffer.concat(chunks).toString(), ms: Date.now() - t0 }));
      r.on('error', () => res({ status: 0, error: 'body', ms: Date.now() - t0 }));
    });
    req.setTimeout(timeoutMs, () => req.destroy(new Error('timeout')));
    req.on('error', (e) => res({ status: 0, error: e.code ?? e.message, ms: Date.now() - t0 }));
    req.end();
  });
}
async function ready(port, path, ms = 60000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    if ((await send({ port, path, timeoutMs: 2000 })).status === 200) return true;
    await sleep(200);
  }
  return false;
}
const metrics = async () => { try { return JSON.parse((await send({ port: METRICS, path: '/metrics.json' })).body); } catch { return null; } };

let dbDir;
function startApi() {
  return spawn(manifest.api.command[0], manifest.api.command.slice(1), { cwd: src, env: { ...cleanEnv, ...manifest.api.env, RETAILING_API_PORT: String(API), VELASIA_TODAY: manifest.today, TALADB_PATH: join(dbDir, 'v.db'), LOG_LEVEL: 'error' }, stdio: 'ignore', detached: true });
}
function startWeb(extra = []) {
  const args = ['start', standalone, '--js-runtime', 'embedded-v8', '--port', String(WEB), '--isolates', '2', '--net-allow', apiUrl, '--net-allow-addr', '127.0.0.1',
    '--metrics-addr', `127.0.0.1:${METRICS}`, '--strip-forwarded-headers', ...extra];
  for (const n of Object.keys(webEnv)) args.push('--js-env', n);
  const child = spawn(nextrs, args, { cwd: src, env: { ...cleanEnv, ...webEnv, RUST_LOG: 'nextrs=info', NO_COLOR: '1' }, stdio: ['ignore', 'ignore', 'pipe'], detached: true });
  child.log = '';
  child.stderr.on('data', (c) => (child.log += c));
  child.exited = new Promise((r) => child.on('exit', (code, signal) => r({ code, signal })));
  return child;
}
const kill = (child, sig = 'SIGTERM') => { try { process.kill(-child.pid, sig); } catch {} };

async function load(ms, paths, concurrency = 6, headers = {}) {
  const out = { statuses: {}, n: 0, samples: [] };
  const until = Date.now() + ms;
  await Promise.all(Array.from({ length: concurrency }, async (_, i) => {
    while (Date.now() < until) {
      const path = paths[(out.n + i) % paths.length];
      const r = await send({ path, headers });
      out.n++;
      const key = `${path.split('?')[0]} ${r.status}`;
      out.statuses[key] = (out.statuses[key] ?? 0) + 1;
      if (out.samples.length < 6 && r.status !== 200) out.samples.push({ path, status: r.status, error: r.error, nextrsError: r.headers?.['x-nextrs-error'], body: r.body?.slice(0, 120) });
    }
  }));
  return out;
}

const results = { nextrs, started: new Date().toISOString(), scenarios: {} };
dbDir = mkdtempSync(join(tmpdir(), 'velasia-faults-db-'));
let api = startApi();
try {
  if (!(await ready(API, '/health'))) throw new Error('api not ready');

  if (only.includes('api-outage')) {
    log('api-outage');
    const web = startWeb();
    await ready(WEB, '/api/internal/health/ready');
    const before = await load(5000, ['/', SEARCH]);
    kill(api, 'SIGKILL');
    await sleep(300);
    const during = await load(15000, ['/', SEARCH, '/destinations/manila', '/api/flights?from=MNL&to=SIN&departureDate=2026-12-15&adults=1']);
    const errorUi = await send({ path: SEARCH });
    api = startApi();
    await ready(API, '/health');
    const after = await load(5000, ['/', SEARCH]);
    const recovered = await send({ path: SEARCH });
    const m = await metrics();
    results.scenarios['api-outage'] = {
      before: before.statuses, during: during.statuses, duringSamples: during.samples,
      searchDuringOutage: { status: errorUi.status, showsErrorUi: /load flights right now|Something went wrong|taking too long/.test(errorUi.body), inventsFlights: errorUi.body.includes('NV501') },
      after: after.statuses, recovered: { status: recovered.status, showsFlights: recovered.body.includes('Singapore') && recovered.body.includes('NV5') },
      restarts: m?.executors?.reduce((a, e) => a + e.restarts, 0), readyIsolates: m?.executors?.filter((e) => e.state === 'ready').length,
    };
    kill(web);
    await web.exited;
  }

  if (only.includes('sigterm-drain')) {
    log('sigterm-drain');
    const web = startWeb(['--shutdown-grace', '10']);
    await ready(WEB, '/api/internal/health/ready');
    // Slow requests: the API delays each search by 2 s (fault cookie).
    const slow = Array.from({ length: 6 }, () => send({ path: SEARCH, headers: { cookie: 'velasia-fault=delay=2000' }, newConnection: true }));
    await sleep(500);
    const t0 = Date.now();
    process.kill(web.pid, 'SIGTERM');
    let readyz = null;
    for (let i = 0; i < 40 && readyz === null; i++) {
      const r = await send({ port: METRICS, path: '/readyz', timeoutMs: 500 });
      if (r.status !== 200) readyz = { status: r.status, afterMs: Date.now() - t0 };
      else await sleep(20);
    }
    await sleep(200);
    const late = await send({ path: '/', newConnection: true, timeoutMs: 3000 });
    const inflight = await Promise.all(slow);
    const exit = await Promise.race([web.exited, sleep(30000).then(() => null)]);
    results.scenarios['sigterm-drain'] = {
      readyzAfterSignal: readyz,
      newConnectionAfterSignal: { status: late.status, error: late.error },
      inFlight: inflight.map((r) => ({ status: r.status, ms: r.ms, complete: r.body?.includes('</html>') ?? false })),
      exitMs: Date.now() - t0, exit,
      log: web.log.split('\n').filter((l) => /shutting down|stopping|stopped|grace/.test(l)).map((l) => l.replace(/\x1b\[[0-9;]*m/g, '').slice(0, 160)),
    };
    if (!exit) kill(web, 'SIGKILL');
  }

  if (only.includes('sigkill-cache')) {
    log('sigkill-cache');
    const cacheDir = mkdtempSync(join(tmpdir(), 'nextrs-faults-cache-'));
    let web = startWeb(['--cache-dir', join(cacheDir, 'data')]);
    await ready(WEB, '/api/internal/health/ready');
    const pages = ['/', '/destinations/manila', '/destinations/tokyo', '/destinations/seoul', '/flights', SEARCH];
    const warm = await load(6000, pages);
    const m0 = await metrics();
    kill(web, 'SIGKILL');
    const killed = await web.exited;
    const files = existsSync(join(cacheDir, 'data')) ? readdirSync(join(cacheDir, 'data'), { recursive: true }).length : 0;
    web = startWeb(['--cache-dir', join(cacheDir, 'data')]);
    const restarted = await ready(WEB, '/api/internal/health/ready');
    const check = await Promise.all(pages.map((p) => send({ path: p })));
    const m1 = await metrics();
    results.scenarios['sigkill-cache'] = {
      warm: warm.statuses, killed, cacheEntriesBeforeKill: m0?.cache?.entries, filesAfterKill: files,
      restarted, afterRestart: check.map((r, i) => ({ path: pages[i], status: r.status, ok: r.status === 200 && r.body.includes('</html>') })),
      cacheAfterRestart: m1?.cache && { entries: m1.cache.entries, corrupt: m1.cache.corrupt, io_errors: m1.cache.io_errors, hits: m1.cache.hits },
    };
    kill(web);
    await web.exited;
    rmSync(cacheDir, { recursive: true, force: true });
  }

  if (only.includes('cache-unwritable')) {
    log('cache-unwritable');
    const cacheDir = mkdtempSync(join(tmpdir(), 'nextrs-faults-cache-'));
    const data = join(cacheDir, 'data');
    const web = startWeb(['--cache-dir', data]);
    await ready(WEB, '/api/internal/health/ready');
    await send({ path: '/' });
    const m0 = await metrics();
    // Make every directory under the cache read-only (as a full or failed
    // disk refuses writes), then visit pages whose entries are not stored yet.
    const dirs = [data, ...readdirSync(data, { recursive: true, withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => join(d.parentPath ?? d.path, d.name))];
    for (const d of dirs) chmodSync(d, 0o500);
    const fresh = ['/destinations/bangkok', '/destinations/osaka', '/destinations/taipei', '/destinations/cebu', '/about', '/help'];
    const served = await Promise.all(fresh.map((p) => send({ path: p })));
    await sleep(500);
    const again = await Promise.all(fresh.map((p) => send({ path: p })));
    const m1 = await metrics();
    for (const d of dirs) { try { chmodSync(d, 0o700); } catch {} }
    results.scenarios['cache-unwritable'] = {
      firstVisit: served.map((r, i) => ({ path: fresh[i], status: r.status })),
      secondVisit: again.map((r, i) => ({ path: fresh[i], status: r.status })),
      cacheErrors: { before: m0?.cache?.io_errors, after: m1?.cache?.io_errors },
      entries: { before: m0?.cache?.entries, after: m1?.cache?.entries },
      log: web.log.split('\n').filter((l) => /cache/i.test(l) && /WARN|ERROR/.test(l)).slice(0, 3).map((l) => l.replace(/\x1b\[[0-9;]*m/g, '').slice(0, 200)),
    };
    kill(web);
    await web.exited;
    rmSync(cacheDir, { recursive: true, force: true });
  }
} finally {
  kill(api, 'SIGTERM');
  await sleep(500);
  rmSync(dbDir, { recursive: true, force: true });
}
const out = resolve(opt('out', join(repo, 'benchmarks/results/phase3c/faults/faults.json')));
writeFileSync(out, JSON.stringify(results, null, 2));
console.log(JSON.stringify(results.scenarios, null, 2));
