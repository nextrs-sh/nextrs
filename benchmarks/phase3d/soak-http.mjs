// Phase 3D: the HTTP client shared by the soak drivers.
//
// Phase 2G and 3C soaks exited 1 on ECONNRESETs from a keep-alive race. The
// server (hyper's header_read_timeout, 30 s) closes a connection that has
// been idle for 30 s. The drivers' 30-second timers could reuse such a
// connection at that instant. This client removes the race at its source
// and records enough to tell a race from a server fault:
//
// 1. Idle retirement. A pooled socket idle for IDLE_RETIRE_MS (15 s, half
//    the server's limit) is destroyed by the agent, so no request is ever
//    written to a connection the server may be closing.
// 2. Bounded retry, idempotent requests only. A GET/HEAD that fails with
//    ECONNRESET or EPIPE on a *reused* socket before any response byte
//    arrived is sent once more on a fresh connection. Each retry is counted
//    and kept with its diagnostics; the drivers report them. Nothing else
//    is retried.
// 3. No replay of mutations. A POST (or any other method) is never retried,
//    whatever the error. A transport failure on one is returned as
//    ambiguous: the request may or may not have been processed. The drivers
//    reconcile mutations against an external ledger.
// 4. Diagnostics. Every result carries the connection state: whether the
//    socket was reused, how long it had been idle, its age, how many
//    requests it had carried, and whether a response had started.
import http from "node:http";

export const IDLE_RETIRE_MS = 15_000;
const RETRYABLE = new Set(["ECONNRESET", "EPIPE"]);

export function createAgent({ maxSockets, idleRetireMs = IDLE_RETIRE_MS } = {}) {
  // `timeout` is applied to free sockets by Agent#keepSocketAlive; Node's
  // agent destroys a *free* socket when it times out. A request in flight
  // sets its own timeout on the socket (send() below), which replaces it.
  const agent = new http.Agent({ keepAlive: true, maxSockets, timeout: idleRetireMs, scheduling: "lifo" });
  const keep = agent.keepSocketAlive.bind(agent);
  agent.keepSocketAlive = (socket) => {
    socket.__freedAt = Date.now();
    return keep(socket);
  };
  agent.on("free", (socket) => {
    socket.__requests = (socket.__requests ?? 0) + 1;
  });
  agent.idleRetireMs = idleRetireMs;
  return agent;
}

function once({ host, port, method, path, headers, body, agent, timeoutMs, abortAfterMs }) {
  return new Promise((resolve) => {
    const t0 = process.hrtime.bigint();
    const ms = () => Number(process.hrtime.bigint() - t0) / 1e6;
    const conn = { reused: false, idleMs: null, ageMs: null, socketRequests: 0, responseStarted: false, bytes: 0 };
    let done = false;
    const finish = (r) => {
      if (!done) {
        done = true;
        resolve({ ms: ms(), conn, ...r });
      }
    };
    const req = http.request({ host, port, method, path, headers, agent: abortAfterMs ? false : agent }, (res) => {
      conn.responseStarted = true;
      const chunks = [];
      res.on("data", (c) => {
        conn.bytes += c.length;
        chunks.push(c);
      });
      res.on("end", () => finish({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      res.on("error", (e) => finish({ status: 0, error: "body", detail: e.code ?? e.message, headers: res.headers, body: Buffer.concat(chunks) }));
      res.on("aborted", () => finish({ status: 0, error: "aborted", headers: res.headers, body: Buffer.concat(chunks) }));
    });
    req.on("socket", (s) => {
      const now = Date.now();
      s.__createdAt ??= now;
      conn.reused = req.reusedSocket === true;
      conn.idleMs = conn.reused && s.__freedAt ? now - s.__freedAt : 0;
      conn.ageMs = now - s.__createdAt;
      conn.socketRequests = s.__requests ?? 0;
    });
    req.setTimeout(timeoutMs, () => {
      req.destroy(new Error("timeout"));
      finish({ status: 0, error: "timeout" });
    });
    req.on("error", (e) => finish({ status: 0, error: e.code ?? e.message }));
    if (abortAfterMs)
      setTimeout(() => {
        req.destroy(new Error("aborted by client"));
        finish({ status: 0, error: "client-abort" });
      }, abortAfterMs);
    req.end(body);
  });
}

// One request, with the retry policy above. `retries` (optional array)
// receives a record for every retry made.
export async function send(opts) {
  const o = { method: "GET", headers: {}, timeoutMs: 30_000, ...opts };
  const first = await once(o);
  const idempotent = (o.method === "GET" || o.method === "HEAD") && !o.abortAfterMs;
  if (
    first.status === 0 &&
    RETRYABLE.has(first.error) &&
    first.conn.reused &&
    !first.conn.responseStarted
  ) {
    if (!idempotent) return { ...first, ambiguous: true };
    // A new connection for the retry: no agent, not kept.
    const second = await once({ ...o, agent: false });
    o.retries?.push({ at: new Date().toISOString(), method: o.method, path: o.path, first: { error: first.error, ms: +first.ms.toFixed(1), conn: first.conn }, retry: { status: second.status, error: second.error, ms: +second.ms.toFixed(1) } });
    return { ...second, retried: true, firstAttempt: { error: first.error, conn: first.conn } };
  }
  if (first.status === 0 && !idempotent && !o.abortAfterMs && first.error !== "timeout") return { ...first, ambiguous: true };
  return first;
}

// Bounded, redacted capture of unexpected responses: JSON lines, one per
// response, with the full body up to MAX_BODY bytes. Cookie values are
// replaced (they are synthetic session tokens, but nothing secret-shaped
// is written to evidence).
const MAX_BODY = 256 * 1024;
const REDACT = new Set(["set-cookie", "cookie", "authorization"]);
export function redactHeaders(h = {}) {
  return Object.fromEntries(Object.entries(h).map(([k, v]) => [k, REDACT.has(k.toLowerCase()) ? "[redacted]" : v]));
}
export function createCapture(file, { limit = 200 } = {}) {
  let n = 0;
  return {
    get count() {
      return n;
    },
    async write(entry, res, req) {
      n++;
      if (n > limit) return;
      const { appendFileSync } = await import("node:fs");
      const body = res?.body ?? Buffer.alloc(0);
      appendFileSync(
        file,
        JSON.stringify({
          at: new Date().toISOString(),
          ...entry,
          request: req && { method: req.method ?? "GET", path: req.path, headers: redactHeaders(req.headers) },
          response: res && {
            status: res.status,
            error: res.error,
            detail: res.detail,
            ambiguous: res.ambiguous ?? false,
            retried: res.retried ?? false,
            firstAttempt: res.firstAttempt,
            ms: res.ms && +res.ms.toFixed(1),
            conn: res.conn,
            headers: redactHeaders(res.headers),
            bodyBytes: body.length,
            bodyTruncated: body.length > MAX_BODY,
            body: body.subarray(0, MAX_BODY).toString("utf8"),
          },
        }) + "\n",
      );
    },
  };
}
