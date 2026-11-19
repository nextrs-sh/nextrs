// Tests for the soak HTTP client's connection policy (soak-http.mjs):
//
//   node --test benchmarks/phase3d/soak-http.test.mjs
//
// Local servers only. Each case reproduces one part of the keep-alive race
// deterministically: a server that resets a reused connection, as one
// closing at its idle limit does.
import { test } from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import { createAgent, send } from "./soak-http.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// A server whose behaviour for the Nth request on a connection is chosen
// by `onRequest(req, res, { connRequests, method })`.
async function server(onRequest, options = {}) {
  const seen = [];
  const s = http.createServer(options, (req, res) => {
    req.socket.__n = (req.socket.__n ?? 0) + 1;
    seen.push({ method: req.method, url: req.url, connRequests: req.socket.__n, port: req.socket.remotePort });
    req.resume();
    req.on("end", () => onRequest(req, res, { connRequests: req.socket.__n }));
  });
  await new Promise((r) => s.listen(0, "127.0.0.1", r));
  return { port: s.address().port, seen, close: () => new Promise((r) => { s.closeAllConnections(); s.close(r); }) };
}

const reset = (req) => req.socket.resetAndDestroy();
const ok = (res) => { res.setHeader("content-type", "text/plain"); res.end("ok"); };

test("an idle pooled socket is retired before the server's idle limit", async () => {
  // The server closes idle connections after 600 ms; the client retires them after 300.
  const srv = await server((req, res) => ok(res), { keepAliveTimeout: 600 });
  const agent = createAgent({ maxSockets: 4, idleRetireMs: 300 });
  const a = await send({ host: "127.0.0.1", port: srv.port, path: "/a", agent });
  assert.equal(a.status, 200);
  await sleep(450); // past the client's limit, before the server's
  const b = await send({ host: "127.0.0.1", port: srv.port, path: "/b", agent });
  assert.equal(b.status, 200);
  assert.equal(b.conn.reused, false, "the idle socket must have been retired, not reused");
  assert.notEqual(srv.seen[0].port, srv.seen[1].port);
  agent.destroy();
  await srv.close();
});

test("a socket used within the idle limit is reused, with its idle time recorded", async () => {
  const srv = await server((req, res) => ok(res), { keepAliveTimeout: 5000 });
  const agent = createAgent({ maxSockets: 4, idleRetireMs: 2000 });
  await send({ host: "127.0.0.1", port: srv.port, path: "/a", agent });
  await sleep(100);
  const b = await send({ host: "127.0.0.1", port: srv.port, path: "/b", agent });
  assert.equal(b.conn.reused, true);
  assert.ok(b.conn.idleMs >= 90 && b.conn.idleMs < 2000, `idleMs ${b.conn.idleMs}`);
  assert.equal(b.conn.socketRequests, 1);
  agent.destroy();
  await srv.close();
});

test("a GET reset on a reused socket before any response byte is retried once, on a new connection", async () => {
  // Second request on any connection: reset, as a server closing at its idle limit.
  const srv = await server((req, res, { connRequests }) => (connRequests === 2 ? reset(req) : ok(res)), { keepAliveTimeout: 5000 });
  const agent = createAgent({ maxSockets: 4, idleRetireMs: 2000 });
  const retries = [];
  await send({ host: "127.0.0.1", port: srv.port, path: "/a", agent, retries });
  const b = await send({ host: "127.0.0.1", port: srv.port, path: "/b", agent, retries });
  assert.equal(b.status, 200);
  assert.equal(b.retried, true);
  assert.equal(b.firstAttempt.error, "ECONNRESET");
  assert.equal(b.firstAttempt.conn.reused, true);
  assert.equal(retries.length, 1);
  assert.equal(srv.seen.filter((s) => s.url === "/b").length, 2);
  agent.destroy();
  await srv.close();
});

test("a GET reset on a new connection is not retried (not the idle race)", async () => {
  const srv = await server((req) => reset(req));
  const agent = createAgent({ maxSockets: 4 });
  const retries = [];
  const r = await send({ host: "127.0.0.1", port: srv.port, path: "/a", agent, retries });
  assert.equal(r.status, 0);
  assert.equal(r.error, "ECONNRESET");
  assert.equal(r.retried, undefined);
  assert.equal(retries.length, 0);
  assert.equal(srv.seen.length, 1);
  agent.destroy();
  await srv.close();
});

test("a GET whose response had started is not retried", async () => {
  const srv = await server((req, res, { connRequests }) => {
    if (connRequests === 1) return ok(res);
    res.writeHead(200, { "content-length": "100" });
    res.write("partial");
    setTimeout(() => reset(req), 20);
  }, { keepAliveTimeout: 5000 });
  const agent = createAgent({ maxSockets: 4, idleRetireMs: 2000 });
  const retries = [];
  await send({ host: "127.0.0.1", port: srv.port, path: "/a", agent, retries });
  const b = await send({ host: "127.0.0.1", port: srv.port, path: "/b", agent, retries });
  assert.equal(b.status, 0);
  assert.equal(b.conn.responseStarted, true);
  assert.equal(retries.length, 0);
  assert.equal(srv.seen.filter((s) => s.url === "/b").length, 1);
  agent.destroy();
  await srv.close();
});

test("a POST is never retried: a reset on a reused socket is ambiguous, and the server saw it once", async () => {
  const srv = await server((req, res, { connRequests }) => (connRequests === 2 ? reset(req) : ok(res)), { keepAliveTimeout: 5000 });
  const agent = createAgent({ maxSockets: 4, idleRetireMs: 2000 });
  const retries = [];
  await send({ host: "127.0.0.1", port: srv.port, path: "/warm", agent, retries });
  const r = await send({ host: "127.0.0.1", port: srv.port, method: "POST", path: "/mutate", body: "x", agent, retries });
  assert.equal(r.status, 0);
  assert.equal(r.ambiguous, true);
  assert.equal(r.retried, undefined);
  assert.equal(retries.length, 0);
  assert.equal(srv.seen.filter((s) => s.url === "/mutate").length, 1, "the mutation reached the server exactly once");
  agent.destroy();
  await srv.close();
});

test("a POST reset on a new connection is also ambiguous and not retried", async () => {
  const srv = await server((req) => reset(req));
  const r = await send({ host: "127.0.0.1", port: srv.port, method: "POST", path: "/mutate", body: "x", agent: createAgent({ maxSockets: 2 }) });
  assert.equal(r.status, 0);
  assert.equal(r.ambiguous, true);
  assert.equal(srv.seen.length, 1);
  await srv.close();
});

test("a deliberate client abort is neither retried nor ambiguous", async () => {
  const srv = await server((req, res) => setTimeout(() => ok(res), 200));
  const r = await send({ host: "127.0.0.1", port: srv.port, path: "/slow", abortAfterMs: 20, agent: createAgent({ maxSockets: 2 }) });
  assert.equal(r.status, 0);
  assert.equal(r.error, "client-abort");
  assert.equal(r.ambiguous, undefined);
  await srv.close();
});
