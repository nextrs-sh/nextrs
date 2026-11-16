#!/usr/bin/env node
// Independent-application certification (Phase 3B.8): one unchanged build,
// served by `next start` and by NextRS, compared request by request.
//
//   compare-app.mjs prepare --app NAME [--work DIR]
//   compare-app.mjs compare --app NAME [--work DIR] [--isolates N]
//
// applications/NAME/manifest.json gives the repository, the pinned commit,
// an optional subdirectory, install/build commands, environment, and the
// scenarios: { name, method, path, headers, body, expect: {status, contains,
// notContains, header} }. Every scenario is sent to both servers. A scenario
// passes when both servers satisfy its expectations and agree with each
// other on status, Location, Content-Type and the normalized body (build
// independent: both serve the same build; per-request values such as dates,
// nonces and CSRF tokens are masked by the manifest's `mask` patterns).

import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createWriteStream, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../..');
const args = process.argv.slice(2);
const command = args[0];
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? fallback : args[i + 1];
};
const appName = opt('app');
const manifest = JSON.parse(readFileSync(join(here, '../applications', appName, 'manifest.json'), 'utf8'));
const work = resolve(opt('work', join(process.env.HOME, '.cache/nextrs-phase3b/independent')));
const checkout = join(work, appName);
const appDir = join(checkout, manifest.subdir ?? '');
const isolates = Number(opt('isolates', 1));
const nextrsBin = resolve(opt('nextrs', join(repoRoot, 'target/release/nextrs')));
const outDir = join(repoRoot, 'benchmarks/certification/results/independent', appName, `i${isolates}`);
const ports = { next: 3410, nextrs: 3420 };

const log = (...a) => console.error(`[${appName}]`, ...a);
function run(cmd, cmdArgs, options = {}) {
  log('$', cmd, cmdArgs.join(' '));
  const r = spawnSync(cmd, cmdArgs, { stdio: 'inherit', ...options });
  if (r.status !== 0) throw new Error(`${cmd} exited ${r.status}`);
}
const capture = (cmd, cmdArgs, options = {}) =>
  spawnSync(cmd, cmdArgs, { encoding: 'utf8', ...options }).stdout.trim();

function prepare() {
  mkdirSync(work, { recursive: true });
  if (!existsSync(checkout)) {
    // Sparse, shallow fetch of exactly the pinned commit.
    run('git', ['init', '--quiet', checkout]);
    run('git', ['remote', 'add', 'origin', manifest.repository], { cwd: checkout });
    if (manifest.subdir) {
      run('git', ['sparse-checkout', 'set', '--no-cone', `/${manifest.subdir}/`], { cwd: checkout });
    }
    run('git', ['fetch', '--quiet', '--depth', '1', '--filter=blob:none', 'origin', manifest.commit], { cwd: checkout });
    run('git', ['-c', 'advice.detachedHead=false', 'checkout', '--quiet', 'FETCH_HEAD'], { cwd: checkout });
  }
  const head = capture('git', ['rev-parse', 'HEAD'], { cwd: checkout });
  if (head !== manifest.commit) throw new Error(`checkout is at ${head}, not ${manifest.commit}`);
  const env = { ...process.env, NEXT_TELEMETRY_DISABLED: '1', ...(manifest.buildEnv ?? {}) };
  run(manifest.install[0], manifest.install.slice(1), { cwd: appDir, env });
  run(manifest.build[0], manifest.build.slice(1), { cwd: appDir, env });
  const nextVersion = JSON.parse(readFileSync(join(appDir, 'node_modules/next/package.json'), 'utf8')).version;
  const report = {
    commit: head,
    nextVersion,
    reactVersion: JSON.parse(readFileSync(join(appDir, 'node_modules/react/package.json'), 'utf8')).version,
    trackedChanges: capture('git', ['status', '--porcelain', '--untracked-files=no'], { cwd: checkout }),
    node: capture('node', ['--version']),
  };
  mkdirSync(outDir, { recursive: true });
  writeFileSync(join(dirname(outDir), 'build.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  if (report.trackedChanges) throw new Error('tracked files changed during install/build');
}

function start(name, cmd, cmdArgs, env) {
  mkdirSync(outDir, { recursive: true });
  const out = createWriteStream(join(outDir, `${name}.log`));
  const child = spawn(cmd, cmdArgs, { cwd: appDir, env, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.pipe(out);
  child.stderr.pipe(out);
  return child;
}
async function waitUp(port, child, name) {
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`${name} exited ${child.exitCode} (see ${name}.log)`);
    try {
      const r = await fetch(`http://127.0.0.1:${port}${manifest.health ?? '/'}`, { redirect: 'manual' });
      await r.arrayBuffer();
      return;
    } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`${name} did not start`);
}
const stop = (child) =>
  new Promise((done) => {
    if (!child || child.exitCode !== null) return done();
    child.once('exit', done);
    try {
      process.kill(-child.pid, 'SIGTERM');
    } catch {
      done();
    }
    setTimeout(() => {
      try {
        process.kill(-child.pid, 'SIGKILL');
      } catch {}
    }, 10_000);
  });

function normalize(body) {
  let text = body;
  for (const pattern of manifest.mask ?? []) text = text.replace(new RegExp(pattern, 'g'), '<masked>');
  return text;
}

/** `{{name}}` in a path or body: a value an earlier scenario extracted from this server's responses. */
const fillVars = (text, vars) => (text == null ? text : text.replace(/\{\{(\w+)\}\}/g, (_, k) => encodeURIComponent(vars.get(k) ?? '')));

async function send(port, scenario, jar, vars = new Map()) {
  const headers = { ...(scenario.headers ?? {}) };
  if (jar.size) headers.cookie = [...jar].map(([k, v]) => `${k}=${v}`).join('; ');
  const started = performance.now();
  const response = await fetch(`http://127.0.0.1:${port}${fillVars(scenario.path, vars)}`, {
    method: scenario.method ?? 'GET',
    headers,
    body: fillVars(scenario.body, vars),
    redirect: 'manual',
  });
  const body = await response.text();
  for (const cookie of response.headers.getSetCookie()) {
    const [pair] = cookie.split(';');
    const eq = pair.indexOf('=');
    jar.set(pair.slice(0, eq), pair.slice(eq + 1));
  }
  for (const [name, pattern] of Object.entries(scenario.extract ?? {})) {
    const match = new RegExp(pattern).exec(body);
    if (match) vars.set(name, match[1]);
  }
  return {
    status: response.status,
    location: response.headers.get('location'),
    contentType: response.headers.get('content-type'),
    headers: Object.fromEntries(response.headers),
    setCookieNames: response.headers.getSetCookie().map((c) => c.split('=')[0]).sort(),
    body,
    digest: createHash('sha256').update(normalize(body)).digest('hex').slice(0, 16),
    ms: Math.round(performance.now() - started),
  };
}

function check(scenario, result) {
  const problems = [];
  const e = scenario.expect ?? {};
  if (e.status !== undefined && result.status !== e.status) problems.push(`status ${result.status}, expected ${e.status}`);
  for (const text of [].concat(e.contains ?? [])) if (!result.body.includes(text)) problems.push(`body lacks ${JSON.stringify(text)}`);
  for (const text of [].concat(e.notContains ?? [])) if (result.body.includes(text)) problems.push(`body has ${JSON.stringify(text)}`);
  for (const [name, value] of Object.entries(e.header ?? {})) {
    const actual = result.headers[name.toLowerCase()];
    if (value === true ? actual == null : !String(actual ?? '').includes(value)) problems.push(`header ${name}=${actual}`);
  }
  if (e.location && !String(result.location ?? '').includes(e.location)) problems.push(`location ${result.location}`);
  return problems;
}

async function compare() {
  rmSync(outDir, { recursive: true, force: true });
  const env = { PATH: process.env.PATH, HOME: process.env.HOME, NEXT_TELEMETRY_DISABLED: '1', ...(manifest.runEnv ?? {}) };
  const next = start('next', 'node', [join(appDir, 'node_modules/next/dist/bin/next'), 'start', '-p', String(ports.next), '-H', '127.0.0.1'], { ...env, NODE_ENV: 'production' });
  const nextrsArgs = ['start', appDir, '--js-runtime', 'embedded-v8', '--port', String(ports.nextrs), '--isolates', String(isolates), ...(manifest.nextrsArgs ?? [])];
  for (const name of Object.keys(manifest.runEnv ?? {})) nextrsArgs.push('--js-env', name);
  const nextrs = start('nextrs', nextrsBin, nextrsArgs, { ...env, NO_COLOR: '1' });
  const results = [];
  let startup = {};
  try {
    let t = Date.now();
    await waitUp(ports.next, next, 'next');
    startup.next = Date.now() - t;
    t = Date.now();
    await waitUp(ports.nextrs, nextrs, 'nextrs');
    startup.nextrs = Date.now() - t;
    const jars = { next: new Map(), nextrs: new Map() };
    const vars = { next: new Map(), nextrs: new Map() };
    for (const scenario of manifest.scenarios) {
      if (scenario.resetCookies) {
        jars.next.clear();
        jars.nextrs.clear();
      }
      const a = await send(ports.next, scenario, jars.next, vars.next);
      const b = await send(ports.nextrs, scenario, jars.nextrs, vars.nextrs);
      const problems = [
        ...check(scenario, a).map((p) => `next start: ${p}`),
        ...check(scenario, b).map((p) => `nextrs: ${p}`),
      ];
      if (a.status !== b.status) problems.push(`status differs: ${a.status} vs ${b.status}`);
      if ((a.location ?? '') !== (b.location ?? '')) problems.push(`location differs: ${a.location} vs ${b.location}`);
      if ((a.contentType ?? '') !== (b.contentType ?? '')) problems.push(`content-type differs: ${a.contentType} vs ${b.contentType}`);
      if (!scenario.bodyMayDiffer && a.digest !== b.digest) problems.push(`body differs (${a.body.length} vs ${b.body.length} bytes)`);
      if (JSON.stringify(a.setCookieNames) !== JSON.stringify(b.setCookieNames))
        problems.push(`set-cookie names differ: ${a.setCookieNames} vs ${b.setCookieNames}`);
      results.push({
        name: scenario.name,
        method: scenario.method ?? 'GET',
        path: scenario.path,
        ok: problems.length === 0,
        problems,
        next: { status: a.status, ms: a.ms, bytes: a.body.length, digest: a.digest },
        nextrs: { status: b.status, ms: b.ms, bytes: b.body.length, digest: b.digest },
      });
      if (problems.length) {
        mkdirSync(join(outDir, 'diff'), { recursive: true });
        const file = scenario.name.replace(/[^a-z0-9-]+/gi, '_');
        writeFileSync(join(outDir, 'diff', `${file}.next.txt`), a.body);
        writeFileSync(join(outDir, 'diff', `${file}.nextrs.txt`), b.body);
      }
    }
  } finally {
    await stop(next);
    await stop(nextrs);
  }
  const summary = {
    app: appName,
    commit: manifest.commit,
    nextrsCommit: capture('git', ['rev-parse', 'HEAD'], { cwd: repoRoot }),
    isolates,
    startupMs: startup,
    total: results.length,
    passed: results.filter((r) => r.ok).length,
    failed: results.filter((r) => !r.ok).length,
    results,
  };
  writeFileSync(join(outDir, 'summary.json'), JSON.stringify(summary, null, 2));
  for (const r of results) log(`${r.ok ? 'PASS' : 'FAIL'} ${r.name} ${r.method} ${r.path} ${r.next.status}/${r.nextrs.status} ${r.problems.join('; ')}`);
  log(`${summary.passed}/${summary.total} scenarios passed`);
  process.exitCode = summary.failed ? 1 : 0;
}

if (command === 'prepare') prepare();
else if (command === 'compare') await compare();
else {
  console.error('usage: compare-app.mjs prepare|compare --app NAME');
  process.exitCode = 2;
}
