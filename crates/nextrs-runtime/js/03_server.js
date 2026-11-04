// NextRS bootstrap, part 4: Node's IncomingMessage/ServerResponse built from
// Rust-side request data (no sockets exist in JavaScript), and the request
// loop that calls each compiled route's `handler(req, res, ctx)`, the way
// Next's own server does.
"use strict";

((globalThis) => {
  const internal = globalThis.__nextrsInternal;
  const { core, ops, boot, Module, Buffer, stream, path, requestScope } = internal;
  const { Readable, Writable } = stream;

  // op_nextrs_response_write results (response.rs WriteStatus).
  const WRITE_ACCEPTED = 0;
  const WRITE_FULL = 1;
  const WRITE_BUSY = 2;
  // op_nextrs_count kinds (metrics.rs).
  const COUNT_REQUEST_TIMEOUT = 1;
  const COUNT_WAIT_UNTIL_TIMEOUT = 2;
  const COUNT_HANDLER_ERROR = 3;

  // Request body failures from op_nextrs_request_read, as Node reports
  // them: an aborted upload is ECONNRESET "aborted".
  function bodyError(error) {
    const message = String(error?.message ?? error);
    if (message.startsWith("ECONNRESET")) {
      const aborted = new Error("aborted");
      aborted.code = "ECONNRESET";
      return aborted;
    }
    const tooLarge = message.startsWith("ERR_NEXTRS_BODY_TOO_LARGE");
    const failure = new Error(message.replace(/^[A-Z_]+: /, ""));
    failure.code = tooLarge ? "ERR_NEXTRS_BODY_TOO_LARGE" : "ERR_NEXTRS_BODY";
    if (tooLarge) failure.status = failure.statusCode = 413;
    return failure;
  }

  // Node's rules for combining repeated request headers
  // (IncomingMessage#_addHeaderLine).
  const DROP_DUPLICATES = new Set([
    "age", "authorization", "content-length", "content-type", "etag", "expires", "from", "host",
    "if-modified-since", "if-unmodified-since", "last-modified", "location", "max-forwards",
    "proxy-authorization", "referer", "retry-after", "server", "user-agent",
  ]);

  // Ordinary objects, as in Node (getHeaders() is the null-prototype one).
  function headersObject(pairs) {
    const headers = {};
    for (const [rawName, value] of pairs) {
      const name = rawName.toLowerCase();
      if (!(name in headers)) {
        headers[name] = name === "set-cookie" ? [value] : value;
      } else if (name === "set-cookie") {
        headers[name].push(value);
      } else if (!DROP_DUPLICATES.has(name)) {
        headers[name] += (name === "cookie" ? "; " : ", ") + value;
      }
    }
    return headers;
  }

  class IncomingMessage extends Readable {
    #id;
    #body;
    #streamed;
    #reading = false;

    constructor(request) {
      super();
      this.method = request.method;
      this.url = request.url;
      this.httpVersion = "1.1";
      this.httpVersionMajor = 1;
      this.httpVersionMinor = 1;
      this.rawHeaders = request.headers.flat();
      this.headers = headersObject(request.headers);
      this.headersDistinct = {};
      for (const [name, value] of request.headers) (this.headersDistinct[name.toLowerCase()] ??= []).push(value);
      this.trailers = {};
      this.rawTrailers = [];
      this.complete = false;
      this.aborted = false;
      // A description of the connection, not a net.Socket: there is none.
      this.socket = this.connection = Object.freeze({
        encrypted: false,
        remoteAddress: request.remoteAddress ?? undefined,
        remoteFamily: request.remoteAddress?.includes(":") ? "IPv6" : "IPv4",
      });
      this.#id = request.id;
      this.#body = request.body ? Buffer.from(request.body) : null;
      this.#streamed = request.bodyStream;
    }

    _read() {
      if (this.#streamed) {
        this.#pull();
        return;
      }
      if (this.#body) {
        const body = this.#body;
        this.#body = null;
        this.push(body);
      }
      this.complete = true;
      this.push(null);
    }

    // One host read per _read(): the connection is only read as fast as
    // the handler consumes, so an unread body costs no memory here.
    #pull() {
      if (this.#reading) return;
      this.#reading = true;
      ops.op_nextrs_request_read(this.#id).then(
        (chunk) => {
          this.#reading = false;
          if (chunk === null) {
            this.complete = true;
            this.push(null);
          } else {
            this.push(Buffer.from(chunk.buffer, chunk.byteOffset, chunk.byteLength));
          }
        },
        (error) => {
          this.#reading = false;
          const failure = bodyError(error);
          if (failure.code === "ECONNRESET") {
            this.aborted = true;
            this.emit("aborted");
          }
          this.destroy(failure);
        },
      );
    }

    setTimeout(_ms, callback) {
      if (callback) this.once("timeout", callback);
      return this;
    }
  }

  const kHeaders = Symbol("headers");
  const kId = Symbol("request id");

  class ServerResponse extends Writable {
    constructor(req, id) {
      super({ decodeStrings: true });
      this.req = req;
      this[kId] = id;
      this[kHeaders] = new Map();
      this.statusCode = 200;
      this.statusMessage = undefined;
      this.headersSent = false;
      this.sendDate = true;
      this.chunkedEncoding = false;
      this.useChunkedEncodingByDefault = true;
      this.strictContentLength = false;
      this.socket = this.connection = req.socket;
      this.on("finish", () => {
        this.finished = true;
      });
      this.finished = false;
    }

    setHeader(name, value) {
      if (this.headersSent) {
        const error = new Error("Cannot set headers after they are sent to the client");
        error.code = "ERR_HTTP_HEADERS_SENT";
        throw error;
      }
      this[kHeaders].set(String(name).toLowerCase(), { name: String(name), value });
      return this;
    }

    appendHeader(name, value) {
      const existing = this.getHeader(name);
      if (existing === undefined) return this.setHeader(name, value);
      const values = [existing, value].flat();
      return this.setHeader(name, values);
    }

    getHeader(name) {
      return this[kHeaders].get(String(name).toLowerCase())?.value;
    }

    getHeaders() {
      const out = Object.create(null);
      for (const [key, { value }] of this[kHeaders]) out[key] = value;
      return out;
    }

    getHeaderNames() {
      return [...this[kHeaders].keys()];
    }

    getRawHeaderNames() {
      return [...this[kHeaders].values()].map(({ name }) => name);
    }

    hasHeader(name) {
      return this[kHeaders].has(String(name).toLowerCase());
    }

    removeHeader(name) {
      if (this.headersSent) {
        const error = new Error("Cannot remove headers after they are sent to the client");
        error.code = "ERR_HTTP_HEADERS_SENT";
        throw error;
      }
      this[kHeaders].delete(String(name).toLowerCase());
    }

    writeHead(statusCode, reason, headers) {
      if (typeof reason !== "string") {
        headers = reason;
        reason = undefined;
      }
      this.statusCode = statusCode;
      if (reason !== undefined) this.statusMessage = reason;
      if (Array.isArray(headers)) {
        for (let i = 0; i < headers.length; i += 2) this.appendHeader(headers[i], headers[i + 1]);
      } else if (headers) {
        for (const [name, value] of Object.entries(headers)) this.setHeader(name, value);
      }
      this.#sendHead();
      return this;
    }

    flushHeaders() {
      this.#sendHead();
    }

    #sendHead() {
      if (this.headersSent) return;
      this.headersSent = true;
      const pairs = [];
      for (const { name, value } of this[kHeaders].values()) {
        for (const item of Array.isArray(value) ? value : [value]) pairs.push([name, String(item)]);
      }
      if (!ops.op_nextrs_response_head(this[kId], this.statusCode, pairs)) this.#clientGone();
    }

    // Flow control: the write callback is held while the host's buffer
    // for this response is full, so writableLength grows, write() returns
    // false and 'drain' waits for the client, as with a slow socket in
    // Node. A write after the client left is dropped; destroy() emits
    // 'close', which aborts the render.
    _write(chunk, _encoding, callback) {
      this.#sendHead();
      if (chunk.length === 0) {
        callback();
        return;
      }
      this.#offer(chunk, callback);
    }

    #offer(chunk, callback) {
      switch (ops.op_nextrs_response_write(this[kId], chunk)) {
        case WRITE_ACCEPTED:
          callback();
          return;
        case WRITE_FULL:
          this.#afterDrain(null, callback);
          return;
        case WRITE_BUSY:
          this.#afterDrain(chunk, callback);
          return;
        default:
          this.#clientGone();
          callback();
      }
    }

    #afterDrain(retry, callback) {
      ops.op_nextrs_response_drain(this[kId]).then((open) => {
        if (!open) {
          this.#clientGone();
          callback();
        } else if (retry) {
          this.#offer(retry, callback);
        } else {
          callback();
        }
      });
    }

    _final(callback) {
      this.#sendHead();
      ops.op_nextrs_response_end(this[kId]);
      callback();
    }

    #clientGone() {
      if (!this.destroyed) this.destroy();
    }

    // Node's ServerResponse never errors on a destroyed connection; it
    // just stops writing. 'close' tells Next.js to abort rendering.
    _destroy(error, callback) {
      if (!this.writableFinished) ops.op_nextrs_response_error(this[kId], String(error?.message ?? "response destroyed"));
      callback(null);
    }

    setTimeout(_ms, callback) {
      if (callback) this.once("timeout", callback);
      return this;
    }

    // Body-less informational responses are not supported.
    writeContinue() {
      internal.unsupported("http.ServerResponse", "writeContinue");
    }

    writeEarlyHints() {
      internal.unsupported("http.ServerResponse", "writeEarlyHints");
    }
  }

  // ---------------------------------------------------------------------
  // Request loop
  // ---------------------------------------------------------------------
  const distDir = boot.distDir;
  const entries = new Map();
  const rootRequire = Module.createRequire(path.join(boot.projectDir, "nextrs-host.js"));

  function loadEntry(file) {
    if (!entries.has(file)) entries.set(file, rootRequire(file));
    return entries.get(file);
  }

  function errorText(error) {
    return String(error?.stack ?? error);
  }

  // Paths only: query strings can carry tokens.
  function describe(request) {
    return `${request.method} ${request.url.split("?")[0]}`;
  }

  const encoder = new TextEncoder();

  // The request deadline passed. A response that has not started becomes
  // a 504 (sent directly, so the handler's own later writes hit a
  // destroyed stream and are dropped); a started one is aborted. 'close'
  // then tells Next.js to stop rendering.
  function expire(request, req, res) {
    const id = request.id;
    ops.op_nextrs_count(COUNT_REQUEST_TIMEOUT);
    ops.op_nextrs_diagnostic("request-timeout", `${describe(request)} exceeded ${request.timeoutMs} ms`);
    if (!res.headersSent) {
      res.headersSent = true;
      ops.op_nextrs_response_head(id, 504, [
        ["content-type", "text/plain; charset=utf-8"],
        ["cache-control", "no-store"],
        ["x-nextrs-error", "request-timeout"],
      ]);
      ops.op_nextrs_response_write(id, encoder.encode("NextRS: the request exceeded its deadline\n"));
      ops.op_nextrs_response_end(id);
    } else {
      ops.op_nextrs_response_error(id, "request deadline exceeded");
    }
    req.aborted = true;
    req.destroy();
    res.destroy();
  }

  async function serve(request, scope) {
    const id = request.id;
    let timer;
    try {
      const req = new IncomingMessage(request);
      const res = new ServerResponse(req, id);
      for (const [name, value] of request.initialHeaders) res.appendHeader(name, value);

      // Settles when the request stops mattering before it finished: the
      // client left, the request was cancelled, or its deadline passed.
      // Neither the handler nor waitUntil work is awaited past that, so a
      // promise that never settles cannot hold the request's slot.
      let cancel;
      const cancelled = new Promise((resolve) => (cancel = resolve));
      ops.op_nextrs_response_closed(id).then(() => {
        if (res.writableFinished) return;
        if (!res.destroyed) {
          req.aborted = true;
          req.destroy();
          res.destroy();
        }
        cancel("closed");
      });
      if (request.timeoutMs != null) {
        timer = setTimeout(() => {
          if (res.writableFinished) {
            ops.op_nextrs_count(COUNT_WAIT_UNTIL_TIMEOUT);
            ops.op_nextrs_diagnostic("wait-until-timeout", `${describe(request)}: waitUntil work abandoned at the deadline`);
          } else {
            expire(request, req, res);
          }
          cancel("timeout");
        }, request.timeoutMs);
      }

      const pending = [];
      try {
        const mod = loadEntry(request.entry);
        if (typeof mod.handler !== "function") {
          throw new TypeError(`${request.entry} does not export a handler(req, res, ctx) function`);
        }
        await Promise.race([
          mod.handler(req, res, {
            waitUntil: (promise) => pending.push(promise),
            requestMeta: { relativeProjectDir: ".", distDir },
          }),
          cancelled,
        ]);
      } catch (error) {
        ops.op_nextrs_count(COUNT_HANDLER_ERROR);
        ops.op_nextrs_diagnostic("handler-error", `${describe(request)}: ${errorText(error)}`);
        if (!res.headersSent) {
          res.statusCode = 500;
          res.setHeader("content-type", "text/plain; charset=utf-8");
          res.setHeader("x-nextrs-error", "handler-exception");
          res.end("NextRS: the route handler threw an exception\n");
        } else if (!res.writableEnded) {
          // The response already started: abort it, as Node drops the socket.
          ops.op_nextrs_response_error(id, String(error?.message ?? error));
          res.destroy();
        }
      }
      if (pending.length > 0) {
        const settled = await Promise.race([Promise.allSettled(pending), cancelled]);
        if (Array.isArray(settled)) {
          for (const result of settled) {
            if (result.status === "rejected") ops.op_nextrs_diagnostic("wait-until-error", errorText(result.reason));
          }
        }
      }
    } finally {
      clearTimeout(timer);
      // Outbound fetches the request left open end with it.
      if (scope.fetches?.size) {
        const ended = new DOMException("The request that started this fetch has ended", "AbortError");
        for (const abort of [...scope.fetches]) abort(ended);
      }
      ops.op_nextrs_request_done(id);
    }
  }

  async function loop() {
    for (;;) {
      const request = await ops.op_nextrs_next_request();
      if (request === null) return;
      // Concurrent: each request runs in its own async context.
      const scope = { fetches: null };
      requestScope
        .run(scope, () => serve(request, scope))
        .catch((error) => ops.op_nextrs_diagnostic("serve-error", errorText(error)));
    }
  }

  // `next start` loads every app module at startup and calls its
  // patchFetch() (next-server.js), so Next's fetch wrapper is built by the
  // first module in app-paths-manifest order; later calls are no-ops. That
  // choice matters: route handlers and pages are bundled with separate
  // React instances, and the wrapper dedupes a render's fetches with
  // React.cache of the instance that built it. Built from a route handler,
  // it never dedupes page renders. Make the same first call; the other
  // modules still load on demand.
  function patchFetchAsNextStartDoes() {
    const patched = Symbol.for("next-patch");
    let manifest;
    try {
      manifest = rootRequire(path.join(distDir, "server", "app-paths-manifest.json"));
    } catch {
      return;
    }
    for (const page of Object.keys(manifest)) {
      if (globalThis[patched] === true) return;
      try {
        loadEntry(path.join(distDir, "server", "app", `${page}.js`)).patchFetch?.();
      } catch (error) {
        // next start ignores preload failures too.
        ops.op_nextrs_diagnostic("preload-error", `${page}: ${errorText(error)}`);
      }
    }
  }

  globalThis.__nextrsStart = function start() {
    delete globalThis.__nextrsStart;
    delete globalThis.__nextrsInternal;
    delete globalThis.Deno;
    try {
      // Next's official adapter bootstrap: AsyncLocalStorage global,
      // require hooks, crypto polyfills, error handling.
      if (boot.bootstrap) rootRequire(boot.bootstrap);
    } catch (error) {
      ops.op_nextrs_ready(errorText(error));
      return;
    }
    patchFetchAsNextStartDoes();
    if (boot.faultInjection) {
      Object.defineProperty(globalThis, "__nextrsFault", { value: (kind) => ops.op_nextrs_fault(kind) });
    }
    ops.op_nextrs_ready(null);
    loop().catch((error) => core.reportUnhandledException(error));
  };
})(globalThis);
