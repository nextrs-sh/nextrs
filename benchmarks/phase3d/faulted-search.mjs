#!/usr/bin/env node
// Phase 3D: faulted Velasia flight searches, classified independently, on the
// official Next.js standalone server and on NextRS (1 and 4 isolates), with
// the same build, the same retailing API and a fresh TalaDB file.
//
// Background: in the Phase 3C six-hour soak at 4 isolates, 1 of 63,469
// faulted searches (fault `malformed`) answered 200 without the
// "Something went wrong loading flights" copy. Only a 200-byte prefix of
// that body was kept, so its content is unknown. This probe looks for the
// mechanism(s) that can produce such a response and checks that both servers
// behave the same way.
//
// Every response is classified on its own (not against what the soak driver
// looked for): which of Velasia's ERROR_COPY titles appear, whether
// data-testid="results-error", "flight-card" or "empty-*" appear, whether a
// React error digest is present, whether the HTML document is complete,
// how many Suspense boundaries are still pending, status and byte length.
// Every response whose outcome differs from the scenario's expectation is
// written to disk in full (capped at 512 KiB) with its timing.
//
// Scenarios (per runtime):
//   basic-*          one cookie fault each (none, unavailable, timeout,
//                    malformed, empty, delay=1000/2000/3000/4000)
//   concurrent       faulted and unfaulted searches for the same dates,
//                    24 in flight
//   rsc-*            App Router navigation requests (rsc: 1,
//                    next-router-state-tree, then the `_rsc` the server asks
//                    for), faulted and unfaulted
//   cancel           client aborts after 1-40 ms, then normal requests, then
//                    executor health (NextRS /metrics.json)
//   api-paused-*     the API process group is SIGSTOPped while malformed
//                    (and unfaulted) searches are in flight, for 1.2 s (below
//                    the 2.5 s client timeout), 2.5 s (at it) or 4 s (above
//                    it), then SIGCONT
//   api-latency-*    the API is restarted with RETAILING_API_LATENCY_MS
//                    (1500 or 3500) so every malformed answer arrives below /
//                    above the client timeout (deterministic mechanism test)
//   api-killed       the API is stopped (connection refused)
//   mixed            randomized load replaying the soak's traffic mix (the
//                    kinds that need no signed-in state) with fault storms
//
//   export PATH=$HOME/.cache/nextrs-phase3a/node-v24.18.0-linux-x64/bin:$PATH
//   node benchmarks/phase3d/faulted-search.mjs \
//     [--runtimes next,nextrs-i1,nextrs-i4] [--mixed-minutes 6] [--rounds 30]
//     [--scenarios basic,concurrent,rsc,cancel,paused,latency,killed,mixed]
//     [--src ~/.cache/nextrs-phase3b/velasia-src] [--nextrs target/release/nextrs]
//     [--out benchmarks/results/phase3d/faulted-search] [--port 3910] [--seed 11]
//   node benchmarks/phase3d/faulted-search.mjs --reconcile-soak \
//     benchmarks/results/phase3c/soak-i4/velasia   (no servers; see below)
//
// The Velasia clone is only read: the API runs from it with a TalaDB file in
// the OS temporary directory; the web servers serve its standalone build.
import http from 'node:http';
import { spawn } from 'node:child_process';
import { webcrypto } from 'node:crypto';
import { mkdirSync, mkdtempSync, openSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const argv = process.argv.slice(2);
const opt = (name, fallback) => (argv.includes(`--${name}`) ? argv[argv.indexOf(`--${name}`) + 1] : fallback);
const src = resolve(opt('src', join(process.env.HOME, '.cache/nextrs-phase3b/velasia-src')));
const out = resolve(opt('out', join(repo, 'benchmarks/results/phase3d/faulted-search')));
const nextrsBin = resolve(opt('nextrs', join(repo, 'target/release/nextrs')));
const runtimes = opt('runtimes', 'next,nextrs-i1,nextrs-i4').split(',');
const scenarioSet = new Set(opt('scenarios', 'basic,concurrent,rsc,cancel,paused,latency,killed,mixed').split(','));
const mixedMinutes = Number(opt('mixed-minutes', 6));
const rounds = Number(opt('rounds', 30));
const seed = Number(opt('seed', 11));
const WEB = Number(opt('port', 3910));
const METRICS = WEB + 1;
const API = WEB + 10;
const CLIENT_TIMEOUT_MS = 2500;
const BODY_CAP = 512 * 1024;
const MAX_CAPTURES = 25;
mkdirSync(out, { recursive: true });

const manifest = JSON.parse(readFileSync(join(repo, 'benchmarks/certification/applications/velasia-airways/manifest.json'), 'utf8'));
const standalone = join(src, manifest.app.standaloneDir);
const env0 = { PATH: process.env.PATH, HOME: process.env.HOME, LANG: 'C.UTF-8' };
const webEnv = { ...manifest.web.env, SITE_URL: `http://127.0.0.1:${WEB}`, RETAILING_API_URL: `http://127.0.0.1:${API}` };
if (webEnv.RETAILING_API_FORWARD_FAULTS !== 'enabled' || manifest.api.env.RETAILING_API_FAULTS !== 'enabled') throw new Error('manifest does not enable fault forwarding');
if (Number(webEnv.RETAILING_API_TIMEOUT_MS) !== CLIENT_TIMEOUT_MS) throw new Error(`expected RETAILING_API_TIMEOUT_MS=${CLIENT_TIMEOUT_MS}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.error(`[faulted-search ${new Date().toISOString()}]`, ...a);

function rng(s) {
  return () => {
    s |= 0; s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// --- HTTP ----------------------------------------------------------------
const agent = new http.Agent({ keepAlive: true, maxSockets: 64 });
function send({ port = WEB, method = 'GET', path, headers = {}, body, abortAfterMs, timeoutMs = 30000 }) {
  return new Promise((done) => {
    const t0 = process.hrtime.bigint();
    const ms = () => Number(process.hrtime.bigint() - t0) / 1e6;
    let finished = false;
    let ttfb = null;
    const finish = (r) => { if (!finished) { finished = true; done({ ms: ms(), ttfbMs: ttfb, ...r }); } };
    const req = http.request({ host: '127.0.0.1', port, method, path, headers, agent: abortAfterMs ? false : agent }, (res) => {
      ttfb = ms();
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

// --- classification --------------------------------------------------------
// ERROR_COPY titles (apps/web/src/features/flights/results-states.tsx), as
// ASCII-only substrings so curly apostrophes do not matter.
const COPY = {
  unavailable: 'load flights right now',
  timeout: 'Flight search is taking too long',
  'bad-response': 'Something went wrong loading flights',
  rejected: 'run this search',
  'not-found': 'That flight is no longer available',
};
const count = (s, re) => (s.match(re) ?? []).length;
export function classify(res) {
  if (res.status === 0) return { status: 0, error: res.error, outcome: `client:${res.error}` };
  const body = res.body.toString('utf8');
  const ct = String(res.headers['content-type'] ?? '');
  const rsc = ct.includes('text/x-component');
  const titles = Object.entries(COPY).filter(([, t]) => body.includes(t)).map(([k]) => k);
  const c = {
    status: res.status,
    bytes: res.body.length,
    contentType: ct.split(';')[0],
    rsc,
    titles,
    resultsErrorMarkup: count(body, /data-testid="results-error"/g),
    resultsErrorAny: count(body, /data-testid\W{1,6}results-error/g),
    flightCardMarkup: count(body, /data-testid="flight-card"/g),
    flightCardAny: count(body, /data-testid\W{1,6}flight-card/g),
    flightResultsAny: count(body, /data-testid\W{1,6}flight-results/g),
    flightNumbers: count(body, /\bNV\d{3}\b/g),
    empty: [...new Set(body.match(/empty-(?:filtered-out|no-route|cabin-not-offered|beyond-schedule|insufficient-seats|no-flights-on-date)/g) ?? [])],
    digest: /data-dgst=|"digest":/.test(body),
    errorBoundary: body.includes('load this step'),
    complete: rsc ? null : body.trimEnd().endsWith('</html>'),
    pendingBoundaries: count(body, /<!--\$\?-->/g),
    completedBoundaries: count(body, /\$RC\("B:|\$RC\(\\?"B:/g) + count(body, /<div hidden id="S:/g),
    location: res.headers.location ?? null,
    nextrsError: res.headers['x-nextrs-error'] ?? null,
  };
  c.outcome = outcomeOf(c);
  return c;
}
function outcomeOf(c) {
  if (c.status === 307) return 'redirect-307';
  if (c.status !== 200) return `status-${c.status}`;
  if (c.complete === false) return 'incomplete-document';
  if (c.titles.length > 1) return `error:multiple(${c.titles.join('+')})`;
  if (c.titles.length === 1) {
    if (c.resultsErrorAny === 0) return `copy-without-panel:${c.titles[0]}`;
    if (c.flightCardAny > 0) return `error+results:${c.titles[0]}`;
    return `error:${c.titles[0]}`;
  }
  if (c.digest || c.errorBoundary) return 'error-boundary';
  if (c.flightCardAny > 0) return 'results';
  if (c.empty.length > 0) return 'empty';
  return 'other';
}

// --- processes ---------------------------------------------------------------
const children = new Set();
const killGroup = (p, sig = 'SIGTERM') => { try { process.kill(-p.pid, sig); } catch {} };
function cleanup() { for (const p of children) { killGroup(p, 'SIGCONT'); killGroup(p, 'SIGKILL'); } }
process.on('SIGINT', () => { cleanup(); process.exit(130); });
process.on('SIGTERM', () => { cleanup(); process.exit(143); });
process.on('exit', cleanup);

async function waitReady(port, path, ms = 120000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    const r = await send({ port, path, timeoutMs: 2000 });
    if (r.status === 200) return;
    await sleep(250);
  }
  throw new Error(`port ${port}${path} not ready`);
}
async function waitClosed(port, ms = 15000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    const r = await send({ port, path: '/health', timeoutMs: 500 });
    if (r.status === 0 && r.error === 'ECONNREFUSED') return;
    await sleep(100);
  }
  throw new Error(`port ${port} still open`);
}

let api = null;
async function startApi(extraEnv = {}) {
  const dbDir = mkdtempSync(join(tmpdir(), 'velasia-p3d-'));
  // The same process `pnpm --filter @velasia/retailing-api start` runs (tsx
  // src/server.ts), started directly so the whole group can be paused.
  const proc = spawn(join(src, 'apps/retailing-api/node_modules/.bin/tsx'), ['src/server.ts'], {
    cwd: join(src, 'apps/retailing-api'),
    env: { ...env0, ...manifest.api.env, ...extraEnv, RETAILING_API_PORT: String(API), VELASIA_TODAY: manifest.today, TALADB_PATH: join(dbDir, 'velasia.db'), LOG_LEVEL: 'error' },
    stdio: 'ignore', detached: true,
  });
  children.add(proc);
  api = { proc, dbDir, env: extraEnv };
  await waitReady(API, '/health', 60000);
}
async function stopApi() {
  if (!api) return;
  killGroup(api.proc, 'SIGCONT');
  killGroup(api.proc, 'SIGKILL');
  children.delete(api.proc);
  await waitClosed(API);
  rmSync(api.dbDir, { recursive: true, force: true });
  api = null;
}

async function startWeb(runtime) {
  const logFd = openSync(join(out, `${runtime}.server.log`), 'w');
  let proc;
  if (runtime === 'next') {
    proc = spawn('node', [join(standalone, 'server.js')], { cwd: src, env: { ...env0, ...manifest.web.baseline.env, ...webEnv, PORT: String(WEB), HOSTNAME: '127.0.0.1' }, stdio: ['ignore', logFd, logFd], detached: true });
  } else {
    const isolates = Number(runtime.split('-i')[1]);
    const args = ['start', standalone, '--js-runtime', 'embedded-v8', '--port', String(WEB), '--isolates', String(isolates),
      '--net-allow', `http://127.0.0.1:${API}`, '--net-allow-addr', '127.0.0.1', '--metrics-addr', `127.0.0.1:${METRICS}`,
      '--strip-forwarded-headers', ...Object.keys(webEnv).flatMap((n) => ['--js-env', n])];
    proc = spawn(nextrsBin, args, { cwd: src, env: { ...env0, ...webEnv, RUST_LOG: 'nextrs=warn', NO_COLOR: '1' }, stdio: ['ignore', logFd, logFd], detached: true });
  }
  children.add(proc);
  await waitReady(WEB, manifest.web.health);
  if (runtime !== 'next') await waitReady(METRICS, '/readyz', 10000);
  return proc;
}
async function stopWeb(proc) {
  killGroup(proc, 'SIGTERM');
  for (let i = 0; i < 100 && (await send({ path: '/', timeoutMs: 300 })).error !== 'ECONNREFUSED'; i++) await sleep(100);
  killGroup(proc, 'SIGKILL');
  children.delete(proc);
}
async function executorState(runtime) {
  if (runtime === 'next') {
    const r = await send({ path: manifest.web.health, timeoutMs: 5000 });
    return { healthStatus: r.status };
  }
  const r = await send({ port: METRICS, path: '/metrics.json', timeoutMs: 5000 });
  let m = null;
  try { m = JSON.parse(r.body.toString()); } catch {}
  const ex = m?.executors ?? [];
  return {
    executors: ex.length, ready: ex.filter((e) => e.state === 'ready').length, restarts: ex.reduce((a, e) => a + (e.restarts ?? 0), 0),
    active: ex.reduce((a, e) => a + (e.active ?? 0), 0), queued: m?.queued ?? null,
    handlerErrors: m?.handler_errors ?? null, requestTimeouts: m?.request_timeouts ?? null, responsesCancelled: m?.responses_cancelled ?? null,
    rejectedOverloaded: m?.rejected_overloaded ?? null, rejectedUnavailable: m?.rejected_unavailable ?? null, completed: m?.requests_completed ?? null,
  };
}

// --- requests --------------------------------------------------------------------
const day = (i) => `2026-12-${String(5 + (((i % 20) + 20) % 20)).padStart(2, '0')}`;
const searchPath = (d) => `/flights?trip=one-way&from=MNL&to=SIN&depart=${d}&adults=1&cabin=economy`;
const faultCookie = (f) => (f ? { cookie: `velasia-fault=${f}` } : {});
const EXPECT = {
  none: ['results'], unavailable: ['error:unavailable'], timeout: ['error:timeout'], malformed: ['error:bad-response'],
  empty: ['empty'], 'delay=1000': ['results'], 'delay=2000': ['results'], 'delay=3000': ['error:timeout'], 'delay=4000': ['error:timeout'],
};

// --- recording -----------------------------------------------------------------
let current = null; // { runtime, summary }
function scenarioRec(name) {
  current.summary.scenarios[name] ??= { n: 0, matched: 0, mismatched: 0, outcomes: {}, byFault: {}, ms: [], mismatches: [] };
  return current.summary.scenarios[name];
}
function record(name, { fault = 'none', expect, req, res, extra = {} }) {
  const s = scenarioRec(name);
  const c = classify(res);
  const ok = expect.includes(c.outcome);
  s.n++;
  s.outcomes[c.outcome] = (s.outcomes[c.outcome] ?? 0) + 1;
  const bf = (s.byFault[fault] ??= { n: 0, outcomes: {}, msMax: 0, over2500: 0 });
  bf.n++;
  bf.outcomes[c.outcome] = (bf.outcomes[c.outcome] ?? 0) + 1;
  bf.msMax = Math.max(bf.msMax, Math.round(res.ms));
  if (res.ms > CLIENT_TIMEOUT_MS) bf.over2500++;
  s.ms.push(res.ms);
  if (ok) s.matched++;
  else {
    s.mismatched++;
    const entry = { at: new Date().toISOString(), fault, expect, path: req.path, headers: req.headers ?? {}, ms: Math.round(res.ms), ttfbMs: res.ttfbMs && Math.round(res.ttfbMs), ...extra, classification: c };
    if (s.mismatches.length < MAX_CAPTURES && res.body) {
      const file = `${current.runtime}.${name}.${s.mismatches.length}.${c.rsc ? 'rsc.txt' : 'html'}`;
      writeFileSync(join(out, file), res.body.subarray(0, BODY_CAP));
      entry.bodyFile = file;
      entry.responseHeaders = res.headers;
    }
    if (s.mismatches.length < MAX_CAPTURES) s.mismatches.push(entry);
  }
  return c;
}
const pct = (a, p) => { if (!a.length) return null; const s = [...a].sort((x, y) => x - y); return Math.round(s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))]); };
function finalizeScenario(s) {
  const ms = s.ms;
  delete s.ms;
  s.msP50 = pct(ms, 50); s.msP99 = pct(ms, 99); s.msMax = pct(ms, 100);
}
async function pool(n, items, fn) {
  let i = 0;
  await Promise.all(Array.from({ length: n }, async () => { while (i < items.length) { const k = i++; await fn(items[k], k); } }));
}

// --- scenarios ---------------------------------------------------------------------
async function basic() {
  for (const fault of Object.keys(EXPECT)) {
    const items = Array.from({ length: rounds }, (_, i) => i);
    await pool(4, items, async (i) => {
      const req = { path: searchPath(day(i)), headers: faultCookie(fault === 'none' ? null : fault) };
      const res = await send(req);
      record(`basic-${fault}`, { fault, expect: EXPECT[fault], req, res });
    });
  }
}

async function concurrent() {
  const faults = ['malformed', 'unavailable', 'timeout', 'empty'];
  const items = [];
  for (let i = 0; i < rounds * 8; i++) items.push({ i, fault: i % 2 ? faults[(i >> 1) % 4] : null });
  await pool(24, items, async ({ i, fault }) => {
    const req = { path: searchPath(day(i >> 1)), headers: faultCookie(fault) };
    const res = await send(req);
    record('concurrent', { fault: fault ?? 'none', expect: EXPECT[fault ?? 'none'], req, res });
  });
}

async function rscHash(stateTree) {
  const input = ['0', '0', stateTree, '0'].join(',');
  const d = await webcrypto.subtle.digest('SHA-256', new TextEncoder().encode(input));
  return Buffer.from(new Uint8Array(d).subarray(0, 12)).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}
async function rsc() {
  // What the App Router sends for a soft navigation (router.push) to a
  // search: rsc: 1, the current router state tree (client-only fields
  // stripped, as prepareFlightRouterStateForRequest does), then the `_rsc`
  // the server asks for. Two origins: the home page, and the /flights page
  // (editing a search from the results page).
  const trees = {
    home: ['', { children: ['__PAGE__', {}] }],
    flights: ['', { children: ['flights', { children: ['__PAGE__', {}] }] }],
  };
  const s = current.summary.rsc = { redirects: {} };
  for (const [from, t] of Object.entries(trees)) {
    const tree = encodeURIComponent(JSON.stringify(t));
    const expectedHash = await rscHash(tree);
    const red = s.redirects[from] = { expectedHash, n: 0, locationMatchesHash: 0, other: [] };
    for (const fault of ['none', 'malformed', 'unavailable', 'timeout']) {
      const items = Array.from({ length: Math.ceil(rounds / 2) }, (_, i) => i);
      await pool(4, items, async (i) => {
        const headers = { rsc: '1', 'next-router-state-tree': tree, ...faultCookie(fault === 'none' ? null : fault) };
        const path = searchPath(day(i));
        const first = await send({ path, headers });
        red.n++;
        const loc = first.headers?.location ?? '';
        if (first.status === 307 && new URL(loc, 'http://x').searchParams.get('_rsc') === expectedHash) red.locationMatchesHash++;
        else if (red.other.length < 5) red.other.push({ status: first.status, location: loc });
        const req = { path: `${path}&_rsc=${expectedHash}`, headers };
        const res = await send(req);
        record(`rsc-from-${from}-${fault}`, { fault, expect: EXPECT[fault], req, res });
      });
    }
  }
}

async function cancel() {
  const before = await executorState(current.runtime);
  const r = rng(seed);
  const faults = [null, 'malformed', 'unavailable', 'timeout'];
  const aborted = { n: 0, completedBeforeAbort: 0 };
  const items = Array.from({ length: rounds * 4 }, (_, i) => i);
  await pool(8, items, async (i) => {
    const fault = faults[i % 4];
    const req = { path: searchPath(day(i)), headers: faultCookie(fault), abortAfterMs: 1 + Math.floor(r() * 40) };
    const res = await send(req);
    aborted.n++;
    if (res.status !== 0) {
      aborted.completedBeforeAbort++;
      record('cancel-completed', { fault: fault ?? 'none', expect: EXPECT[fault ?? 'none'], req, res });
    }
  });
  await sleep(3000);
  for (let i = 0; i < rounds; i++) {
    const fault = faults[i % 4];
    const req = { path: searchPath(day(i)), headers: faultCookie(fault) };
    record('cancel-after', { fault: fault ?? 'none', expect: EXPECT[fault ?? 'none'], req, res: await send(req) });
  }
  await sleep(1000);
  current.summary.cancel = { aborted, before, after: await executorState(current.runtime) };
}

async function paused() {
  for (const [label, pauseMs] of [['1200ms', 1200], ['2500ms', 2500], ['4000ms', 4000]]) {
    for (let k = 0; k < Math.ceil(rounds / 3); k++) {
      // Three malformed searches and one unfaulted, sent first; the API is
      // stopped while they are in flight (some may already be answered).
      const reqs = [
        { fault: 'malformed', req: { path: searchPath(day(k * 3)), headers: faultCookie('malformed') } },
        { fault: 'malformed', req: { path: searchPath(day(k * 3 + 1)), headers: faultCookie('malformed') } },
        { fault: 'malformed', req: { path: searchPath(day(k * 3 + 2)), headers: faultCookie('malformed') } },
        { fault: 'none', req: { path: searchPath(day(k * 3 + 7)), headers: {} } },
      ];
      const pending = reqs.map((x) => send(x.req).then((res) => ({ ...x, res })));
      await sleep(5 + (k % 4) * 5);
      const tStop = Date.now();
      killGroup(api.proc, 'SIGSTOP');
      await sleep(pauseMs);
      killGroup(api.proc, 'SIGCONT');
      const pausedMs = Date.now() - tStop;
      for (const { fault, req, res } of await Promise.all(pending)) {
        // Below the client timeout the malformed copy is expected; above it
        // the timeout copy is what the application should render.
        const expect = pauseMs < CLIENT_TIMEOUT_MS ? EXPECT[fault] : fault === 'malformed' ? ['error:bad-response', 'error:timeout'] : ['results', 'error:timeout'];
        const c = record(`api-paused-${label}`, { fault, expect, req, res, extra: { pausedMs } });
        const tally = (current.summary.pausedDetail ??= {});
        const key = `${label}/${fault}/${c.outcome}`;
        tally[key] = (tally[key] ?? 0) + 1;
      }
      await sleep(300);
    }
  }
}

async function latency() {
  for (const [label, ms, expectMalformed] of [['1500ms', '1500', ['error:bad-response']], ['3500ms', '3500', ['error:timeout']]]) {
    await stopApi();
    await startApi({ RETAILING_API_LATENCY_MS: ms });
    const items = Array.from({ length: rounds }, (_, i) => i);
    await pool(4, items, async (i) => {
      const fault = i % 3 === 2 ? null : 'malformed';
      const req = { path: searchPath(day(i)), headers: faultCookie(fault) };
      const expect = fault ? expectMalformed : ms === '1500' ? ['results'] : ['error:timeout'];
      record(`api-latency-${label}`, { fault: fault ?? 'none', expect, req, res: await send(req) });
    });
  }
  await stopApi();
  await startApi();
}

async function killed() {
  await stopApi();
  for (let i = 0; i < rounds; i++) {
    const fault = [null, 'malformed', 'unavailable', 'timeout'][i % 4];
    const req = { path: searchPath(day(i)), headers: faultCookie(fault) };
    record('api-killed', { fault: fault ?? 'none', expect: ['error:unavailable'], req, res: await send(req) });
  }
  await startApi();
  // Recovery: normal behaviour once the API is back.
  for (let i = 0; i < 8; i++) {
    const fault = [null, 'malformed'][i % 2];
    const req = { path: searchPath(day(i)), headers: faultCookie(fault) };
    record('api-killed-recovered', { fault: fault ?? 'none', expect: EXPECT[fault ?? 'none'], req, res: await send(req) });
  }
}

async function mixed() {
  // The soak's kinds (benchmarks/phase3c/velasia-soak.mjs) that need no
  // signed-in state; api-fault weight raised from 2 to 6 for more samples.
  const DEST = ['bangkok', 'cebu', 'davao', 'ho-chi-minh-city', 'hong-kong', 'kuala-lumpur', 'manila', 'osaka', 'seoul', 'singapore', 'taipei', 'tokyo'];
  const WIDTHS = [128, 256, 384, 640, 828, 1080, 1280, 1600, 1920];
  const rday = (r) => day(Math.floor(r() * 20));
  const kinds = {
    'ppr-home': { w: 10, make: () => ({ path: '/' }), expect: 'Beyond every horizon' },
    'ppr-destination': { w: 8, make: (r) => ({ path: `/destinations/${DEST[Math.floor(r() * DEST.length)]}` }), expect: '</html>' },
    'rsc-destination': { w: 3, make: (r) => ({ path: `/destinations/${DEST[Math.floor(r() * DEST.length)]}`, headers: { rsc: '1' } }), ok: [307] },
    'static-page': { w: 3, make: (r) => ({ path: ['/about', '/help', '/credits', '/destinations', '/robots.txt', '/sitemap.xml'][Math.floor(r() * 6)] }) },
    'flight-search-shell': { w: 4, make: () => ({ path: '/flights' }), expect: 'Velasia' },
    'flight-results': { w: 10, make: (r) => ({ path: searchPath(rday(r)) }), classify: true },
    'fare-offers': { w: 4, make: (r) => ({ path: `${searchPath(rday(r))}&fares=NV501-2026121${Math.floor(r() * 5) + 1}` }), ok: [200] },
    'api-route': { w: 6, make: (r) => ({ path: `/api/flights?from=MNL&to=SIN&departureDate=${rday(r)}&adults=1` }), expect: 'NV' },
    'account-anon': { w: 2, make: () => ({ path: '/account/trips' }), ok: [200, 303, 307, 308] },
    'image-hit': { w: 8, make: (r) => ({ path: `/_next/image?url=%2Fimages%2Fdestinations%2F${DEST[Math.floor(r() * 4)]}.jpg&w=640&q=70`, headers: { accept: 'image/webp' } }) },
    'image-transform': { w: 4, make: (r) => ({ path: `/_next/image?url=%2Fimages%2Fdestinations%2F${DEST[Math.floor(r() * DEST.length)]}.jpg&w=${WIDTHS[Math.floor(r() * WIDTHS.length)]}&q=${r() < 0.5 ? 70 : 80}`, headers: { accept: r() < 0.7 ? 'image/webp' : '*/*' } }), ok: [200, 503] },
    'not-found': { w: 1, make: () => ({ path: '/no-such-page' }), ok: [404] },
    'api-fault': { w: 6, make: (r) => { const f = ['unavailable', 'malformed', 'timeout'][Math.floor(r() * 3)]; return { path: searchPath(rday(r)), headers: faultCookie(f), fault: f }; }, classify: true },
    'abort-results': { w: 2, make: (r) => ({ path: searchPath(rday(r)), abortAfterMs: 2 + Math.floor(r() * 40) }), abort: true },
    'abort-home': { w: 1, make: (r) => ({ path: '/', abortAfterMs: 1 + Math.floor(r() * 5) }), abort: true },
  };
  const table = Object.entries(kinds).flatMap(([name, k]) => Array(k.w).fill(name));
  const other = current.summary.mixedOther = { requests: 0, ok: 0, aborted: 0, overload503: 0, unexpected: 0, samples: [], kinds: {} };
  const before = await executorState(current.runtime);
  const started = Date.now();
  const deadline = started + mixedMinutes * 60000;
  // Fault storms: 30 s out of every 2 minutes, a third of searches faulted.
  const storm = () => ((Date.now() - started) / 1000) % 120 > 90;
  async function worker(id) {
    const r = rng(seed * 1000 + id);
    while (Date.now() < deadline) {
      let name = table[Math.floor(r() * table.length)];
      if (storm() && name === 'flight-results' && r() < 0.33) name = 'api-fault';
      const k = kinds[name];
      const req = k.make(r);
      const res = await send(req);
      other.requests++;
      const ks = (other.kinds[name] ??= { n: 0, msMax: 0, over2500: 0 });
      ks.n++; ks.msMax = Math.max(ks.msMax, Math.round(res.ms)); if (res.ms > CLIENT_TIMEOUT_MS) ks.over2500++;
      if (k.abort && res.status === 0) { other.aborted++; continue; }
      if (res.status === 503 && res.headers['x-nextrs-error']) { other.overload503++; continue; }
      if (k.classify || k.abort && name === 'abort-results') {
        record(req.fault ? 'mixed-faulted' : 'mixed-unfaulted-search', { fault: req.fault ?? 'none', expect: EXPECT[req.fault ?? 'none'], req, res, extra: { storm: storm() } });
        continue;
      }
      const body = res.body?.toString('utf8') ?? '';
      const okStatus = (k.ok ?? [200]).includes(res.status);
      const okBody = !(res.status === 200 && k.expect && !body.includes(k.expect));
      if (okStatus && okBody) other.ok++;
      else {
        other.unexpected++;
        if (other.samples.length < 20) other.samples.push({ kind: name, path: req.path, status: res.status, error: res.error, ms: Math.round(res.ms), body: body.slice(0, 300) });
      }
    }
  }
  await Promise.all(Array.from({ length: 8 }, (_, i) => worker(i)));
  await sleep(1000);
  current.summary.mixedExecutors = { before, after: await executorState(current.runtime) };
}

// --- soak reconciliation -------------------------------------------------------------
// `--reconcile-soak <dir>`: no servers. Re-classifies the faulted-search
// records a Phase 3C soak kept (a 200-byte body prefix), and reconciles
// Velasia's own "retailing api call failed" log lines in that soak's NextRS
// log: every faulted search logs one line for the fare calendar ("calendar
// outbound") and one for the results ("search outbound"), each with the
// RetailingApiErrorKind the page rendered. A malformed search whose results
// call was classified "timeout" shows up as one search line too many of kind
// timeout and one too few of kind bad-response.
function reconcileSoak(dir) {
  const json = readdirSync(dir).find((f) => /^velasia-soak-i\d+\.json$/.test(f));
  const logName = readdirSync(dir).find((f) => /^nextrs-i\d+\.log(\.gz)?$/.test(f));
  const soak = JSON.parse(readFileSync(join(dir, json), 'utf8'));
  const raw = readFileSync(join(dir, logName));
  const text = (logName.endsWith('.gz') ? gunzipSync(raw) : raw).toString('utf8');
  const lines = [];
  for (const l of text.split('\n')) {
    if (!l.includes('retailing api call failed')) continue;
    lines.push({ t: Number(/"time":(\d+)/.exec(l)[1]), action: /"action":"([^"]+)"/.exec(l)[1], kind: /"kind":"([^"]+)"/.exec(l)[1] });
  }
  lines.sort((a, b) => a.t - b.t);
  const counts = {};
  for (const x of lines) counts[`${x.action}/${x.kind}`] = (counts[`${x.action}/${x.kind}`] ?? 0) + 1;
  const kinds = [...new Set(lines.map((x) => x.kind))];
  const searchMinusCalendar = Object.fromEntries(kinds.map((k) => [k, (counts[`search outbound/${k}`] ?? 0) - (counts[`calendar outbound/${k}`] ?? 0)]));
  // Pair calendar/search lines of the same kind within 50 ms (one search's
  // two calls normally finish within a few ms of each other).
  const orphansIn = (from, to) => {
    const win = lines.filter((x) => x.t >= from && x.t <= to).map((x) => ({ ...x, used: false }));
    for (let i = 0; i < win.length; i++) {
      if (win[i].used) continue;
      for (let j = i + 1; j < win.length && win[j].t - win[i].t <= 50; j++) {
        if (!win[j].used && win[j].action !== win[i].action && win[j].kind === win[i].kind) { win[i].used = win[j].used = true; break; }
      }
    }
    return win;
  };
  const bursts = [];
  for (let i = 0; i < lines.length; i++) {
    let j = i;
    while (j + 1 < lines.length && lines[j + 1].t - lines[i].t <= 5) j++;
    if (j - i + 1 >= 8) bursts.push({ at: new Date(lines[i].t).toISOString(), lines: j - i + 1, msSincePreviousLine: lines[i].t - (lines[i - 1]?.t ?? lines[i].t) });
    i = j;
  }
  const records = soak.errors.filter((e) => e.fault).map((e) => {
    const c = classify({ status: e.status, headers: { 'content-type': 'text/html' }, body: Buffer.from(e.body) });
    const at = Date.parse(e.at);
    const win = orphansIn(at - 8000, at + 1000);
    return {
      record: e,
      prefixClassification: c,
      prefixVerdict: 'The stored 200-byte prefix is only the start of <head>: no ERROR_COPY title, no results, no completion marker can appear in it, so the body itself cannot be classified.',
      logWindow: win.map((x) => ({ at: new Date(x.t).toISOString(), action: x.action, kind: x.kind, pairedWithin50ms: x.used })),
    };
  });
  const result = {
    soakDir: dir, soakJson: json, log: logName, soakStarted: soak.started, config: soak.config, totals: soak.totals,
    apiFault: soak.kinds?.['api-fault'], slowestKinds: Object.fromEntries(Object.entries(soak.kinds ?? {}).map(([k, v]) => [k, { count: v.count, msMax: Math.round(v.msMax), over5s: v.slow }])),
    logLines: lines.length, counts, searchMinusCalendar, burstsOf8LinesIn5ms: bursts, records,
  };
  const name = `reconcile-${dir.split('/').filter(Boolean).slice(-2).join('-')}.json`;
  writeFileSync(join(out, name), JSON.stringify(result, null, 1));
  console.log(JSON.stringify({ file: join(out, name), counts, searchMinusCalendar, bursts, unexpected: soak.totals.unexpected }, null, 1));
}
if (argv.includes('--reconcile-soak')) {
  reconcileSoak(resolve(opt('reconcile-soak')));
  process.exit(0);
}

// --- main ----------------------------------------------------------------------------
const SCENARIOS ={ basic, concurrent, rsc, cancel, paused, latency, killed, mixed };
const all = { started: new Date().toISOString(), config: { runtimes, rounds, mixedMinutes, seed, nextrsBin, velasiaCommit: manifest.commit, clientTimeoutMs: CLIENT_TIMEOUT_MS, webEnv: manifest.web.env, apiEnv: manifest.api.env }, runs: {} };
try {
  for (const runtime of runtimes) {
    log('runtime', runtime);
    await startApi();
    const web = await startWeb(runtime);
    current = { runtime, summary: { runtime, started: new Date().toISOString(), scenarios: {}, executorsAtStart: await executorState(runtime) } };
    const save = () => writeFileSync(join(out, `${runtime}.json`), JSON.stringify(current.summary, null, 1));
    try {
      // Warm-up (first renders compile routes); not recorded.
      for (let i = 0; i < 6; i++) await send({ path: searchPath(day(i)), headers: faultCookie(i % 2 ? 'malformed' : null) });
      for (const name of Object.keys(SCENARIOS)) {
        if (!scenarioSet.has(name)) continue;
        log(runtime, 'scenario', name);
        await SCENARIOS[name]();
        save();
      }
      current.summary.executorsAtEnd = await executorState(runtime);
    } catch (error) {
      current.summary.fatal = String(error?.stack ?? error);
      log('FAILED', runtime, error);
      process.exitCode = 1;
    } finally {
      for (const s of Object.values(current.summary.scenarios)) finalizeScenario(s);
      current.summary.finished = new Date().toISOString();
      save();
      all.runs[runtime] = Object.fromEntries(Object.entries(current.summary.scenarios).map(([k, s]) => [k, { n: s.n, matched: s.matched, mismatched: s.mismatched, outcomes: s.outcomes }]));
      await stopWeb(web);
      await stopApi();
    }
  }
} finally {
  all.finished = new Date().toISOString();
  writeFileSync(join(out, 'summary.json'), JSON.stringify(all, null, 1));
  cleanup();
}
