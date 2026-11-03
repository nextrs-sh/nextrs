// NextRS bootstrap, part 4: Node's IncomingMessage/ServerResponse built from
// Rust-side request data (no sockets exist in JavaScript), and the request
// loop that calls each compiled route's `handler(req, res, ctx)`, the way
// Next's own server does.
"use strict";

((globalThis) => {
  const internal = globalThis.__nextrsInternal;
  const { core, ops, boot, Module, Buffer, stream, path } = internal;
  const { Readable, Writable } = stream;

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
    #body;

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
      this.#body = request.body ? Buffer.from(request.body) : null;
    }

    _read() {
      if (this.#body) {
        const body = this.#body;
        this.#body = null;
        this.push(body);
      }
      this.complete = true;
      this.push(null);
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

    _write(chunk, _encoding, callback) {
      this.#sendHead();
      // A write after the client left is dropped, as with a closed socket;
      // destroy() emits 'close', which aborts the render.
      if (chunk.length > 0 && !ops.op_nextrs_response_write(this[kId], chunk)) this.#clientGone();
      callback();
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

  async function serve(request) {
    const req = new IncomingMessage(request);
    const res = new ServerResponse(req, request.id);
    ops.op_nextrs_response_closed(request.id).then(() => {
      if (!res.writableFinished && !res.destroyed) {
        req.aborted = true;
        req.destroy();
        res.destroy();
      }
    });
    const pending = [];
    try {
      const mod = loadEntry(request.entry);
      if (typeof mod.handler !== "function") {
        throw new TypeError(`${request.entry} does not export a handler(req, res, ctx) function`);
      }
      await mod.handler(req, res, {
        waitUntil: (promise) => pending.push(promise),
        requestMeta: { relativeProjectDir: ".", distDir },
      });
    } catch (error) {
      ops.op_nextrs_diagnostic("handler-error", `${request.method} ${request.url}: ${errorText(error)}`);
      if (!res.headersSent) {
        res.statusCode = 500;
        res.setHeader("content-type", "text/plain; charset=utf-8");
        res.setHeader("x-nextrs-error", "handler-exception");
        res.end("NextRS: the route handler threw an exception\n");
      } else if (!res.writableEnded) {
        // The response already started: abort it, as Node drops the socket.
        ops.op_nextrs_response_error(request.id, String(error?.message ?? error));
        res.destroy();
      }
    }
    const settled = await Promise.allSettled(pending);
    for (const result of settled) {
      if (result.status === "rejected") ops.op_nextrs_diagnostic("wait-until-error", errorText(result.reason));
    }
  }

  async function loop() {
    for (;;) {
      const request = await ops.op_nextrs_next_request();
      if (request === null) return;
      // Concurrent: each request runs in its own async context.
      serve(request);
    }
  }

  globalThis.__nextrsStart = function start() {
    delete globalThis.__nextrsStart;
    delete globalThis.__nextrsInternal;
    delete globalThis.Deno;
    try {
      // Next's official adapter bootstrap: AsyncLocalStorage global,
      // require hooks, crypto polyfills, error handling.
      rootRequire(path.join(boot.projectDir, "node_modules/next/dist/build/adapter/setup-node-env.external.js"));
    } catch (error) {
      ops.op_nextrs_ready(errorText(error));
      return;
    }
    ops.op_nextrs_ready(null);
    loop().catch((error) => core.reportUnhandledException(error));
  };
})(globalThis);
