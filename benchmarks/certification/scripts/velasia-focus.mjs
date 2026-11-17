#!/usr/bin/env node
// Focused Velasia measurements (Phase 3C): a few workloads, several server
// configurations, alternated round by round on the same API process and
// TalaDB state, with optional CPU sampling. Used for the PPR-shell
// investigation and its before/after comparisons; velasia-perf.mjs remains
// the full certification matrix.
//
//   velasia-focus.mjs --src <clone> --configs node,nextrs:1,nextrs:4
//       [--workloads home,destination] [--rounds 5] [--duration 8] [--warmup 3]
//       [--connections 16] [--nextrs-bin target/release/nextrs]
//       [--alt-bin target/other/nextrs]   (config "alt:1" runs this binary)
//       [--perf DIR] [--perf-seconds 10]  (perf record per config/workload)
//       [--out DIR] [--extra-arg ARG]...
//
// Only workloads that need no prepared state are offered here (home,
// destination, flight-search, api-route, image-cached).
//
// CPU layout as velasia-perf.mjs: web 0-2, API 3, load generator 4-5.
import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadgen, median, metrics, sample, sleep, stopServer, waitReady } from '../../phase3a/lib.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../..');
const argv = process.argv.slice(2);
const opt = (name, fallback) => (argv.includes(`--${name}`) ? argv[argv.indexOf(`--${name}`) + 1] : fallback);
const many = (name) => argv.flatMap((a, i) => (a === `--${name}` ? [argv[i + 1]] : []));
const src = resolve(opt('src', join(process.env.HOME, '.cache/nextrs-phase3b/velasia-src')));
const configs = opt('configs', 'node,nextrs:1').split(',');
const names = opt('workloads', 'home,destination').split(',');
const rounds = Number(opt('rounds', 5));
const duration = Number(opt('duration', 8));
const warmup = Number(opt('warmup', 3));
const connections = Number(opt('connections', 16));
const rate = Number(opt('rate', 0));
const perfDir = opt('perf', null);
const perfSeconds = Number(opt('perf-seconds', 10));
const out = resolve(opt('out', join(repo, 'benchmarks/results/phase3c/focus')));
const bins = { nextrs: resolve(opt('nextrs-bin', join(repo, 'target/release/nextrs'))), alt: opt('alt-bin', null) && resolve(opt('alt-bin')) };
const extraArgs = many('extra-arg');
const manifest = JSON.parse(readFileSync(join(here, '../applications/velasia-airways/manifest.json'), 'utf8'));
const loadgenBin = join(repo, 'benchmarks/loadgen/target/release/nextrs-loadgen');
const standalone = join(src, manifest.app.standaloneDir);
const CPU = { web: '0-2', api: '3', load: '4-5' };
const API_PORT = 4610, WEB_PORT = 3600, METRICS_PORT = 4600;
const base = `http://127.0.0.1:${WEB_PORT}`;
const apiUrl = `http://127.0.0.1:${API_PORT}`;
mkdirSync(out, { recursive: true });
if (perfDir) mkdirSync(perfDir, { recursive: true });
const log = (...a) => console.error('[focus]', ...a);
const webEnv = { ...manifest.web.env, SITE_URL: base, RETAILING_API_URL: apiUrl };
const cleanEnv = { PATH: process.env.PATH, HOME: process.env.HOME, LANG: 'C.UTF-8' };

const SPECS = {
  home: { requests: [{ path: '/', expect_body: ['Beyond every horizon'] }] },
  destination: { requests: [{ path: '/destinations/manila', expect_body: ['Manila'] }] },
  'flight-search': { requests: [{ path: '/flights', expect_body: ['Velasia'] }] },
  'api-route': { requests: [{ path: '/api/flights?from=MNL&to=SIN&departureDate=2026-12-15&adults=1', expect_body: ['NV501'] }] },
  'image-cached': { requests: [{ path: '/_next/image?url=%2Fimages%2Fdestinations%2Fmanila.jpg&w=640&q=70', headers: [['accept', 'image/webp']] }] },
  'rsc-home': { requests: [{ path: '/', headers: [['rsc', '1']] }] },
};
for (const n of names) {
  if (!SPECS[n]) throw new Error(`unknown workload ${n}`);
  writeFileSync(join(out, `spec-${n}.json`), JSON.stringify(SPECS[n]));
}

async function startWeb(config) {
  const [runtime, iso] = config.split(':');
  const isolates = Number(iso ?? 1);
  let cmd, args, env;
  if (runtime === 'node') {
    cmd = 'node';
    args = [join(standalone, 'server.js')];
    env = { ...cleanEnv, NODE_ENV: 'production', ...webEnv, PORT: String(WEB_PORT), HOSTNAME: '127.0.0.1', ...(perfDir ? { NODE_OPTIONS: '--perf-basic-prof' } : {}) };
  } else {
    cmd = bins[runtime];
    if (!cmd) throw new Error(`no binary for ${runtime}`);
    args = ['start', standalone, '--js-runtime', 'embedded-v8', '--port', String(WEB_PORT), '--isolates', String(isolates),
      '--net-allow', apiUrl, '--net-allow-addr', '127.0.0.1', '--metrics-addr', `127.0.0.1:${METRICS_PORT}`, ...extraArgs];
    if (perfDir) args.push('--v8-flags=--perf-basic-prof --no-logfile-per-isolate --logfile=/dev/null');
    for (const name of Object.keys(webEnv)) args.push('--js-env', name);
    env = { ...cleanEnv, ...webEnv, RUST_LOG: 'warn', NO_COLOR: '1' };
  }
  const t0 = Date.now();
  const child = spawn('taskset', ['-c', CPU.web, cmd, ...args], { cwd: src, env, stdio: ['ignore', 'ignore', 'pipe'], detached: true });
  let stderr = '';
  child.stderr.on('data', (c) => (stderr = (stderr + c).slice(-4000)));
  try {
    await waitReady(WEB_PORT, '/api/internal/health/ready', 60_000);
  } catch (e) {
    throw new Error(`${config} did not start: ${e.message}\n${stderr}`);
  }
  return { child, pid: child.pid, port: WEB_PORT, startup_ms: Date.now() - t0, runtime, stderr: () => stderr };
}

const lg = (name, o) => loadgen(loadgenBin, { cpus: CPU.load, target: `127.0.0.1:${WEB_PORT}`, spec: join(out, `spec-${name}.json`), connections, rate, threads: 2, warmup: 0, ...o });

async function measure(server, name) {
  await lg(name, { duration: warmup });
  const m0 = server.runtime !== 'node' ? await metrics(METRICS_PORT) : null;
  const before = sample(server.pid);
  const r = await lg(name, { duration });
  const after = sample(server.pid);
  const m1 = server.runtime !== 'node' ? await metrics(METRICS_PORT) : null;
  const cpu = after.cpu_user_s + after.cpu_sys_s - (before.cpu_user_s + before.cpu_sys_s);
  return {
    rps: r.achieved_rps,
    cpu_ms_per_req: r.completed ? (cpu * 1000) / r.completed : null,
    p50: r.latency?.p50_ms, p95: r.latency?.p95_ms, p99: r.latency?.p99_ms,
    ttfb_p50: r.ttfb?.p50_ms ?? null,
    completed: r.completed,
    check_failures: r.status_mismatch + r.body_mismatch,
    errors: r.io_errors + r.timeouts,
    anon_mib: after.anon_mib,
    metrics_delta: m0 && m1 ? delta(m0, m1, r.completed) : null,
  };
}

// Per-request deltas of the counters that matter for the PPR path.
function delta(a, b, n) {
  const flat = (m) => {
    const o = {};
    const walk = (x, p) => {
      if (typeof x === 'number') o[p] = x;
      else if (x && typeof x === 'object') for (const [k, v] of Object.entries(x)) walk(v, p ? `${p}.${k}` : k);
    };
    walk(m, '');
    return o;
  };
  const fa = flat(a), fb = flat(b), d = {};
  for (const [k, v] of Object.entries(fb)) {
    if (!/total|events|hits|misses|sets|waits|seconds/.test(k)) continue;
    const x = v - (fa[k] ?? 0);
    if (x) d[k] = +(x / n).toFixed(4);
  }
  return d;
}

async function profile(server, config, name) {
  const tag = `${config.replace(':', '-i')}.${name}`;
  await lg(name, { duration: warmup });
  const data = join(perfDir, `${tag}.perf.data`);
  const pids = execFileSync('pgrep', ['-g', String(server.pid)], { encoding: 'utf8' }).trim().split('\n').join(',');
  const perf = spawn('perf', ['record', '-e', 'cycles:u', '-F', '999', '-g', '--user-callchains', '-p', pids, '-o', data], { stdio: ['ignore', 'ignore', 'pipe'] });
  const closed = new Promise((r) => perf.on('close', r));
  await sleep(500);
  const r = await lg(name, { duration: perfSeconds });
  perf.kill('SIGINT');
  await closed;
  const run = (a) => execFileSync('perf', a, { maxBuffer: 1 << 30, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  writeFileSync(join(perfDir, `${tag}.by-dso.txt`), run(['report', '-i', data, '--no-children', '--sort', 'dso', '--stdio', '-g', 'none', '--percent-limit', '0.5']));
  writeFileSync(join(perfDir, `${tag}.by-symbol.txt`), run(['report', '-i', data, '--no-children', '--sort', 'dso,sym', '--stdio', '-g', 'none', '--percent-limit', '0.25']));
  writeFileSync(join(perfDir, `${tag}.children.txt`), run(['report', '-i', data, '--children', '--sort', 'sym', '--stdio', '-g', 'none', '--percent-limit', '0.75']));
  log(`perf ${tag}: ${Math.round(r.achieved_rps)} rps -> ${data}`);
}

const dbDir = mkdtempSync(join(tmpdir(), 'velasia-focus-db-'));
const api = spawn('taskset', ['-c', CPU.api, ...manifest.api.command], {
  cwd: src,
  env: { ...cleanEnv, ...manifest.api.env, RETAILING_API_PORT: String(API_PORT), VELASIA_TODAY: manifest.today, TALADB_PATH: join(dbDir, 'velasia.db'), LOG_LEVEL: 'error' },
  stdio: 'ignore', detached: true,
});
const results = { started: new Date().toISOString(), configs, names, rounds, duration, warmup, connections, rate, extraArgs, bins, runs: [] };
try {
  await waitReady(API_PORT, '/health', 60_000);
  for (let round = 1; round <= rounds; round++) {
    const order = round % 2 ? configs : [...configs].reverse();
    for (const config of order) {
      const server = await startWeb(config);
      try {
        for (const name of names) {
          const m = await measure(server, name);
          results.runs.push({ round, config, name, startup_ms: server.startup_ms, ...m });
          log(`r${round} ${config} ${name}: ${m.rps.toFixed(0)} rps, cpu ${m.cpu_ms_per_req?.toFixed(2)} ms/req, p50 ${m.p50} p99 ${m.p99}, fail ${m.check_failures}/${m.errors}`);
        }
        if (perfDir && round === 1) for (const name of names) await profile(server, config, name);
      } finally {
        await stopServer(server);
      }
      writeFileSync(join(out, 'runs.json'), JSON.stringify(results, null, 2));
    }
  }
  const summary = {};
  for (const config of configs) for (const name of names) {
    const rs = results.runs.filter((r) => r.config === config && r.name === name);
    summary[`${config} ${name}`] = {
      rps: median(rs.map((r) => r.rps)), rps_all: rs.map((r) => Math.round(r.rps)),
      cpu_ms_per_req: median(rs.map((r) => r.cpu_ms_per_req)), p50: median(rs.map((r) => r.p50)), p99: median(rs.map((r) => r.p99)),
      failures: rs.reduce((a, r) => a + r.check_failures + r.errors, 0),
    };
  }
  results.summary = summary;
  writeFileSync(join(out, 'runs.json'), JSON.stringify(results, null, 2));
  console.log(JSON.stringify(summary, null, 2));
} finally {
  try { process.kill(-api.pid, 'SIGTERM'); } catch {}
  await sleep(500);
  rmSync(dbDir, { recursive: true, force: true });
}
