#!/usr/bin/env node
// Real-application certification runner (Phase 3B).
//
// The application under test is read-only: it is cloned at a pinned commit
// into a disposable work directory, installed with its frozen lockfile and
// built with its own build command. Nothing in its tree is edited; every
// difference between runtimes is configuration passed from outside
// (environment variables, ports, NextRS flags).
//
//   certify.mjs prepare  --app velasia-airways --work DIR
//   certify.mjs verify   --app velasia-airways --work DIR
//   certify.mjs up       --app velasia-airways --work DIR --runtime node|nextrs [--isolates N]
//   certify.mjs e2e      --app velasia-airways --work DIR --runtime node|nextrs [--isolates N]
//                        [--label NAME] [-- extra playwright args]
//
// `up` keeps the API (fresh TalaDB file) and the web server running until
// SIGINT/SIGTERM. `e2e` starts both, runs the application's own Playwright
// suite against them, and stores the JSON report, server logs and a summary
// under benchmarks/certification/results/<app>/<label>/.
//
// Requirements: node and pnpm on PATH (the app's documented versions), the
// NextRS binary (--nextrs, default target/release/nextrs).

import { spawn, spawnSync } from 'node:child_process';
import { createWriteStream, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, cpSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../..');

function parseArgs(argv) {
  const [command, ...rest] = argv;
  const opts = { _: [], extra: [] };
  for (let i = 0; i < rest.length; i++) {
    const arg = rest[i];
    if (arg === '--') {
      opts.extra = rest.slice(i + 1);
      break;
    }
    if (arg.startsWith('--')) {
      const key = arg.slice(2);
      const next = rest[i + 1];
      // --nextrs-args takes NextRS flags as its value ("--a --b x").
      if (next === undefined || (next.startsWith('--') && key !== 'nextrs-args')) opts[key] = true;
      else {
        opts[key] = next;
        i++;
      }
    } else opts._.push(arg);
  }
  return { command, opts };
}

const { command, opts } = parseArgs(process.argv.slice(2));
const appName = opts.app ?? 'velasia-airways';
const manifest = JSON.parse(
  readFileSync(join(here, '../applications', appName, 'manifest.json'), 'utf8'),
);
const work = resolve(opts.work ?? join(process.env.HOME, '.cache/nextrs-phase3b'));
const src = resolve(opts.src ?? join(work, `${appName}-src`));
const nextrsBin = resolve(opts.nextrs ?? join(repoRoot, 'target/release/nextrs'));
const isolates = Number(opts.isolates ?? 1);
const apiPort = Number(opts['api-port'] ?? manifest.ports.api);
const webPort = Number(opts['web-port'] ?? manifest.ports.web);
const metricsPort = Number(opts['metrics-port'] ?? webPort + 1000);

function log(...args) {
  console.error('[certify]', ...args);
}

function run(cmd, args, options = {}) {
  log('$', cmd, args.join(' '), options.cwd ? `(in ${options.cwd})` : '');
  const result = spawnSync(cmd, args, { stdio: 'inherit', ...options });
  if (result.status !== 0) throw new Error(`${cmd} ${args.join(' ')} exited with ${result.status}`);
}

function capture(cmd, args, options = {}) {
  const result = spawnSync(cmd, args, { encoding: 'utf8', ...options });
  if (result.status !== 0) throw new Error(`${cmd} ${args.join(' ')}: ${result.stderr}`);
  return result.stdout.trim();
}

/** Tracked-file integrity of the checkout: HEAD, modified tracked files, index hash. */
function integrity(dir) {
  const head = capture('git', ['rev-parse', 'HEAD'], { cwd: dir });
  const modified = capture('git', ['status', '--porcelain=v1', '--untracked-files=no'], { cwd: dir });
  // Every tracked path's content hash in the working tree, compared with the commit's tree.
  const diff = capture('git', ['diff', '--stat', manifest.commit], { cwd: dir });
  const untracked = capture('git', ['status', '--porcelain=v1', '--untracked-files=all'], { cwd: dir })
    .split('\n')
    .filter((l) => l.startsWith('??'));
  return {
    head,
    pinned: manifest.commit,
    headMatchesPin: head === manifest.commit,
    modifiedTrackedFiles: modified ? modified.split('\n') : [],
    diffAgainstPin: diff,
    untrackedNotIgnored: untracked,
    clean: head === manifest.commit && !modified && !diff && untracked.length === 0,
  };
}

async function prepare() {
  mkdirSync(work, { recursive: true });
  if (!existsSync(src)) {
    run('git', ['clone', '--quiet', manifest.repository, src]);
  }
  run('git', ['-c', 'advice.detachedHead=false', 'checkout', '--quiet', '--detach', manifest.commit], { cwd: src });
  const env = { ...process.env, ...manifest.buildEnv };
  delete env.CI; // as a developer machine; Velasia's CI sets it only for its own reporter choices
  run(manifest.install[0], manifest.install.slice(1), { cwd: src, env: { ...env, CI: '1' } });
  run(manifest.build[0], manifest.build.slice(1), { cwd: src, env });
  const report = {
    at: new Date().toISOString(),
    node: capture('node', ['--version']),
    pnpm: capture('pnpm', ['--version'], { cwd: src }),
    buildId: readFileSync(join(src, manifest.app.dir, '.next/BUILD_ID'), 'utf8').trim(),
    integrity: integrity(src),
  };
  console.log(JSON.stringify(report, null, 2));
  if (!report.integrity.clean) throw new Error('the checkout changed during install/build');
}

function verify() {
  const report = integrity(src);
  console.log(JSON.stringify(report, null, 2));
  if (!report.clean) process.exitCode = 1;
}

async function waitFor(url, timeoutMs, child, name) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode) {
      throw new Error(`${name} exited (${child.exitCode ?? child.signalCode}) before ${url} answered`);
    }
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(2000) });
      if (response.ok) return Date.now();
      await response.body?.cancel();
    } catch {}
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`${url} did not answer within ${timeoutMs} ms`);
}

function startProcess(name, cmd, args, { cwd, env, logFile }) {
  log(`start ${name}:`, cmd, args.join(' '));
  const out = createWriteStream(logFile);
  const child = spawn(cmd, args, { cwd, env, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.pipe(out);
  child.stderr.pipe(out);
  return child;
}

function stopProcess(child) {
  if (!child || child.exitCode !== null || child.signalCode) return Promise.resolve();
  return new Promise((resolveStop) => {
    const timer = setTimeout(() => {
      try {
        process.kill(-child.pid, 'SIGKILL');
      } catch {}
    }, 15000);
    child.once('exit', () => {
      clearTimeout(timer);
      resolveStop();
    });
    try {
      process.kill(-child.pid, 'SIGTERM');
    } catch {
      clearTimeout(timer);
      resolveStop();
    }
  });
}

function fill(template, vars) {
  return template.replace(/\{(\w+)\}/g, (_, key) => {
    if (!(key in vars)) throw new Error(`unknown placeholder {${key}}`);
    return String(vars[key]);
  });
}

/** Starts the API on a fresh database and the chosen web runtime. */
async function up(runtime, stateDir) {
  mkdirSync(stateDir, { recursive: true });
  // As Velasia's playwright.config.ts does: a fresh orders database per run in
  // the OS temporary directory (tmpfs here). TalaDB fsyncs every order write, so
  // a spinning disk would add seconds per checkout.
  const dbDir = mkdtempSync(join(opts['db-root'] ?? tmpdir(), 'velasia-cert-db-'));
  const baseUrl = `http://127.0.0.1:${webPort}`;
  const apiUrl = `http://127.0.0.1:${apiPort}`;
  const cleanEnv = { PATH: process.env.PATH, HOME: process.env.HOME, LANG: 'C.UTF-8' };
  const apiEnv = {
    ...cleanEnv,
    ...manifest.api.env,
    RETAILING_API_PORT: String(apiPort),
    VELASIA_TODAY: manifest.today,
    [manifest.api.databaseEnv]: join(dbDir, 'velasia.db'),
  };
  const api = startProcess('api', manifest.api.command[0], manifest.api.command.slice(1), {
    cwd: src,
    env: apiEnv,
    logFile: join(stateDir, 'api.log'),
  });
  const servers = { api, dbDir };
  try {
    await waitFor(apiUrl + manifest.api.health, manifest.api.startupTimeoutMs, api, 'api');
    const webEnv = {
      ...manifest.web.env,
      SITE_URL: baseUrl,
      RETAILING_API_URL: apiUrl,
    };
    const started = Date.now();
    let web;
    if (runtime === 'node') {
      const b = manifest.web.baseline;
      web = startProcess('web (next standalone)', b.command[0], b.command.slice(1), {
        cwd: src,
        env: { ...cleanEnv, ...b.env, ...webEnv, PORT: String(webPort), HOSTNAME: '127.0.0.1' },
        logFile: join(stateDir, 'web.log'),
      });
    } else if (runtime === 'nextrs') {
      const vars = {
        standaloneDir: join(src, manifest.app.standaloneDir),
        appDir: join(src, manifest.app.dir),
        webPort,
        apiPort,
        isolates,
        metricsPort,
      };
      const args = manifest.web.nextrs.args.map((a) => fill(a, vars));
      if (opts.layout === 'app') args[1] = vars.appDir;
      for (const name of Object.keys(webEnv)) args.push('--js-env', name);
      args.push(...(opts['nextrs-args'] ? String(opts['nextrs-args']).split(' ') : []));
      web = startProcess('web (nextrs)', nextrsBin, args, {
        cwd: src,
        env: { ...cleanEnv, ...webEnv, RUST_LOG: process.env.RUST_LOG ?? 'info', NO_COLOR: '1' },
        logFile: join(stateDir, 'web.log'),
      });
    } else throw new Error(`unknown runtime ${runtime}`);
    servers.web = web;
    const ready = await waitFor(baseUrl + manifest.web.health, manifest.web.startupTimeoutMs, web, 'web');
    servers.startupMs = ready - started;
    log(`ready: ${runtime} on ${baseUrl} (startup ${servers.startupMs} ms), api on ${apiUrl}, db ${dbDir}`);
    writeFileSync(
      join(stateDir, 'servers.json'),
      JSON.stringify({ runtime, isolates, baseUrl, apiUrl, dbDir, apiPid: api.pid, webPid: web.pid, startupMs: servers.startupMs }, null, 2),
    );
    return servers;
  } catch (error) {
    await down(servers);
    throw error;
  }
}

async function down(servers, { keepDb = false } = {}) {
  await stopProcess(servers.web);
  await stopProcess(servers.api);
  if (!keepDb && servers.dbDir) rmSync(servers.dbDir, { recursive: true, force: true });
}

function summarize(reportPath) {
  const report = JSON.parse(readFileSync(reportPath, 'utf8'));
  const tests = [];
  const walk = (suite, path) => {
    for (const spec of suite.specs ?? []) {
      for (const test of spec.tests) {
        const last = test.results.at(-1);
        tests.push({
          file: spec.file,
          title: [...path, spec.title].filter(Boolean).join(' › '),
          project: test.projectName,
          status: test.status, // expected | unexpected | flaky | skipped
          outcome: last?.status,
          durationMs: last?.duration,
          error: last?.error?.message?.split('\n').slice(0, 6).join('\n'),
        });
      }
    }
    for (const child of suite.suites ?? []) walk(child, [...path, child.title]);
  };
  for (const suite of report.suites) walk(suite, []);
  const count = (s) => tests.filter((t) => t.status === s).length;
  return {
    stats: report.stats,
    total: tests.length,
    passed: count('expected'),
    failed: count('unexpected'),
    flaky: count('flaky'),
    skipped: count('skipped'),
    failures: tests.filter((t) => t.status === 'unexpected'),
    tests,
  };
}

async function e2e(runtime) {
  const label = opts.label ?? `${runtime}${runtime === 'nextrs' ? `-i${isolates}` : ''}`;
  const outDir = join(repoRoot, 'benchmarks/certification/results', appName, label);
  rmSync(outDir, { recursive: true, force: true });
  mkdirSync(outDir, { recursive: true });
  const servers = await up(runtime, outDir);
  let status = 1;
  try {
    const jsonPath = join(outDir, 'playwright.json');
    const env = { ...process.env, PLAYWRIGHT_JSON_OUTPUT_NAME: jsonPath };
    delete env.CI; // reuse the servers this runner started (reuseExistingServer: !CI)
    const args = [...manifest.e2e.command.slice(1), ...manifest.e2e.args];
    if (opts.config) {
      // A NextRS-owned Playwright configuration (scenarios/), run from the
      // clone for its node_modules; specs reach Velasia's helpers read-only.
      args.push('--config', resolve(String(opts.config)));
      env.VELASIA_SRC = src;
      env.NODE_PATH = join(src, 'node_modules');
      env.E2E_WEB_PORT = String(webPort);
    }
    args.push(...opts.extra);
    if (opts.workers) args.push(`--workers=${opts.workers}`);
    log('$', manifest.e2e.command[0], args.join(' '));
    const started = Date.now();
    const result = spawnSync(manifest.e2e.command[0], args, {
      cwd: src,
      env,
      stdio: ['ignore', 'pipe', 'inherit'],
      encoding: 'utf8',
      maxBuffer: 64 * 1024 * 1024,
    });
    writeFileSync(join(outDir, 'playwright.txt'), result.stdout ?? '');
    status = result.status;
    const summary = existsSync(jsonPath) ? summarize(jsonPath) : { error: 'no JSON report' };
    // Failure artefacts (traces, screenshots) land in the app's ignored test-results/.
    const artefacts = join(src, 'test-results');
    if (existsSync(artefacts)) cpSync(artefacts, join(outDir, 'test-results'), { recursive: true });
    const meta = {
      app: appName,
      commit: manifest.commit,
      runtime,
      isolates: runtime === 'nextrs' ? isolates : null,
      nextrsCommit: capture('git', ['rev-parse', 'HEAD'], { cwd: repoRoot }),
      nextrsDirty: capture('git', ['status', '--porcelain', '--untracked-files=no'], { cwd: repoRoot }) !== '',
      node: capture('node', ['--version']),
      startupMs: servers.startupMs,
      wallMs: Date.now() - started,
      exitCode: status,
      integrity: integrity(src),
    };
    writeFileSync(join(outDir, 'summary.json'), JSON.stringify({ meta, ...summary }, null, 2));
    log(`${label}: ${summary.passed}/${summary.total} passed, ${summary.failed} failed, ${summary.flaky} flaky, ${summary.skipped} skipped`);
    for (const f of summary.failures ?? []) log(`  FAIL ${f.file} › ${f.title} [${f.project}]`);
  } finally {
    await down(servers);
  }
  return status;
}

switch (command) {
  case 'prepare':
    await prepare();
    break;
  case 'verify':
    verify();
    break;
  case 'up': {
    const stateDir = resolve(opts.state ?? join(work, 'state', `${opts.runtime}-${Date.now()}`));
    const servers = await up(opts.runtime, stateDir);
    console.log(JSON.stringify({ stateDir, startupMs: servers.startupMs }));
    const stop = async () => {
      await down(servers, { keepDb: Boolean(opts['keep-db']) });
      process.exit(0);
    };
    process.on('SIGINT', stop);
    process.on('SIGTERM', stop);
    servers.web.on('exit', (code) => {
      log(`web exited (${code})`);
      stop();
    });
    break;
  }
  case 'e2e':
    process.exitCode = await e2e(opts.runtime);
    break;
  default:
    console.error('usage: certify.mjs prepare|verify|up|e2e --app NAME --work DIR [--runtime node|nextrs]');
    process.exitCode = 2;
}
