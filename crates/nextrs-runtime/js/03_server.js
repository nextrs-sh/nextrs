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

  // Next's NoFallbackError (next/dist/shared/lib/no-fallback-error.external),
  // thrown by a page handler for params it has no prerender or fallback for.
  const isNoFallbackError = (error) =>
    error instanceof Error && error.constructor?.name === "NoFallbackError" && error.message === "Internal: NoFallbackError";

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

  // How long a fetch may outlive its request when the request had no
  // deadline.
  const DETACHED_FETCH_GRACE_MS = 30_000;

  async function serve(request, scope) {
    const id = request.id;
    let timer;
    // Set when the request was cancelled: its handler may still be running
    // (it is no longer awaited), so its fetches are detached.
    let cancelledBy = null;
    try {
      const req = new IncomingMessage(request);
      const res = new ServerResponse(req, id);
      for (const [name, value] of request.initialHeaders) res.appendHeader(name, value);
      if (request.initialStatus != null) res.statusCode = request.initialStatus;

      // Settles when the request stops mattering before it finished: the
      // client left, the request was cancelled, or its deadline passed.
      // Neither the handler nor waitUntil work is awaited past that, so a
      // promise that never settles cannot hold the request's slot.
      let cancel;
      const cancelled = new Promise((resolve) => {
        cancel = (reason) => {
          cancelledBy ??= reason;
          resolve(reason);
        };
      });
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
      const invocation = request.invocation ?? { kind: "handler" };
      try {
        if (invocation.kind === "proxy") {
          await Promise.race([runProxy(request, invocation, req, res, pending), cancelled]);
        } else {
          const mod = loadEntry(request.entry);
          if (typeof mod.handler !== "function") {
            throw new TypeError(`${request.entry} does not export a handler(req, res, ctx) function`);
          }
          const requestMeta = { relativeProjectDir: ".", distDir };
          // next-server's attachRequestMeta: Next builds a route handler's
          // NextRequest URL (host and port) from initURL.
          if (request.initUrl) {
            requestMeta.initURL = request.initUrl;
            requestMeta.initProtocol = request.initUrl.startsWith("https:") ? "https" : "http";
          }
          if (invocation.kind === "routed") Object.assign(requestMeta, routedMeta(invocation));
          await Promise.race([
            mod.handler(req, res, {
              waitUntil: (promise) => pending.push(promise),
              requestMeta,
            }),
            cancelled,
          ]);
        }
      } catch (error) {
        if (isNoFallbackError(error) && invocation.kind !== "proxy" && !res.headersSent) {
          // The page has no entry for these params (dynamicParams = false,
          // or a fallback-less route): router-server routes the request on
          // without this output, which Rust does when it sees the marker.
          res.statusCode = 404;
          res.setHeader("x-nextrs-error", "no-fallback");
          res.end();
        } else {
          ops.op_nextrs_count(COUNT_HANDLER_ERROR);
          ops.op_nextrs_diagnostic("handler-error", `${describe(request)}: ${errorText(error)}`);
          if (!res.headersSent) {
            // next start answers an exception that escapes the handler
            // (an MPA Server Action error, a CSRF rejection) this way; a
            // proxy exception too (Rust turns the marker into next start's
            // response and never continues to the route).
            res.statusCode = 500;
            res.setHeader("content-type", "text/plain");
            res.setHeader("x-nextrs-error", invocation.kind === "proxy" ? "proxy-exception" : "handler-exception");
            res.end("Internal Server Error");
          } else if (!res.writableEnded) {
            // The response already started: abort it, as Node drops the socket.
            ops.op_nextrs_response_error(id, String(error?.message ?? error));
            res.destroy();
          }
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
      releaseAllLeases(scope);
      // Outbound fetches still open (see 01_fetch.js): after a handler that
      // finished, unread response bodies are released now; everything else
      // (and everything of a cancelled request, whose handler may still be
      // about to read) is detached and ends on its own, or after a grace as
      // long as the request's deadline.
      if (scope.fetches?.size) {
        const open = [...scope.fetches];
        scope.fetches = null;
        const ended = (message) => new DOMException(message, "AbortError");
        const detached = [];
        for (const entry of open) {
          if (cancelledBy === null && entry.unread()) entry.abort(ended("The request that fetched this response ended without reading its body"));
          else detached.push(entry);
        }
        if (detached.length) {
          setTimeout(() => {
            for (const entry of detached) entry.abort(ended("The request that started this fetch ended too long ago"));
          }, request.timeoutMs ?? DETACHED_FETCH_GRACE_MS);
        }
      }
      ops.op_nextrs_request_done(id);
    }
  }

  async function loop() {
    for (;;) {
      const request = await ops.op_nextrs_next_request();
      if (request === null) return;
      // Concurrent: each request runs in its own async context.
      // selfOrigin: the origin of the request's initURL, which Next fetches
      // itself at (forwarded Server Actions, action redirects).
      const scope = { fetches: null, leases: null, selfOrigin: request.initUrl ? new URL(request.initUrl).origin : null };
      requestScope
        .run(scope, () => serve(request, scope))
        .catch((error) => ops.op_nextrs_diagnostic("serve-error", errorText(error)));
    }
  }

  // ---------------------------------------------------------------------
  // Proxy (proxy.ts / middleware.ts) and routed requests
  // ---------------------------------------------------------------------

  // What next start's NextNodeServer passes the proxy as nextConfig.
  let proxyConfig;
  function proxyNextConfig() {
    if (proxyConfig === undefined) {
      let config = {};
      try {
        config = rootRequire(path.join(distDir, "required-server-files.json")).config ?? {};
      } catch {
        // A build without it has no config to pass.
      }
      proxyConfig = {
        basePath: config.basePath ?? "",
        i18n: config.i18n ?? null,
        trailingSlash: Boolean(config.trailingSlash),
        experimental: config.experimental ?? {},
      };
    }
    return proxyConfig;
  }

  const nextServerModule = (relative) => {
    try {
      return rootRequire(`next/dist/${relative}.js`);
    } catch {
      return null;
    }
  };

  // Next's toNodeOutgoingHttpHeaders (server/web/utils.js): lowercase
  // names, set-cookie split into an array.
  let toNodeHeaders;
  function nodeOutgoingHeaders(headers) {
    toNodeHeaders ??= nextServerModule("server/web/utils")?.toNodeOutgoingHttpHeaders ?? null;
    if (toNodeHeaders) return toNodeHeaders(headers);
    const out = {};
    for (const [key, value] of headers.entries()) out[key] = value;
    const cookies = headers.getSetCookie?.() ?? [];
    if (cookies.length) out["set-cookie"] = cookies.length === 1 ? cookies[0] : cookies;
    return out;
  }

  // The compiled proxy (server/middleware.js), called as next start's
  // NextNodeServer.runMiddleware calls it: its default export is Next's
  // adapter, which builds the NextRequest, runs the user's proxy in its
  // request scope and returns the NextResponse. The response's status and
  // headers go back to Rust, which applies them as resolve-routes does; a
  // direct response streams its body.
  async function runProxy(request, invocation, req, res, pending) {
    const mod = loadEntry(request.entry);
    const adapterFn = mod.default || mod;
    if (typeof adapterFn !== "function") {
      throw new TypeError(`${request.entry} does not export the proxy adapter`);
    }
    const method = String(request.method || "GET").toUpperCase();
    const hasBody = method !== "GET" && method !== "HEAD" && (request.body != null || request.bodyStream);
    // signalFromNodeResponse: aborted when the response closes unfinished.
    const controller = new AbortController();
    res.once("close", () => {
      if (!res.writableFinished) controller.abort();
    });
    const result = await adapterFn({
      handler: mod.proxy || mod.middleware || mod,
      request: {
        headers: req.headers,
        method,
        nextConfig: proxyNextConfig(),
        url: invocation.url,
        page: {},
        // next start passes a clone of the body stream; the route gets
        // its own copy (Rust buffered the body for both).
        body: hasBody ? req : undefined,
        signal: controller.signal,
        waitUntil: (promise) => pending.push(promise),
      },
      page: "middleware",
    });
    if (result?.waitUntil) pending.push(Promise.resolve(result.waitUntil));
    const response = result?.response;
    if (!response) {
      // runMiddleware renders a 404 when the adapter returns nothing.
      res.statusCode = 404;
      res.setHeader("x-nextrs-error", "proxy-no-response");
      res.end();
      return;
    }
    res.statusCode = response.status;
    const headers = nodeOutgoingHeaders(response.headers);
    for (const [name, value] of Object.entries(headers)) {
      if (value !== undefined) res.setHeader(name, value);
    }
    if (!response.body) {
      res.end();
      return;
    }
    const reader = response.body.getReader();
    res.once("close", () => {
      if (!res.writableFinished) reader.cancel().catch(() => {});
    });
    for (;;) {
      const { done, value } = await reader.read();
      if (done || res.destroyed) break;
      const chunk = typeof value === "string" ? Buffer.from(value) : Buffer.from(value.buffer, value.byteOffset, value.byteLength);
      if (!res.write(chunk)) await new Promise((resolve) => res.once("drain", resolve));
    }
    if (!res.destroyed) res.end();
  }

  // Request meta next start's base server gives a handler after routing:
  // the cookies the proxy set (patchSetHeaderWithCookieSupport merges
  // them with the route's own), and for a rewritten request the params
  // and query of the route it was rewritten to. The params are taken from
  // the rewritten pathname with Next's own route matcher.
  const routeMatchers = new Map();
  function paramsFor(page, pathname) {
    let matcher = routeMatchers.get(page);
    if (matcher === undefined) {
      const regexModule = nextServerModule("shared/lib/router/utils/route-regex");
      const matcherModule = nextServerModule("shared/lib/router/utils/route-matcher");
      matcher = regexModule && matcherModule ? matcherModule.getRouteMatcher(regexModule.getRouteRegex(page)) : null;
      routeMatchers.set(page, matcher);
    }
    if (!matcher) throw new Error("Next's route matcher (next/dist/shared/lib/router/utils) is not available");
    return matcher(pathname) || undefined;
  }

  function routedMeta(invocation) {
    const meta = {};
    if (invocation.middlewareCookies?.length) meta.middlewareCookie = invocation.middlewareCookies;
    if (invocation.page != null) {
      const params = paramsFor(invocation.page, invocation.invokePath);
      if (params) meta.params = params;
      meta.query = JSON.parse(invocation.query ?? "{}");
      meta.rewrittenPathname = invocation.invokePath;
    }
    return meta;
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

  // The cache shared by every isolate (src/cache.rs), offered to Next.js
  // as hosting platforms do: as the FetchCache handler on
  // globalThis[Symbol.for("@next/cache-handlers")]. IncrementalCache uses
  // it unless next.config sets its own `cacheHandler`, which wins. It
  // implements Next's documented CacheHandler interface and behaves as
  // Next's own FileSystemCache does, with one store for every isolate:
  //
  // * FETCH entries (the fetch data cache and unstable_cache), and APP_PAGE
  //   and APP_ROUTE entries (ISR: regenerated pages and route handler
  //   output) are stored; PAGES and IMAGE entries are not.
  // * Tags: Rust records revalidations for the whole pool. Before judging
  //   an entry, each isolate mirrors them into Next's own tags manifest
  //   (the `tags-manifest.external` module every Next bundle shares), so
  //   Next's areTagsExpired/areTagsStale decide, in this handler as in
  //   IncrementalCache, exactly as under next start.
  // * A response entry the store has never held is read from the build's
  //   prerendered files with Next's own FileSystemCache (no writes, no
  //   memory cache), then kept in the store with its original time, as
  //   next start promotes such "seeds".
  const NEXT_CACHE_TAGS_HEADER = "x-next-cache-tags";
  const RESPONSE_KINDS = new Set(["APP_PAGE", "APP_ROUTE"]);
  const ENVELOPE = 1;
  const notStored = new Set();
  const nextModule = (name) => {
    try {
      return rootRequire(`next/dist/server/lib/incremental-cache/${name}.js`);
    } catch {
      return null;
    }
  };

  // Next's areTagsExpired/areTagsStale (tags-manifest.external.js), used
  // only when Next's module cannot be loaded (handler tests without Next).
  function localTags() {
    const tagsManifest = new Map();
    const now = () => performance.timeOrigin + performance.now();
    return {
      tagsManifest,
      areTagsExpired(tags, timestamp) {
        for (const tag of tags) {
          const expiredAt = tagsManifest.get(tag)?.expired;
          if (typeof expiredAt === "number" && expiredAt <= now() && expiredAt > timestamp) return true;
        }
        return false;
      },
      areTagsStale(tags, timestamp) {
        for (const tag of tags) {
          const staleAt = tagsManifest.get(tag)?.stale ?? 0;
          if (typeof staleAt === "number" && staleAt > timestamp) return true;
        }
        return false;
      },
    };
  }

  let tagsModule;
  let tagVersion = 0;
  // Brings this isolate's copy of Next's tags manifest up to date with the
  // store's (a version check when nothing changed).
  function syncTags() {
    if (tagsModule === undefined) {
      const loaded = nextModule("tags-manifest.external");
      tagsModule = loaded?.tagsManifest instanceof Map ? loaded : localTags();
    }
    const sync = ops.op_nextrs_cache_tag_sync(tagVersion);
    if (sync.version !== tagVersion) {
      const manifest = tagsModule.tagsManifest;
      if (sync.full) manifest.clear();
      for (const [tag, stale, expired] of sync.tags) {
        if (stale == null && expired == null) {
          manifest.delete(tag);
          continue;
        }
        const entry = {};
        if (stale != null) entry.stale = stale;
        if (expired != null) entry.expired = expired;
        manifest.set(tag, entry);
      }
      tagVersion = sync.version;
    }
    return tagsModule;
  }

  const b64 = (bytes) => (bytes == null ? undefined : Buffer.from(bytes).toString("base64"));
  const unb64 = (text) => (text == null ? undefined : Buffer.from(text, "base64"));

  // Response values hold Buffers (RSC payloads, segments, route bodies);
  // they are stored as base64 inside JSON.
  function encodeResponse(value) {
    if (value == null) return null;
    if (value.kind === "APP_PAGE") {
      return {
        kind: value.kind,
        html: typeof value.html === "string" ? value.html : String(value.html ?? ""),
        rscData: b64(value.rscData),
        postponed: value.postponed,
        headers: value.headers,
        status: value.status,
        segmentData: value.segmentData ? [...value.segmentData].map(([path, data]) => [path, b64(data)]) : undefined,
      };
    }
    return { kind: value.kind, body: b64(value.body), headers: value.headers, status: value.status };
  }

  function decodeResponse(stored) {
    if (stored == null) return null;
    if (stored.kind === "APP_PAGE") {
      return {
        kind: stored.kind,
        html: stored.html,
        rscData: unb64(stored.rscData),
        postponed: stored.postponed,
        headers: stored.headers,
        status: stored.status,
        segmentData: stored.segmentData ? new Map(stored.segmentData.map(([path, data]) => [path, unb64(data)])) : undefined,
      };
    }
    return { kind: stored.kind, body: unb64(stored.body) ?? Buffer.alloc(0), headers: stored.headers, status: stored.status };
  }

  function responseTags(value) {
    const header = value?.headers?.[NEXT_CACHE_TAGS_HEADER];
    return typeof header === "string" ? header.split(",") : [];
  }

  async function storeResponse(key, value, cacheControl, options = {}) {
    const envelope = JSON.stringify({ v: ENVELOPE, value: encodeResponse(value), cacheControl });
    const tags = responseTags(value).filter(Boolean);
    await ops.op_nextrs_cache_set(key, envelope, tags, true, options.lastModified ?? -1, options.ifAbsent ?? false);
  }

  // The build's own files only. A server that writes into the build (next
  // start keeps regenerated pages under server/route-cache) must not feed
  // this process: such files are reported missing, so FileSystemCache
  // falls back to the immutable build prerender, verified by its
  // route-cache ownership record.
  function buildOnlyFs(fs, serverDistDir) {
    const scoped = path.join(serverDistDir, "route-cache") + path.sep;
    const hidden = (file) => String(file).startsWith(scoped);
    const missing = (file) => {
      const error = new Error(`ENOENT: no such file or directory, open '${file}'`);
      error.code = "ENOENT";
      return error;
    };
    return {
      existsSync: (file) => !hidden(file) && fs.existsSync(file),
      readFile: (file, ...rest) => (hidden(file) ? Promise.reject(missing(file)) : fs.readFile(file, ...rest)),
      readFileSync: (file, ...rest) => {
        if (hidden(file)) throw missing(file);
        return fs.readFileSync(file, ...rest);
      },
      stat: (file) => (hidden(file) ? Promise.reject(missing(file)) : fs.stat(file)),
    };
  }

  // Regeneration leases this request holds (see get()): released when it
  // stores the entry, or when the request ends whatever happened.
  const LEASE_MS = 10_000;
  function holdLease(key, id) {
    const scope = requestScope.getStore();
    if (scope) (scope.leases ??= new Map()).set(key, id);
  }
  function releaseLease(key) {
    const leases = requestScope.getStore()?.leases;
    const id = leases?.get(key);
    if (id !== undefined) {
      leases.delete(key);
      ops.op_nextrs_cache_release(key, id);
    }
  }
  function releaseAllLeases(scope) {
    if (!scope.leases) return;
    for (const [key, id] of scope.leases) ops.op_nextrs_cache_release(key, id);
    scope.leases = null;
  }

  let SeedCache;
  class NextRSCacheHandler {
    #ctx;
    #seed;

    constructor(ctx) {
      this.#ctx = ctx ?? {};
    }

    // The build's prerender for a response entry, read by Next's own
    // FileSystemCache (which also checks its route-cache ownership).
    async #readSeed(key, ctx) {
      const { fs, serverDistDir } = this.#ctx;
      if (!fs || !serverDistDir) return null;
      if (SeedCache === undefined) SeedCache = nextModule("file-system-cache")?.default ?? null;
      if (!SeedCache) return null;
      this.#seed ??= new SeedCache({ fs: buildOnlyFs(fs, serverDistDir), serverDistDir, flushToDisk: false, maxMemoryCacheSize: 0, revalidatedTags: [] });
      const data = await this.#seed.get(key, ctx);
      if (data?.value && RESPONSE_KINDS.has(data.value.kind)) {
        // Keep it, with the build's time, unless something newer was stored.
        await storeResponse(key, data.value, data.cacheControl, { lastModified: data.lastModified, ifAbsent: true });
      }
      return data;
    }

    async get(key, context) {
      const kind = context?.kind;
      if (kind === "FETCH") return this.#fetchEntry(String(key), context);
      if (!RESPONSE_KINDS.has(kind)) return null;
      key = String(key);
      const found = await this.#responseEntry(key, context);
      if (found !== null) return found;
      // Missing or expired: the caller regenerates before it answers. If
      // another isolate already is, wait for its result instead (Next's
      // batcher does the same between requests of one isolate).
      const lease = await ops.op_nextrs_cache_lease(key, LEASE_MS);
      if (lease.acquired) {
        holdLease(key, lease.id);
        return null;
      }
      return this.#responseEntry(key, context);
    }

    async #fetchEntry(key, context) {
      const hit = await ops.op_nextrs_cache_get(key);
      const tags = syncTags();
      if (hit === null || hit.value === null) return null;
      let stored;
      try {
        stored = JSON.parse(hit.value);
      } catch {
        return null;
      }
      if (stored?.kind !== "FETCH") return null;
      // FileSystemCache: a tag expired after the entry was written is a
      // miss. IncrementalCache then applies stale tags itself.
      const combined = [...(context.tags ?? []), ...(context.softTags ?? [])];
      if (tags.areTagsExpired(combined, hit.lastModified)) return null;
      return { value: stored, lastModified: hit.lastModified };
    }

    async #responseEntry(key, context) {
      const kind = context.kind;
      const hit = await ops.op_nextrs_cache_get(key);
      const tags = syncTags();
      if (hit === null) return this.#readSeed(key, context);
      // A response entry that left memory without a disk copy: a miss,
      // never the (older) build prerender.
      if (hit.value === null) return null;
      let stored;
      try {
        stored = JSON.parse(hit.value);
      } catch {
        return null;
      }
      if (stored?.v !== ENVELOPE) return null;
      const value = decodeResponse(stored.value);
      if (value !== null && value.kind !== kind) return null;
      const cacheTags = responseTags(value);
      if (cacheTags.length > 0 && tags.areTagsExpired(cacheTags, hit.lastModified)) return null;
      return { value, lastModified: hit.lastModified, cacheControl: stored.cacheControl };
    }

    async set(key, data, context) {
      key = String(key);
      const response = !context?.fetchCache && RESPONSE_KINDS.has(data?.kind ?? context?.kind);
      if (response) {
        try {
          await storeResponse(key, data ?? null, context?.cacheControl);
        } finally {
          releaseLease(key);
        }
        return;
      }
      if (data === null || data === undefined) {
        await ops.op_nextrs_cache_set(key, null, [], false, -1, false);
        return;
      }
      if (data.kind !== "FETCH") {
        if (!notStored.has(data.kind)) {
          notStored.add(data.kind);
          ops.op_nextrs_diagnostic("cache", `${data.kind} cache entries are not stored by NextRS`);
        }
        return;
      }
      const tags = context?.tags ?? data.tags ?? [];
      await ops.op_nextrs_cache_set(key, JSON.stringify(data), [...tags].map(String), false, -1, false);
    }

    async revalidateTag(tags, durations) {
      tags = (typeof tags === "string" ? [tags] : [...tags]).map(String);
      if (tags.length === 0) return;
      const given = durations !== undefined && durations !== null;
      await ops.op_nextrs_cache_revalidate(tags, given && typeof durations.expire === "number" ? durations.expire : -1, given);
      syncTags();
    }

    resetRequestCache() {}
  }

  // The "use cache" handler (Cache Components), offered as DefaultCache on
  // the same symbol: Next's built-in default is an LRU in module state, so
  // each isolate would cache, and see revalidations, on its own. This one
  // follows createDefaultCacheHandler (next/dist/server/lib/cache-handlers/
  // default.js) step by step, with entries in the shared store and tags
  // judged by Next's functions on the synced manifest. Next's compiled
  // "use cache" wrapper is unchanged: it serializes, keys and decides; this
  // only stores bytes. Revalidations reach the store through the
  // FetchCache handler (Next calls both); updateTags only re-syncs.
  const USE_CACHE_PREFIX = "use-cache:";
  async function streamToBytes(stream) {
    const chunks = [];
    let size = 0;
    const reader = stream.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      const chunk = typeof value === "string" ? new TextEncoder().encode(value) : value;
      chunks.push(chunk);
      size += chunk.byteLength;
    }
    const out = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
      out.set(chunk, offset);
      offset += chunk.byteLength;
    }
    return out;
  }
  function bytesToStream(bytes) {
    return new ReadableStream({
      start(controller) {
        controller.enqueue(bytes);
        controller.close();
      },
    });
  }
  const pendingUseCacheSets = new Map();
  const nextrsUseCache = {
    async get(cacheKey) {
      const pending = pendingUseCacheSets.get(cacheKey);
      if (pending) await pending;
      const hit = await ops.op_nextrs_cache_get(USE_CACHE_PREFIX + cacheKey);
      const tags = syncTags();
      if (hit === null || hit.value === null) return undefined;
      let stored;
      try {
        stored = JSON.parse(hit.value);
      } catch {
        return undefined;
      }
      const entry = stored?.entry;
      if (stored?.v !== ENVELOPE || !entry) return undefined;
      if (entry.expire < 0) return undefined;
      if (performance.timeOrigin + performance.now() > entry.timestamp + entry.revalidate * 1000) return undefined;
      let revalidate = entry.revalidate;
      if (tags.areTagsExpired(entry.tags, entry.timestamp)) return undefined;
      if (tags.areTagsStale(entry.tags, entry.timestamp)) revalidate = -1;
      return { ...entry, revalidate, value: bytesToStream(new Uint8Array(Buffer.from(stored.value, "base64"))) };
    },
    async set(cacheKey, pendingEntry) {
      let resolvePending = () => {};
      pendingUseCacheSets.set(cacheKey, new Promise((resolve) => (resolvePending = resolve)));
      try {
        const entry = await pendingEntry;
        // As Next's handler: an expire: 0 entry is dynamic and never served.
        if (entry.expire === 0) return;
        const bytes = await streamToBytes(entry.value);
        const { value: _value, ...metadata } = entry;
        const envelope = JSON.stringify({ v: ENVELOPE, entry: metadata, value: Buffer.from(bytes).toString("base64") });
        await ops.op_nextrs_cache_set(USE_CACHE_PREFIX + cacheKey, envelope, (metadata.tags ?? []).map(String), true, -1, false);
      } catch {
        // As Next's handler: a failed entry is not stored.
      } finally {
        resolvePending();
        pendingUseCacheSets.delete(cacheKey);
      }
    },
    async refreshTags() {
      syncTags();
    },
    async getExpiration(tags) {
      const manifest = syncTags().tagsManifest;
      return Math.max(...tags.map((tag) => manifest.get(tag)?.expired || 0), 0);
    },
    async updateTags() {
      syncTags();
    },
  };

  globalThis.__nextrsStart = function start() {
    delete globalThis.__nextrsStart;
    delete globalThis.__nextrsInternal;
    delete globalThis.Deno;
    if (boot.cache) {
      Object.defineProperty(globalThis, Symbol.for("@next/cache-handlers"), {
        value: { FetchCache: NextRSCacheHandler, DefaultCache: nextrsUseCache },
        writable: true,
        enumerable: false,
        configurable: true,
      });
    }
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
