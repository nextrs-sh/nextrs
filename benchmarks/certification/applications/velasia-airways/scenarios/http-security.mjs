#!/usr/bin/env node
// HTTP-level security parity for Velasia Airways (Phase 3B.6), NextRS-owned.
//
//   http-security.mjs --node http://127.0.0.1:3300 --nextrs http://127.0.0.1:3200 \
//                     --build <clone>/apps/web/.next/standalone/apps/web/.next [--out file.json]
//
// Both servers serve the same build. Every probe goes to both; a probe
// passes when both meet its expectation and agree (status, the security
// headers, Location). Server Action ids come from the build's
// server-reference-manifest.json.
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const args = process.argv.slice(2);
const opt = (name) => args[args.indexOf(`--${name}`) + 1];
const targets = { node: opt('node'), nextrs: opt('nextrs') };
const build = opt('build');
const manifest = JSON.parse(readFileSync(join(build, 'server/server-reference-manifest.json'), 'utf8'));
// An action exported by a page that the shopping flow uses (any real id works for origin checks).
const actionIds = Object.keys(manifest.node ?? {});
const actionId = actionIds.find((id) => Object.keys(manifest.node[id].workers ?? {}).some((w) => w.includes('flights'))) ?? actionIds[0];
const actionPage = Object.keys(manifest.node[actionId].workers).find((w) => w.includes('flights')) ? '/flights' : '/';

const SECURITY_HEADERS = [
  'content-security-policy',
  'x-content-type-options',
  'referrer-policy',
  'x-frame-options',
  'permissions-policy',
  'cross-origin-opener-policy',
  'x-accel-buffering',
];

const probes = [];
const probe = (name, request, expect) => probes.push({ name, request, expect });

for (const path of ['/', '/flights', '/destinations/manila', '/booking/travellers', '/account', '/api/airports', '/api/internal/health/live', '/nope', '/sitemap.xml', '/_next/image?url=%2Fimages%2Fbrand%2Fhero.jpg&w=640&q=70']) {
  probe(`security headers ${path}`, { path }, { headers: SECURITY_HEADERS, absent: ['x-powered-by'] });
}
// next.config redirects answer before the route's headers apply (next start
// sends none of the custom headers on them); only parity is checked.
for (const [path, status] of [['/checkout', 307], ['/booking/travelers', 308]]) {
  probe(`config redirect ${path} (parity)`, { path }, { status, absent: ['x-powered-by'] });
}
const csp = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; connect-src 'self'; manifest-src 'self'; worker-src 'self' blob:; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";
probe('production CSP (no unsafe-eval, no ws:)', { path: '/' }, { header: { 'content-security-policy': csp } });

const actionHeaders = (extra = {}) => ({
  'next-action': actionId,
  'content-type': 'text/plain;charset=UTF-8',
  accept: 'text/x-component',
  ...extra,
});
probe('Server Action from a foreign Origin is refused', {
  method: 'POST', path: actionPage, headers: (base) => actionHeaders({ origin: 'https://evil.example', host: new URL(base).host }), body: '[]',
}, { notStatus: [200], bodyNot: ['Isolde'] });
probe('Server Action with a matching Origin is accepted for processing', {
  method: 'POST', path: actionPage, headers: (base) => actionHeaders({ origin: base }), body: '[]',
}, {});
probe('Server Action with a spoofed X-Forwarded-Host matching a foreign Origin (no proxy flags: passes through, as next start)', {
  method: 'POST', path: actionPage, headers: (base) => actionHeaders({ origin: 'https://evil.example', 'x-forwarded-host': 'evil.example', 'x-forwarded-proto': 'https' }), body: '[]',
}, {});
probe('Unknown Server Action id', {
  method: 'POST', path: actionPage, headers: (base) => ({ ...actionHeaders({ origin: base }), 'next-action': '00'.repeat(21) }), body: '[]',
}, { notStatus: [200] });
probe('Server Action body over the 128 kb limit', {
  method: 'POST', path: actionPage, headers: (base) => actionHeaders({ origin: base }), body: JSON.stringify(['x'.repeat(200 * 1024)]),
}, { notStatus: [200] });
probe('Host header injection does not change redirects', {
  path: '/checkout', headers: { host: 'evil.example' },
}, { status: 307 });
for (const path of ['/.env', '/_next/static/..%2f..%2f..%2fpackage.json', '/_next/static/../../../.env', '/public/../package.json', '/api/offers/..%2F..%2Fv1%2Forders', '/.next/server/server-reference-manifest.json', '/node_modules/next/package.json', '/apps/web/next.config.ts']) {
  probe(`no file disclosure ${path}`, { path }, { statusIn: [400, 404, 308], bodyNot: ['"dependencies"', 'TALADB_PATH', 'encryptionKey', 'cacheComponents'] });
}

async function send(base, request) {
  const headers = typeof request.headers === 'function' ? request.headers(base) : (request.headers ?? {});
  const response = await fetch(base + request.path, { method: request.method ?? 'GET', headers, body: request.body, redirect: 'manual' });
  const body = await response.text();
  return { status: response.status, headers: Object.fromEntries(response.headers), location: response.headers.get('location'), body };
}

function check(expect, r) {
  const problems = [];
  if (expect.status !== undefined && r.status !== expect.status) problems.push(`status ${r.status}`);
  if (expect.statusIn && !expect.statusIn.includes(r.status)) problems.push(`status ${r.status}`);
  if (expect.notStatus?.includes(r.status)) problems.push(`status ${r.status}`);
  for (const h of expect.headers ?? []) if (!r.headers[h]) problems.push(`missing ${h}`);
  for (const h of expect.absent ?? []) if (r.headers[h]) problems.push(`has ${h}`);
  for (const [h, v] of Object.entries(expect.header ?? {})) if (r.headers[h] !== v) problems.push(`${h}=${r.headers[h]}`);
  for (const t of expect.bodyNot ?? []) if (r.body.includes(t)) problems.push(`body has ${t}`);
  return problems;
}

const results = [];
for (const p of probes) {
  const a = await send(targets.node, p.request);
  const b = await send(targets.nextrs, p.request);
  const problems = [...check(p.expect, a).map((x) => `node: ${x}`), ...check(p.expect, b).map((x) => `nextrs: ${x}`)];
  if (a.status !== b.status) problems.push(`status differs ${a.status}/${b.status}`);
  if ((a.location ?? '') !== (b.location ?? '')) problems.push(`location differs ${a.location}/${b.location}`);
  for (const h of SECURITY_HEADERS) if ((a.headers[h] ?? '') !== (b.headers[h] ?? '')) problems.push(`${h} differs`);
  results.push({ name: p.name, ok: problems.length === 0, node: a.status, nextrs: b.status, location: [a.location, b.location], problems });
  console.log(`${problems.length ? 'FAIL' : 'pass'} ${p.name}: ${a.status}/${b.status} ${problems.join('; ')}`);
}
const summary = { actionId, actionPage, total: results.length, passed: results.filter((r) => r.ok).length, results };
if (opt('out')) writeFileSync(opt('out'), JSON.stringify(summary, null, 2));
console.log(`${summary.passed}/${summary.total} probes passed`);
process.exitCode = summary.passed === summary.total ? 0 : 1;
