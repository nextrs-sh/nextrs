#!/usr/bin/env node
// Phase 3C trusted-proxy validation on the unchanged Velasia build: the same
// probes against NextRS under each forwarded-header policy, plus the official
// standalone server for reference.
//
//   node benchmarks/phase3c/forwarded-matrix.mjs [--src <velasia clone>] [--out file.json]
//
// Modes (the test client always connects from 127.0.0.1):
//   next       official server (trusts X-Forwarded-* from anyone)
//   pass       NextRS default (as next start)
//   strip      --strip-forwarded-headers (clients connect directly)
//   trusted    --trusted-proxy 127.0.0.1/32 (the client is the proxy)
//   untrusted  --trusted-proxy 10.9.9.9/32 (the client is not the proxy)
//
// A Server Action is "refused" when Next answers its origin check failure
// (the same response a foreign Origin without forwarded headers gets), and
// "accepted" when it reaches the action (anything else).
import { spawn } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../..');
const argv = process.argv.slice(2);
const opt = (n, d) => (argv.includes(`--${n}`) ? argv[argv.indexOf(`--${n}`) + 1] : d);
const src = resolve(opt('src', join(process.env.HOME, '.cache/nextrs-phase3b/velasia-src')));
const nextrs = resolve(opt('nextrs', join(repo, 'target/release/nextrs')));
const manifest = JSON.parse(readFileSync(join(repo, 'benchmarks/certification/applications/velasia-airways/manifest.json'), 'utf8'));
const standalone = join(src, manifest.app.standaloneDir);
const WEB = 3800, API = 3810;
const base = `http://127.0.0.1:${WEB}`;
const apiUrl = `http://127.0.0.1:${API}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const cleanEnv = { PATH: process.env.PATH, HOME: process.env.HOME, LANG: 'C.UTF-8' };
const webEnv = { ...manifest.web.env, SITE_URL: base, RETAILING_API_URL: apiUrl };
const refs = JSON.parse(readFileSync(join(standalone, '.next/server/server-reference-manifest.json'), 'utf8'));
const actionId = Object.keys(refs.node).find((id) => Object.keys(refs.node[id].workers ?? {}).some((w) => w.includes('flights')));

async function ready(port, path) {
  for (let i = 0; i < 240; i++) {
    try { if ((await fetch(`http://127.0.0.1:${port}${path}`)).ok) return; } catch {}
    await sleep(250);
  }
  throw new Error(`not ready ${port}`);
}

const MODES = {
  next: null,
  pass: [],
  strip: ['--strip-forwarded-headers'],
  trusted: ['--trusted-proxy', '127.0.0.1/32'],
  untrusted: ['--trusted-proxy', '10.9.9.9/32'],
};

const action = (extra) => ({ method: 'POST', path: '/flights', headers: { 'next-action': actionId, 'content-type': 'text/plain;charset=UTF-8', accept: 'text/x-component', ...extra }, body: '[]' });
const PROBES = {
  'action-same-origin': action({ origin: base }),
  'action-foreign-origin': action({ origin: 'https://evil.example' }),
  'action-foreign-origin-forged-xfh': action({ origin: 'https://evil.example', 'x-forwarded-host': 'evil.example', 'x-forwarded-proto': 'https' }),
  'action-proxied-public-host': action({ origin: 'https://www.velasia.example', 'x-forwarded-host': 'www.velasia.example', 'x-forwarded-proto': 'https' }),
  'redirect-config-forged-xfh': { path: '/checkout', headers: { 'x-forwarded-host': 'evil.example', 'x-forwarded-proto': 'https' } },
  'redirect-auth-forged-xfh': { path: '/account/trips', headers: { 'x-forwarded-host': 'evil.example', 'x-forwarded-proto': 'https' } },
  'home-forged-xfh': { path: '/', headers: { 'x-forwarded-host': 'evil.example', 'x-forwarded-proto': 'https', 'x-forwarded-for': '203.0.113.9' } },
  'home-forged-forwarded': { path: '/', headers: { forwarded: 'for=203.0.113.9;host=evil.example;proto=https' } },
};

async function probe(p) {
  const r = await fetch(base + p.path, { method: p.method ?? 'GET', headers: p.headers, body: p.body, redirect: 'manual' });
  const body = await r.text();
  return { status: r.status, location: r.headers.get('location'), refusedOrigin: /Invalid Server Actions request|x-nextjs-action-not-found/i.test(body) || r.headers.get('x-nextjs-action-not-found') != null, evilInBody: body.includes('evil.example'), bodyHead: body.slice(0, 120) };
}

const dbDir = mkdtempSync(join(tmpdir(), 'velasia-fwd-db-'));
const api = spawn(manifest.api.command[0], manifest.api.command.slice(1), { cwd: src, env: { ...cleanEnv, ...manifest.api.env, RETAILING_API_PORT: String(API), VELASIA_TODAY: manifest.today, TALADB_PATH: join(dbDir, 'v.db'), LOG_LEVEL: 'error' }, stdio: 'ignore', detached: true });
const results = { actionId, modes: {} };
try {
  await ready(API, '/health');
  for (const [mode, flags] of Object.entries(MODES)) {
    let child;
    if (flags === null) {
      child = spawn('node', [join(standalone, 'server.js')], { cwd: src, env: { ...cleanEnv, NODE_ENV: 'production', ...webEnv, PORT: String(WEB), HOSTNAME: '127.0.0.1' }, stdio: 'ignore', detached: true });
    } else {
      const args = ['start', standalone, '--js-runtime', 'embedded-v8', '--port', String(WEB), '--net-allow', apiUrl, '--net-allow-addr', '127.0.0.1', ...flags];
      for (const name of Object.keys(webEnv)) args.push('--js-env', name);
      child = spawn(nextrs, args, { cwd: src, env: { ...cleanEnv, ...webEnv, RUST_LOG: 'warn' }, stdio: 'ignore', detached: true });
    }
    try {
      await ready(WEB, '/api/internal/health/ready');
      const out = {};
      for (const [name, p] of Object.entries(PROBES)) out[name] = await probe(p);
      // The refusal signature: a foreign Origin with no forwarded headers.
      const refusal = out['action-foreign-origin'];
      for (const name of Object.keys(out)) {
        if (!name.startsWith('action-')) continue;
        out[name].verdict = out[name].status === refusal.status && out[name].bodyHead === refusal.bodyHead ? 'refused' : 'accepted';
      }
      results.modes[mode] = out;
      console.log(`== ${mode}`);
      for (const [name, r] of Object.entries(out)) console.log(`  ${name}: ${r.status} ${r.verdict ?? ''} ${r.location ? `-> ${r.location}` : ''} ${r.evilInBody ? 'BODY-HAS-FORGED-HOST' : ''}`);
    } finally {
      try { process.kill(-child.pid, 'SIGTERM'); } catch {}
      await sleep(1500);
    }
  }
} finally {
  try { process.kill(-api.pid, 'SIGTERM'); } catch {}
  rmSync(dbDir, { recursive: true, force: true });
}
writeFileSync(resolve(opt('out', join(repo, 'benchmarks/results/phase3c/security/forwarded-matrix.json'))), JSON.stringify(results, null, 2));
