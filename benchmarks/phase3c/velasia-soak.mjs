#!/usr/bin/env node
// Phase 3C long-duration soak of the unchanged Velasia Airways application
// on NextRS (PPR shells, Cache Components, API-backed dynamic pages, signed-in
// pages, a Server Action, image transformation and cache hits, retailing API
// failures, client cancellation), sampling memory throughout.
//
//   node benchmarks/phase3c/velasia-soak.mjs --src <velasia clone> --out DIR
//     [--isolates 4] [--minutes 360] [--idle-minutes 10] [--concurrency 8]
//     [--nextrs target/release/nextrs] [--sample-every 30] [--storm-every 30]
//     [--storm-minutes 3] [--port 3700] [--seed 7]
//
// The Velasia checkout is never modified: the API runs from the clone with a
// fresh TalaDB file in the OS temporary directory, NextRS serves the
// standalone build, and state (a shopping session, a signed-in account with
// one confirmed order, one captured Server Action) is created through the
// UI by scenarios/perf-prepare.spec.ts.
//
// Every request is counted exactly once: ok; degraded as expected (an API
// fault the request asked for, answered with Velasia's error UI); client
// abort (deliberate); overload (503 from NextRS); client timeout; unexpected.
// Identity checks: the signed-in pages must show the account's booking
// reference, and anonymous requests for the same pages must never contain
// it. Traffic is drawn from a seeded generator (deterministic sequence per
// worker; timing still varies).
import http from 'node:http';
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const argv = process.argv.slice(2);
const opt = (name, fallback) => (argv.includes(`--${name}`) ? argv[argv.indexOf(`--${name}`) + 1] : fallback);
const src = resolve(opt('src', join(process.env.HOME, '.cache/nextrs-phase3b/velasia-src')));
const out = resolve(opt('out', join(repo, 'benchmarks/results/phase3c/soak-velasia')));
const isolates = Number(opt('isolates', 4));
const minutes = Number(opt('minutes', 360));
const idleMinutes = Number(opt('idle-minutes', 10));
const concurrency = Number(opt('concurrency', 8));
const sampleEvery = Number(opt('sample-every', 30));
const stormEvery = Number(opt('storm-every', 30));
const stormMinutes = Number(opt('storm-minutes', 3));
const nextrsBin = resolve(opt('nextrs', join(repo, 'target/release/nextrs')));
const WEB_PORT = Number(opt('port', 3700));
const API_PORT = WEB_PORT + 10;
const METRICS_PORT = WEB_PORT + 1;
const seed = Number(opt('seed', 7));
mkdirSync(out, { recursive: true });
const manifest = JSON.parse(readFileSync(join(repo, 'benchmarks/certification/applications/velasia-airways/manifest.json'), 'utf8'));
const standalone = join(src, manifest.app.standaloneDir);
const base = `http://127.0.0.1:${WEB_PORT}`;
const apiUrl = `http://127.0.0.1:${API_PORT}`;
const log = (...a) => console.error(`[velasia-soak ${new Date().toISOString()}]`, ...a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const cleanEnv = { PATH: process.env.PATH, HOME: process.env.HOME, LANG: 'C.UTF-8' };
const webEnv = { ...manifest.web.env, SITE_URL: base, RETAILING_API_URL: apiUrl };

function rng(s) {
  return () => {
    s |= 0; s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

async function waitReady(port, path, ms) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    const r = await send({ port, path, timeoutMs: 2000 });
    if (r.status === 200) return;
    await sleep(250);
  }
  throw new Error(`port ${port}${path} not ready`);
}

const agent = new http.Agent({ keepAlive: true, maxSockets: concurrency * 2 });
function send({ port = WEB_PORT, method = 'GET', path, headers = {}, body, abortAfterMs, timeoutMs = 30000 }) {
  return new Promise((resolveP) => {
    const t0 = process.hrtime.bigint();
    const ms = () => Number(process.hrtime.bigint() - t0) / 1e6;
    let done = false;
    const finish = (r) => { if (!done) { done = true; resolveP({ ms: ms(), ...r }); } };
    const req = http.request({ host: '127.0.0.1', port, method, path, headers, agent: abortAfterMs ? false : agent }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => finish({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      res.on('error', () => finish({ status: 0, error: 'body' }));
      res.on('aborted', () => finish({ status: 0, error: 'aborted' }));
    });
    req.setTimeout(timeoutMs, () => { req.destroy(new Error('timeout')); finish({ status: 0, error: 'timeout' }); });
    req.on('error', (e) => finish({ status: 0, error: e.code ?? e.message }));
    if (abortAfterMs) setTimeout(() => { req.destroy(new Error('aborted by client')); finish({ status: 0, error: 'client-abort' }); }, abortAfterMs);
    req.end(body);
  });
}

function procMem(pid) {
  try {
    const st = readFileSync(`/proc/${pid}/status`, 'utf8');
    const kb = (k) => Number(new RegExp(`${k}:\\s+(\\d+)`).exec(st)?.[1] ?? 0) / 1024;
    const f = readFileSync(`/proc/${pid}/stat`, 'utf8').split(') ')[1].split(' ');
    return { rssMiB: kb('VmRSS'), anonMiB: kb('RssAnon'), fileMiB: kb('RssFile'), swapMiB: kb('VmSwap'), hwmMiB: kb('VmHWM'), threads: kb('Threads') * 1024, cpuSeconds: (Number(f[11]) + Number(f[12])) / 100 };
  } catch {
    return null;
  }
}

// --- processes ---------------------------------------------------------
const dbDir = mkdtempSync(join(tmpdir(), 'velasia-soak-db-'));
const api = spawn(manifest.api.command[0], manifest.api.command.slice(1), {
  cwd: src,
  env: { ...cleanEnv, ...manifest.api.env, RETAILING_API_PORT: String(API_PORT), VELASIA_TODAY: manifest.today, TALADB_PATH: join(dbDir, 'velasia.db'), LOG_LEVEL: 'error' },
  stdio: ['ignore', 'ignore', 'pipe'], detached: true,
});
let apiErr = '';
api.stderr.on('data', (c) => (apiErr = (apiErr + c).slice(-4000)));
const webArgs = ['start', standalone, '--js-runtime', 'embedded-v8', '--port', String(WEB_PORT), '--isolates', String(isolates),
  '--net-allow', apiUrl, '--net-allow-addr', '127.0.0.1', '--metrics-addr', `127.0.0.1:${METRICS_PORT}`, '--strip-forwarded-headers'];
for (const name of Object.keys(webEnv)) webArgs.push('--js-env', name);
const { writeSync, openSync } = await import('node:fs');
const webLog = openSync(join(out, `nextrs-i${isolates}.log`), 'w');
const web = spawn(nextrsBin, webArgs, { cwd: src, env: { ...cleanEnv, ...webEnv, RUST_LOG: 'nextrs=info', NO_COLOR: '1' }, stdio: ['ignore', webLog, webLog], detached: true });
const stopAll = () => {
  try { process.kill(-web.pid, 'SIGTERM'); } catch {}
  try { process.kill(-api.pid, 'SIGTERM'); } catch {}
};
process.on('SIGINT', () => { stopAll(); process.exit(130); });

const result = { started: new Date().toISOString(), config: { isolates, minutes, idleMinutes, concurrency, stormEvery, stormMinutes, seed, nextrsBin, velasiaCommit: manifest.commit }, phases: [], samples: [], totals: {}, kinds: {}, errors: [] };
const save = () => writeFileSync(join(out, `velasia-soak-i${isolates}.json`), JSON.stringify(result, null, 1));

try {
  await waitReady(API_PORT, '/health', 60000);
  await waitReady(WEB_PORT, '/api/internal/health/ready', 60000);
  await waitReady(METRICS_PORT, '/readyz', 10000);
  log('servers ready; preparing state through the UI');
  const statePath = join(out, 'state.json');
  {
    const config = join(repo, 'benchmarks/certification/applications/velasia-airways/scenarios/playwright.isolation.config.ts');
    const env = { ...process.env, VELASIA_SRC: src, NODE_PATH: join(src, 'node_modules'), E2E_WEB_PORT: String(WEB_PORT), PERF_STATE: statePath, ISOLATION_JSON: join(out, 'prepare.json') };
    delete env.CI;
    const r = spawnSync('pnpm', ['exec', 'playwright', 'test', '--config', config, 'perf-prepare.spec.ts'], { cwd: src, env, stdio: ['ignore', 'pipe', 'pipe'] });
    if (r.status !== 0) throw new Error(`prepare failed: ${r.stdout}\n${r.stderr}`);
  }
  const state = JSON.parse(readFileSync(statePath, 'utf8'));
  const ref = state.account.reference;

  // --- traffic ---------------------------------------------------------
  const DEST = ['bangkok', 'cebu', 'davao', 'ho-chi-minh-city', 'hong-kong', 'kuala-lumpur', 'manila', 'osaka', 'seoul', 'singapore', 'taipei', 'tokyo'];
  const WIDTHS = [128, 256, 384, 640, 828, 1080, 1280, 1600, 1920];
  const day = (r) => `2026-12-${String(5 + Math.floor(r() * 20)).padStart(2, '0')}`;
  const search = (r) => `/flights?trip=one-way&from=MNL&to=SIN&depart=${day(r)}&adults=1&cabin=economy`;
  const faultText = { unavailable: 'load flights right now', timeout: 'taking too long', malformed: 'Something went wrong loading flights' };
  const kinds = {
    'ppr-home': { w: 10, make: () => ({ path: '/' }), expect: 'Beyond every horizon' },
    'ppr-destination': { w: 8, make: (r) => ({ path: `/destinations/${DEST[Math.floor(r() * DEST.length)]}` }), expect: '</html>' },
    'rsc-destination': { w: 3, make: (r) => ({ path: `/destinations/${DEST[Math.floor(r() * DEST.length)]}`, headers: { rsc: '1' } }) },
    'static-page': { w: 3, make: (r) => ({ path: ['/about', '/help', '/credits', '/destinations', '/robots.txt', '/sitemap.xml'][Math.floor(r() * 6)] }) },
    'flight-search-shell': { w: 4, make: () => ({ path: '/flights' }), expect: 'Velasia' },
    'flight-results': { w: 10, make: (r) => ({ path: search(r) }), expect: 'Singapore' },
    'fare-offers': { w: 4, make: (r) => ({ path: `${search(r)}&fares=NV501-2026121${Math.floor(r() * 5) + 1}` }), ok: [200] },
    'api-route': { w: 6, make: (r) => ({ path: `/api/flights?from=MNL&to=SIN&departureDate=${day(r)}&adults=1` }), expect: 'NV' },
    'booking-ssr': { w: 3, make: () => ({ path: '/booking/travellers', headers: { cookie: state.booking.cookies } }), expect: 'traveller' },
    'account-trips': { w: 3, make: () => ({ path: '/account/trips', headers: { cookie: state.account.cookies } }), expect: ref, identity: true },
    'account-anon': { w: 2, make: () => ({ path: '/account/trips' }), ok: [200, 303, 307, 308], forbid: ref },
    'manage-booking': { w: 2, make: () => ({ path: `/manage-booking/${state.account.orderId}`, headers: { cookie: state.account.cookies } }), expect: ref, identity: true },
    'manage-anon': { w: 1, make: () => ({ path: `/manage-booking/${state.account.orderId}` }), ok: [200, 303, 307, 308, 404], forbid: ref },
    'server-action': { w: 4, make: () => ({ method: 'POST', path: state.action.path, headers: { ...state.action.headers, cookie: state.action.cookies }, body: state.action.body }), forbid: 'Invalid Server Actions request' },
    'image-hit': { w: 8, make: (r) => ({ path: `/_next/image?url=%2Fimages%2Fdestinations%2F${DEST[Math.floor(r() * 4)]}.jpg&w=640&q=70`, headers: { accept: 'image/webp' } }) },
    'image-transform': { w: 4, make: (r) => ({ path: `/_next/image?url=%2Fimages%2Fdestinations%2F${DEST[Math.floor(r() * DEST.length)]}.jpg&w=${WIDTHS[Math.floor(r() * WIDTHS.length)]}&q=${r() < 0.5 ? 70 : 80}`, headers: { accept: r() < 0.7 ? 'image/webp' : '*/*' } }), ok: [200, 503] },
    'not-found': { w: 1, make: () => ({ path: '/no-such-page' }), ok: [404] },
    'api-fault': { w: 2, make: (r) => { const f = ['unavailable', 'malformed', 'timeout'][Math.floor(r() * 3)]; return { path: search(r), headers: { cookie: `velasia-fault=${f}` }, fault: f }; } },
    'abort-results': { w: 2, make: (r) => ({ path: search(r), abortAfterMs: 2 + Math.floor(r() * 40) }), abort: true },
    'abort-home': { w: 1, make: (r) => ({ path: '/', abortAfterMs: 1 + Math.floor(r() * 5) }), abort: true },
  };
  const table = Object.entries(kinds).flatMap(([name, k]) => Array(k.w).fill(name));
  const totals = { requests: 0, ok: 0, degradedExpected: 0, aborted: 0, overload503: 0, clientTimeouts: 0, unexpected: 0, identityLeaks: 0, wrongContent: 0, cancelledByServer: 0 };
  result.totals = totals;
  const stats = Object.fromEntries(Object.keys(kinds).map((k) => [k, { count: 0, statuses: {}, msTotal: 0, msMax: 0, slow: 0 }]));
  result.kinds = stats;
  const note = (e) => result.errors.length < 200 && result.errors.push({ at: new Date().toISOString(), ...e });
  const started = Date.now();
  const storm = () => {
    const m = (Date.now() - started) / 60000;
    return stormEvery > 0 && m > 5 && m % stormEvery < stormMinutes;
  };

  async function worker(id, deadline) {
    const r = rng(seed * 1000 + id);
    while (Date.now() < deadline) {
      let name = table[Math.floor(r() * table.length)];
      // Fault storms: a third of the dynamic searches carry an API fault.
      if (storm() && name === 'flight-results' && r() < 0.33) name = 'api-fault';
      const k = kinds[name];
      const req = k.make(r);
      const res = await send(req);
      const s = stats[name];
      s.count++;
      totals.requests++;
      s.statuses[res.status] = (s.statuses[res.status] ?? 0) + 1;
      s.msTotal += res.ms;
      if (res.ms > s.msMax) s.msMax = res.ms;
      if (res.ms > 5000) s.slow++;
      if (k.abort) {
        if (res.status === 0) { totals.aborted++; continue; }
        // Finished before the abort fired: judged as a normal request.
      }
      if (res.status === 0) {
        if (res.error === 'timeout') totals.clientTimeouts++;
        else totals.unexpected++;
        note({ kind: name, path: req.path, error: res.error });
        continue;
      }
      if (res.status === 503 && res.headers['x-nextrs-error']) { totals.overload503++; continue; }
      const body = res.body.toString('utf8');
      if (req.fault) {
        if (res.status === 200 && body.includes(faultText[req.fault])) { totals.degradedExpected++; continue; }
        totals.unexpected++;
        note({ kind: name, fault: req.fault, status: res.status, body: body.slice(0, 200) });
        continue;
      }
      const okStatuses = k.ok ?? [200];
      if (!okStatuses.includes(res.status)) {
        totals.unexpected++;
        note({ kind: name, path: req.path, status: res.status, nextrsError: res.headers['x-nextrs-error'], body: body.slice(0, 200) });
        continue;
      }
      if (k.forbid && body.includes(k.forbid)) {
        if (k.forbid === ref) totals.identityLeaks++; else totals.wrongContent++;
        note({ kind: name, forbidden: true, status: res.status });
        continue;
      }
      if (res.status === 200 && k.expect && !body.includes(k.expect)) {
        if (k.identity) totals.identityLeaks++; else totals.wrongContent++;
        note({ kind: name, missing: k.expect.slice(0, 40), status: res.status, body: body.slice(0, 200) });
        continue;
      }
      totals.ok++;
    }
  }

  const metricsJson = async () => {
    const r = await send({ port: METRICS_PORT, path: '/metrics.json', timeoutMs: 5000 });
    try { return JSON.parse(r.body.toString()); } catch { return null; }
  };
  async function sampleOnce(phase) {
    const m = await metricsJson();
    const execs = m?.executors ?? [];
    result.samples.push({
      t: Math.round((Date.now() - started) / 1000), phase, storm: storm(),
      proc: procMem(web.pid), api: procMem(api.pid),
      heapUsedMiB: execs.reduce((a, e) => a + (e.heap_used_bytes ?? 0), 0) / 1048576,
      heapTotalMiB: execs.reduce((a, e) => a + (e.heap_total_bytes ?? 0), 0) / 1048576,
      heapAfterGcMiB: execs.reduce((a, e) => a + (e.heap_used_after_gc_bytes ?? 0), 0) / 1048576,
      externalMiB: execs.reduce((a, e) => a + (e.external_bytes ?? 0), 0) / 1048576,
      malloc: m?.memory?.malloc ?? null,
      cacheMemoryMiB: (m?.cache?.memory_bytes ?? 0) / 1048576, cacheEntries: m?.cache?.entries ?? null,
      imageReservedMiB: (m?.images?.memory_reserved_bytes ?? 0) / 1048576, imageCacheMiB: (m?.images?.cache_memory_bytes ?? 0) / 1048576,
      imageQueued: m?.images?.queued ?? null, imageInFlight: m?.images?.in_flight ?? null,
      active: execs.reduce((a, e) => a + (e.active ?? 0), 0), queued: m?.queued ?? null,
      readyIsolates: execs.filter((e) => e.state === 'ready').length, restarts: execs.reduce((a, e) => a + (e.restarts ?? 0), 0),
      fullGcs: execs.reduce((a, e) => a + (e.full_gcs ?? 0), 0),
      executor: m ? { rejectedOverloaded: m.rejected_overloaded, rejectedQueueTimeout: m.rejected_queue_timeout, rejectedUnavailable: m.rejected_unavailable, requestTimeouts: m.request_timeouts, responsesCancelled: m.responses_cancelled, handlerErrors: m.handler_errors, completed: m.requests_completed } : null,
      http: m?.http ? { inFlight: m.http.in_flight, connectionsOpen: m.http.connections_open, requests: m.http.requests, bodies: m.http.bodies } : null,
      openFds: m?.process?.open_fds ?? null,
      totals: { ...totals },
    });
    save();
  }

  const deadline = started + minutes * 60000;
  result.phases.push({ name: 'load', from: 0 });
  const sampler = (async () => { while (Date.now() < deadline) { await sampleOnce('load'); await sleep(sampleEvery * 1000); } })();
  await Promise.all([...Array.from({ length: concurrency }, (_, i) => worker(i, deadline)), sampler]);
  result.phases.push({ name: 'idle', from: Math.round((Date.now() - started) / 1000) });
  log('load done; idle sampling', totals);
  const idleUntil = Date.now() + idleMinutes * 60000;
  while (Date.now() < idleUntil) { await sampleOnce('idle'); await sleep(sampleEvery * 1000); }
  // Graceful shutdown: readiness must turn false, then the process exits.
  const t0 = Date.now();
  process.kill(web.pid, 'SIGTERM');
  let readyDuringDrain = null;
  for (let i = 0; i < 40; i++) {
    const r = await send({ port: METRICS_PORT, path: '/readyz', timeoutMs: 1000 });
    if (r.status !== 200) { readyDuringDrain = r.status; break; }
    await sleep(25);
  }
  while (existsSync(`/proc/${web.pid}`) && !readFileSync(`/proc/${web.pid}/stat`, 'utf8').includes(') Z ') && Date.now() - t0 < 30000) await sleep(50);
  result.shutdown = { ms: Date.now() - t0, readyzDuringDrain: readyDuringDrain, exited: !existsSync(`/proc/${web.pid}`) || readFileSync(`/proc/${web.pid}/stat`, 'utf8').includes(') Z ') };
  result.finished = new Date().toISOString();
  save();
  log('done', JSON.stringify(totals), JSON.stringify(result.shutdown));
} catch (error) {
  result.fatal = String(error?.stack ?? error);
  save();
  log('FAILED', error, apiErr);
  process.exitCode = 1;
} finally {
  stopAll();
  await sleep(500);
  rmSync(dbDir, { recursive: true, force: true });
}
