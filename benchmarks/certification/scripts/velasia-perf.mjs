#!/usr/bin/env node
// Velasia Airways workloads: the official standalone server vs NextRS
// (1 and 4 isolates) on the same build, the same API process and the same
// TalaDB state (Phase 3B.7).
//
//   velasia-perf.mjs --src <clone> [--rounds 3] [--duration 8] [--warmup 2]
//                    [--out benchmarks/certification/results/velasia-airways/perf]
//
// CPU layout (6 CPUs): web server 0-2, retailing API 3, load generator 4-5.
// One API process (fresh tmpfs TalaDB) serves every runtime; state for the
// authenticated workloads is created once through the UI
// (scenarios/perf-prepare.spec.ts). Each round runs the runtimes in a
// shuffled order; medians over rounds are reported. Every response is
// checked by the load generator (status, body markers); a run with failed
// checks is reported as such and must not be used.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadgen, median, memorySampler, metrics, rng, sample, shuffle, sleep, stopServer, waitReady } from '../../phase3a/lib.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../..');
const args = process.argv.slice(2);
const opt = (name, fallback) => (args.includes(`--${name}`) ? args[args.indexOf(`--${name}`) + 1] : fallback);
const src = resolve(opt('src'));
const rounds = Number(opt('rounds', 3));
const duration = Number(opt('duration', 8));
const warmup = Number(opt('warmup', 2));
const out = resolve(opt('out', join(repo, 'benchmarks/certification/results/velasia-airways/perf')));
const manifest = JSON.parse(readFileSync(join(here, '../applications/velasia-airways/manifest.json'), 'utf8'));
const nextrsBin = join(repo, 'target/release/nextrs');
const loadgenBin = join(repo, 'benchmarks/loadgen/target/release/nextrs-loadgen');
const standalone = join(src, manifest.app.standaloneDir);
const CPU = { web: '0-2', api: '3', load: '4-5' };
const API_PORT = 4510;
const WEB_PORT = 3500;
const METRICS_PORT = 4500;
const base = `http://127.0.0.1:${WEB_PORT}`;
const apiUrl = `http://127.0.0.1:${API_PORT}`;
mkdirSync(out, { recursive: true });
const log = (...a) => console.error('[perf]', ...a);

const webEnv = { ...manifest.web.env, SITE_URL: base, RETAILING_API_URL: apiUrl };
const cleanEnv = { PATH: process.env.PATH, HOME: process.env.HOME, LANG: 'C.UTF-8' };

function startApi(dbDir) {
  const child = spawn('taskset', ['-c', CPU.api, ...manifest.api.command], {
    cwd: src,
    env: { ...cleanEnv, ...manifest.api.env, RETAILING_API_PORT: String(API_PORT), VELASIA_TODAY: manifest.today, TALADB_PATH: join(dbDir, 'velasia.db'), LOG_LEVEL: 'error' },
    stdio: 'ignore',
    detached: true,
  });
  return child;
}

async function startWeb(runtime, isolates) {
  let cmd, cmdArgs, env;
  if (runtime === 'node') {
    cmd = 'node';
    cmdArgs = [join(standalone, 'server.js')];
    env = { ...cleanEnv, NODE_ENV: 'production', ...webEnv, PORT: String(WEB_PORT), HOSTNAME: '127.0.0.1' };
  } else {
    cmd = nextrsBin;
    cmdArgs = ['start', standalone, '--js-runtime', 'embedded-v8', '--port', String(WEB_PORT), '--isolates', String(isolates),
      '--net-allow', apiUrl, '--net-allow-addr', '127.0.0.1', '--metrics-addr', `127.0.0.1:${METRICS_PORT}`];
    for (const name of Object.keys(webEnv)) cmdArgs.push('--js-env', name);
    env = { ...cleanEnv, ...webEnv, RUST_LOG: 'warn', NO_COLOR: '1' };
  }
  const t0 = Date.now();
  const child = spawn('taskset', ['-c', CPU.web, cmd, ...cmdArgs], { cwd: src, env, stdio: ['ignore', 'ignore', 'pipe'], detached: true });
  let stderr = '';
  child.stderr.on('data', (c) => (stderr = (stderr + c).slice(-4000)));
  await waitReady(WEB_PORT, '/api/internal/health/ready', 60_000);
  return { child, pid: child.pid, port: WEB_PORT, startup_ms: Date.now() - t0, stderr: () => stderr };
}

function prepare() {
  const statePath = join(out, 'state.json');
  const config = join(here, '../applications/velasia-airways/scenarios/playwright.isolation.config.ts');
  const env = { ...process.env, VELASIA_SRC: src, NODE_PATH: join(src, 'node_modules'), E2E_WEB_PORT: String(WEB_PORT), PERF_STATE: statePath, ISOLATION_JSON: join(out, 'prepare.json') };
  delete env.CI;
  const r = spawnSync('pnpm', ['exec', 'playwright', 'test', '--config', config, 'perf-prepare.spec.ts'], { cwd: src, env, stdio: 'inherit' });
  if (r.status !== 0) throw new Error('prepare failed');
  return JSON.parse(readFileSync(statePath, 'utf8'));
}

const SEARCH = '/flights?trip=one-way&from=MNL&to=SIN&depart=2026-12-15&adults=1&cabin=economy';

function workloads(state) {
  const cookie = (c) => [['cookie', c]];
  const w = {
    home: { requests: [{ path: '/', expect_body: ['Beyond every horizon'] }] },
    destination: { requests: [{ path: '/destinations/manila', expect_body: ['Manila'] }] },
    'flight-search': { requests: [{ path: '/flights', expect_body: ['Velasia'] }] },
    'flight-results': { requests: [{ path: SEARCH, expect_body: ['Singapore'] }] },
    'fare-offers': { requests: [{ path: `${SEARCH}&fares=NV501-20261215`, expect_body: ['Velasia Classic'] }] },
    'api-route': { requests: [{ path: '/api/flights?from=MNL&to=SIN&departureDate=2026-12-15&adults=1', expect_body: ['NV501'] }] },
    'booking-ssr': { requests: [{ path: '/booking/travellers', headers: cookie(state.booking.cookies), expect_body: ['traveller'] }] },
    'account-trips': { requests: [{ path: '/account/trips', headers: cookie(state.account.cookies), expect_body: [state.account.reference] }] },
    'manage-booking': { requests: [{ path: `/manage-booking/${state.account.orderId}`, headers: cookie(state.account.cookies), expect_body: [state.account.reference] }] },
    'server-action': {
      requests: [{
        method: 'POST',
        path: state.action.path,
        headers: [...Object.entries(state.action.headers), ['cookie', state.action.cookies]],
        body: state.action.body,
        forbid_body: ['Invalid Server Actions request'],
      }],
    },
    'image-cached': { requests: [{ path: '/_next/image?url=%2Fimages%2Fdestinations%2Fmanila.jpg&w=640&q=70', headers: [['accept', 'image/webp']] }] },
  };
  const mix = [];
  for (const [name, weight] of [['home', 3], ['destination', 2], ['flight-results', 3], ['fare-offers', 2], ['api-route', 2], ['booking-ssr', 1], ['account-trips', 1], ['manage-booking', 1], ['image-cached', 3]]) {
    for (let i = 0; i < weight; i++) mix.push(...w[name].requests.map((r) => ({ ...r, kind: name })));
  }
  w.mixed = { requests: mix };
  for (const [name, spec] of Object.entries(w)) writeFileSync(join(out, `spec-${name}.json`), JSON.stringify(spec));
  return Object.keys(w);
}
const OPEN_LOOP = ['home', 'flight-results', 'fare-offers', 'booking-ssr', 'server-action', 'mixed'];

async function measure(server, name, rate) {
  const spec = join(out, `spec-${name}.json`);
  const connections = rate ? 64 : 16;
  // Warm up unsampled, then measure with no warm-up: the CPU and request
  // counts below cover exactly the same window.
  await loadgen(loadgenBin, { cpus: CPU.load, target: `127.0.0.1:${WEB_PORT}`, spec, connections, rate, duration: warmup, warmup: 0, threads: 2 });
  const before = sample(server.pid);
  const stopMem = memorySampler(server.pid, 200);
  const result = await loadgen(loadgenBin, { cpus: CPU.load, target: `127.0.0.1:${WEB_PORT}`, spec, connections, rate, duration, warmup: 0, threads: 2 });
  const mem = stopMem();
  const after = sample(server.pid);
  const cpu = after.cpu_user_s + after.cpu_sys_s - (before.cpu_user_s + before.cpu_sys_s);
  return {
    throughput: result.achieved_rps,
    latency: result.latency,
    service: result.service,
    errors: result.io_errors + result.timeouts,
    check_failures: result.status_mismatch + result.body_mismatch,
    check_failure_samples: result.check_failure_samples?.slice(0, 3),
    cpu_ms_per_req: result.completed ? (cpu * 1000) / result.completed : null,
    peak_rss_mib: mem.peak.rss_mib,
    peak_anon_mib: mem.peak.anon_mib,
    late_starts: result.late_starts,
    completed: result.completed,
  };
}

async function heap() {
  try {
    const text = await (await fetch(`${base}/api/internal/metrics`)).text();
    const used = /nodejs_heap_bytes\{service="web",kind="used"\} (\d+)/.exec(text)?.[1];
    return used ? Math.round(Number(used) / 1048576) : null;
  } catch {
    return null;
  }
}

const dbDir = mkdtempSync(join(tmpdir(), 'velasia-perf-db-'));
const api = startApi(dbDir);
try {
  await waitReady(API_PORT, '/health', 60_000);
  log('api ready');
  let server = await startWeb('node', 1);
  const state = existsSync(join(out, 'state.json')) && args.includes('--reuse-state') ? JSON.parse(readFileSync(join(out, 'state.json'), 'utf8')) : prepare();
  const names = workloads(state);
  // Offered rates for the open loop: half of next start's closed-loop throughput.
  const rates = {};
  for (const name of OPEN_LOOP) {
    const r = await loadgen(loadgenBin, { cpus: CPU.load, target: `127.0.0.1:${WEB_PORT}`, spec: join(out, `spec-${name}.json`), connections: 16, duration: 4, warmup: 1, threads: 2 });
    rates[name] = Math.max(5, Math.round(r.achieved_rps / 2 / 5) * 5);
  }
  log('open-loop rates', rates);
  await stopServer(server);
  const configs = [['node', 1], ['nextrs', 1], ['nextrs', 4]];
  const runs = [];
  const random = rng(3);
  for (let round = 1; round <= rounds; round++) {
    for (const [runtime, isolates] of shuffle([...configs], random)) {
      const label = runtime === 'node' ? 'next-standalone' : `nextrs-i${isolates}`;
      server = await startWeb(runtime, isolates);
      log(`round ${round} ${label}: startup ${server.startup_ms} ms`);
      const run = { round, label, startup_ms: server.startup_ms, idle: sample(server.pid), closed: {}, open: {} };
      for (const name of names) {
        run.closed[name] = await measure(server, name, 0);
        log(`  ${label} ${name}: ${run.closed[name].throughput?.toFixed(0)} req/s, p99 ${run.closed[name].latency?.p99_ms} ms, cpu ${run.closed[name].cpu_ms_per_req?.toFixed(2)} ms/req, checks failed ${run.closed[name].check_failures}, errors ${run.closed[name].errors}`);
      }
      for (const name of OPEN_LOOP) run.open[name] = await measure(server, name, rates[name]);
      run.heap_used_mib = await heap();
      run.executor = runtime === 'nextrs' ? await metrics(METRICS_PORT) : null;
      run.final = sample(server.pid);
      runs.push(run);
      await stopServer(server);
      writeFileSync(join(out, 'runs.json'), JSON.stringify({ rates, runs }, null, 2));
    }
  }
  // API alone, for the same calls the dynamic pages make (separates API latency).
  writeFileSync(join(out, 'spec-api-direct.json'), JSON.stringify({ requests: [{ path: '/v1/flights?from=MNL&to=SIN&departureDate=2026-12-15&adults=1' }] }));
  const apiDirect = await loadgen(loadgenBin, { cpus: CPU.load, target: `127.0.0.1:${API_PORT}`, spec: join(out, 'spec-api-direct.json'), connections: 16, duration, warmup, threads: 2 });
  writeFileSync(join(out, 'runs.json'), JSON.stringify({ rates, runs, apiDirect }, null, 2));
  log('done');
} finally {
  try { process.kill(-api.pid, 'SIGTERM'); } catch {}
  await sleep(1000);
  rmSync(dbDir, { recursive: true, force: true });
}
