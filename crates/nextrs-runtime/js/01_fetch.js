// NextRS bootstrap, part 2: Headers, Request and Response (WHATWG Fetch),
// built on deno_web's streams, and outbound `fetch`. Next.js needs these
// classes (NextRequest extends Request; route handlers return Response).
// FormData is not supported and fails explicitly.
//
// `fetch` goes through the host's network capability (src/net.rs): the
// destination policy, DNS checks, redirects, timeouts and size limits are
// all enforced in Rust. Without a network configuration every fetch fails.
// deno_fetch is not used: it brings its own HTTP client stack and
// permission model.
"use strict";

((globalThis) => {
  const internal = globalThis.__nextrsInternal;
  const { unsupported, ops } = internal;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();

  // ---------------------------------------------------------------------
  // Headers
  // ---------------------------------------------------------------------
  const TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
  const kList = Symbol("headers list");
  const kGuard = Symbol("headers guard");

  function normalizeName(name) {
    name = String(name);
    if (!TOKEN.test(name)) throw new TypeError(`Invalid header name: "${name}"`);
    return name.toLowerCase();
  }

  function normalizeValue(value) {
    value = String(value).replace(/^[\t\n\r ]+|[\t\n\r ]+$/g, "");
    if (/[\0\r\n]/.test(value)) throw new TypeError(`Invalid header value: "${value}"`);
    return value;
  }

  class Headers {
    constructor(init) {
      this[kList] = [];
      this[kGuard] = "none";
      if (init === undefined || init === null) return;
      if (init instanceof Headers) {
        for (const [name, value] of init[kList]) this[kList].push([name, value]);
      } else if (typeof init === "object" && typeof init[Symbol.iterator] === "function") {
        for (const pair of init) {
          const entry = [...pair];
          if (entry.length !== 2) throw new TypeError("Header pairs must contain exactly two items");
          this.append(entry[0], entry[1]);
        }
      } else if (typeof init === "object") {
        for (const name of Object.keys(init)) this.append(name, init[name]);
      } else {
        throw new TypeError("Headers init must be an object or iterable");
      }
    }

    #check() {
      if (this[kGuard] === "immutable") throw new TypeError("Headers are immutable");
    }

    append(name, value) {
      this.#check();
      this[kList].push([normalizeName(name), normalizeValue(value)]);
    }

    delete(name) {
      this.#check();
      name = normalizeName(name);
      this[kList] = this[kList].filter(([n]) => n !== name);
    }

    get(name) {
      name = normalizeName(name);
      const values = this[kList].filter(([n]) => n === name).map(([, v]) => v);
      return values.length ? values.join(", ") : null;
    }

    getSetCookie() {
      return this[kList].filter(([n]) => n === "set-cookie").map(([, v]) => v);
    }

    has(name) {
      name = normalizeName(name);
      return this[kList].some(([n]) => n === name);
    }

    set(name, value) {
      this.#check();
      name = normalizeName(name);
      value = normalizeValue(value);
      const index = this[kList].findIndex(([n]) => n === name);
      if (index === -1) {
        this[kList].push([name, value]);
        return;
      }
      this[kList][index][1] = value;
      this[kList] = this[kList].filter(([n], i) => n !== name || i === index);
    }

    // Sorted and combined, except set-cookie (Fetch "sort and combine").
    *entries() {
      const names = [...new Set(this[kList].map(([n]) => n))].sort();
      for (const name of names) {
        if (name === "set-cookie") {
          for (const value of this.getSetCookie()) yield [name, value];
        } else {
          yield [name, this.get(name)];
        }
      }
    }

    *keys() {
      for (const [name] of this.entries()) yield name;
    }

    *values() {
      for (const [, value] of this.entries()) yield value;
    }

    forEach(callback, thisArg) {
      for (const [name, value] of this.entries()) callback.call(thisArg, value, name, this);
    }

    [Symbol.iterator]() {
      return this.entries();
    }

    get [Symbol.toStringTag]() {
      return "Headers";
    }
  }

  // ---------------------------------------------------------------------
  // Body
  // ---------------------------------------------------------------------
  const kBody = Symbol("body");

  // Returns { stream, contentType, source }. `source` is the body's bytes
  // (or Blob) when it has a known content, so fetch can send it with a
  // length and resend it on a 307/308 redirect; null for streams.
  function extractBody(init) {
    if (init === null || init === undefined) return { stream: null, contentType: null, source: null };
    if (init instanceof ReadableStream) return { stream: init, contentType: null, source: null };
    let bytes;
    let contentType = null;
    if (typeof init === "string") {
      bytes = encoder.encode(init);
      contentType = "text/plain;charset=UTF-8";
    } else if (init instanceof URLSearchParams) {
      bytes = encoder.encode(init.toString());
      contentType = "application/x-www-form-urlencoded;charset=UTF-8";
    } else if (init instanceof ArrayBuffer) {
      bytes = new Uint8Array(init.slice(0));
    } else if (ArrayBuffer.isView(init)) {
      bytes = new Uint8Array(init.buffer.slice(init.byteOffset, init.byteOffset + init.byteLength));
    } else if (init instanceof Blob) {
      return { stream: init.stream(), contentType: init.type || null, source: init };
    } else if (typeof FormData !== "undefined" && init instanceof FormData) {
      unsupported("Fetch body", "FormData");
    } else if (typeof init[Symbol.asyncIterator] === "function") {
      return { stream: ReadableStream.from(init), contentType: null, source: null };
    } else {
      bytes = encoder.encode(String(init));
      contentType = "text/plain;charset=UTF-8";
    }
    const stream = new ReadableStream({
      type: "bytes",
      start(controller) {
        if (bytes.byteLength) controller.enqueue(bytes.slice());
        controller.close();
      },
    });
    return { stream, contentType, source: bytes };
  }

  async function consumeBytes(owner) {
    const stream = owner[kBody];
    if (stream === null) return new Uint8Array(0);
    if (stream.locked || owner.bodyUsed) throw new TypeError("Body is unusable: Body has already been read");
    disturbed.add(stream);
    const reader = stream.getReader();
    const chunks = [];
    let length = 0;
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      const chunk = typeof value === "string" ? encoder.encode(value) : new Uint8Array(value.buffer ?? value, value.byteOffset ?? 0, value.byteLength);
      chunks.push(chunk);
      length += chunk.byteLength;
    }
    const out = new Uint8Array(length);
    let offset = 0;
    for (const chunk of chunks) {
      out.set(chunk, offset);
      offset += chunk.byteLength;
    }
    return out;
  }

  const BodyMixin = {
    get body() {
      return this[kBody];
    },
    get bodyUsed() {
      const stream = this[kBody];
      return stream !== null && (stream.locked || isDisturbed(stream));
    },
    async arrayBuffer() {
      return (await consumeBytes(this)).buffer;
    },
    async bytes() {
      return consumeBytes(this);
    },
    async text() {
      return decoder.decode(await consumeBytes(this));
    },
    async json() {
      return JSON.parse(decoder.decode(await consumeBytes(this)));
    },
    async blob() {
      return new Blob([await consumeBytes(this)], { type: this.headers.get("content-type") ?? "" });
    },
    async formData() {
      return unsupported("Body", "formData");
    },
  };

  // deno_web keeps a stream's "disturbed" flag private. Bodies consumed
  // through these classes are tracked here; a body read through its own
  // reader counts as used while that reader holds the lock.
  const disturbed = new WeakSet();
  const isDisturbed = (stream) => disturbed.has(stream);

  function mixBody(Class) {
    for (const name of Object.getOwnPropertyNames(BodyMixin)) {
      Object.defineProperty(Class.prototype, name, Object.getOwnPropertyDescriptor(BodyMixin, name));
    }
  }

  function cloneBody(owner) {
    const stream = owner[kBody];
    if (stream === null) return null;
    if (owner.bodyUsed) throw new TypeError("Body has already been consumed");
    const [a, b] = stream.tee();
    owner[kBody] = a;
    return b;
  }

  // ---------------------------------------------------------------------
  // Request
  // ---------------------------------------------------------------------
  const METHODS = new Set(["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT", "PATCH"]);
  const FORBIDDEN_METHODS = new Set(["CONNECT", "TRACE", "TRACK"]);
  const kState = Symbol("request state");
  const kSource = Symbol("body source");

  class Request {
    constructor(input, init = {}) {
      init ??= {};
      let state;
      if (input instanceof Request) {
        state = { ...input[kState], headers: new Headers(input.headers) };
        this[kBody] = input[kBody];
        this[kSource] = input[kSource];
      } else {
        // No document base URL exists in this runtime.
        const parsed = new URL(String(input));
        if (parsed.username || parsed.password) throw new TypeError("Request URL must not include credentials");
        state = {
          url: parsed.href,
          method: "GET",
          headers: new Headers(),
          signal: null,
          cache: "default",
          credentials: "same-origin",
          mode: "cors",
          redirect: "follow",
          referrer: "about:client",
          referrerPolicy: "",
          integrity: "",
          keepalive: false,
          duplex: "half",
        };
        this[kBody] = null;
        this[kSource] = null;
      }
      if (init.method !== undefined) {
        let method = String(init.method);
        if (!TOKEN.test(method)) throw new TypeError(`Invalid method: ${method}`);
        if (FORBIDDEN_METHODS.has(method.toUpperCase())) throw new TypeError(`Forbidden method: ${method}`);
        if (METHODS.has(method.toUpperCase())) method = method.toUpperCase();
        state.method = method;
      }
      for (const key of ["cache", "credentials", "mode", "redirect", "referrer", "referrerPolicy", "integrity", "keepalive", "duplex"]) {
        if (init[key] !== undefined) state[key] = init[key];
      }
      if (init.headers !== undefined) state.headers = new Headers(init.headers);
      const controller = new AbortController();
      const parentSignal = init.signal !== undefined ? init.signal : input instanceof Request ? input.signal : null;
      if (parentSignal) {
        if (parentSignal.aborted) controller.abort(parentSignal.reason);
        else parentSignal.addEventListener("abort", () => controller.abort(parentSignal.reason), { once: true });
      }
      state.signal = controller.signal;
      if (init.body !== undefined && init.body !== null) {
        if (state.method === "GET" || state.method === "HEAD") {
          throw new TypeError("Request with GET/HEAD method cannot have body.");
        }
        const { stream, contentType, source } = extractBody(init.body);
        this[kBody] = stream;
        this[kSource] = source;
        if (contentType && !state.headers.has("content-type")) state.headers.append("content-type", contentType);
      }
      this[kState] = state;
    }

    get url() { return this[kState].url; }
    get method() { return this[kState].method; }
    get headers() { return this[kState].headers; }
    get signal() { return this[kState].signal; }
    get cache() { return this[kState].cache; }
    get credentials() { return this[kState].credentials; }
    get mode() { return this[kState].mode; }
    get redirect() { return this[kState].redirect; }
    get referrer() { return this[kState].referrer; }
    get referrerPolicy() { return this[kState].referrerPolicy; }
    get integrity() { return this[kState].integrity; }
    get keepalive() { return this[kState].keepalive; }
    get duplex() { return this[kState].duplex; }
    get destination() { return ""; }
    get isHistoryNavigation() { return false; }
    get isReloadNavigation() { return false; }

    clone() {
      const clone = Object.create(Object.getPrototypeOf(this));
      clone[kState] = { ...this[kState], headers: new Headers(this.headers) };
      clone[kBody] = cloneBody(this);
      clone[kSource] = this[kSource];
      return clone;
    }

    get [Symbol.toStringTag]() {
      return "Request";
    }
  }
  mixBody(Request);

  // ---------------------------------------------------------------------
  // Response
  // ---------------------------------------------------------------------
  const NULL_BODY_STATUS = new Set([101, 103, 204, 205, 304]);
  const REDIRECT_STATUS = new Set([301, 302, 303, 307, 308]);

  class Response {
    constructor(body = null, init = {}) {
      init ??= {};
      const status = init.status === undefined ? 200 : Number(init.status);
      if (!Number.isInteger(status) || status < 200 || status > 599) {
        throw new RangeError(`The status provided (${init.status}) is outside the range [200, 599].`);
      }
      const statusText = init.statusText === undefined ? "" : String(init.statusText);
      this[kState] = {
        status,
        statusText,
        headers: new Headers(init.headers),
        type: "default",
        url: "",
        redirected: false,
      };
      this[kBody] = null;
      if (body !== null && body !== undefined) {
        if (NULL_BODY_STATUS.has(status)) throw new TypeError(`Response with status ${status} cannot have a body.`);
        const { stream, contentType } = extractBody(body);
        this[kBody] = stream;
        if (contentType && !this.headers.has("content-type")) this.headers.append("content-type", contentType);
      }
    }

    static json(data, init = {}) {
      const text = JSON.stringify(data);
      if (text === undefined) throw new TypeError("Value is not JSON serializable");
      const headers = new Headers(init?.headers);
      if (!headers.has("content-type")) headers.set("content-type", "application/json");
      return new Response(text, { ...init, headers });
    }

    static redirect(url, status = 302) {
      if (!REDIRECT_STATUS.has(status)) throw new RangeError(`Invalid redirect status: ${status}`);
      const response = new Response(null, { status, headers: { location: new URL(String(url)).href } });
      response.headers[kGuard] = "immutable";
      return response;
    }

    static error() {
      const response = new Response(null, { status: 200 });
      response[kState].status = 0;
      response[kState].type = "error";
      response.headers[kGuard] = "immutable";
      return response;
    }

    get status() { return this[kState].status; }
    get statusText() { return this[kState].statusText; }
    get ok() { return this[kState].status >= 200 && this[kState].status <= 299; }
    get headers() { return this[kState].headers; }
    get type() { return this[kState].type; }
    get url() { return this[kState].url; }
    get redirected() { return this[kState].redirected; }

    clone() {
      const clone = Object.create(Object.getPrototypeOf(this));
      clone[kState] = { ...this[kState], headers: new Headers(this.headers) };
      clone[kBody] = cloneBody(this);
      return clone;
    }

    get [Symbol.toStringTag]() {
      return "Response";
    }
  }
  mixBody(Response);

  class FormData {
    constructor() {
      unsupported("FormData", "constructor");
    }
  }

  // ---------------------------------------------------------------------
  // fetch
  // ---------------------------------------------------------------------

  // The fetches each request started (set by 03_server.js). Those still
  // open when the request ends, its waitUntil/after() work included, or
  // when it is cancelled, are aborted: nothing outlives its request.
  const requestScope = new internal.AsyncLocalStorage();
  internal.requestScope = requestScope;

  // A response body nobody reads to the end still holds its connection;
  // release it once the body stream is garbage.
  const closeOnCollect = new FinalizationRegistry((id) => ops.op_nextrs_fetch_close(id));

  // Host errors are "CODE: message". Node reports network failures as
  // TypeError("fetch failed") with the underlying error as `cause`.
  function fetchFailed(message, error) {
    const text = String(error?.message ?? error);
    const match = /^([A-Z][A-Z0-9_]+): ([\s\S]*)$/.exec(text);
    const cause = new Error(match ? match[2] : text);
    if (match) cause.code = match[1];
    return new TypeError(message, { cause });
  }

  // Set by Node's fetch when absent.
  const DEFAULT_HEADERS = [
    ["accept", "*/*"],
    ["accept-language", "*"],
    ["sec-fetch-mode", "cors"],
    ["user-agent", "node"],
  ];
  // Framing and connection headers belong to the client.
  const FORBIDDEN_HEADERS = new Set(["connection", "keep-alive", "transfer-encoding", "upgrade", "te", "trailer", "host"]);

  function outgoingHeaders(headers) {
    const list = [];
    for (const [name, value] of headers[kList]) {
      if (FORBIDDEN_HEADERS.has(name) || name.startsWith("proxy-")) {
        throw new TypeError(`fetch: the '${name}' request header cannot be set`);
      }
      // Computed from the body.
      if (name === "content-length") continue;
      if (/[^\x00-\xff]/.test(value)) throw new TypeError(`fetch: the '${name}' header value is not a byte string`);
      list.push([name, value]);
    }
    for (const [name, value] of DEFAULT_HEADERS) {
      if (!headers.has(name)) list.push([name, value]);
    }
    return list;
  }

  function toBytes(value) {
    if (value instanceof Uint8Array) return value;
    if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (typeof value === "string") return encoder.encode(value);
    throw new TypeError("fetch: request body chunks must be bytes");
  }

  // Writes a streamed request body; each write waits until the network
  // task takes the chunk.
  async function upload(id, stream, abort) {
    const reader = stream.getReader();
    disturbed.add(stream);
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        if (!(await ops.op_nextrs_fetch_write(id, toBytes(value)))) {
          reader.cancel().catch(() => {});
          return;
        }
      }
      ops.op_nextrs_fetch_write_end(id);
    } catch (error) {
      abort(fetchFailed("fetch failed", error));
      reader.cancel(error).catch(() => {});
    }
  }

  function makeResponse(head, body) {
    const headers = new Headers();
    for (const [name, value] of head.headers) headers[kList].push([name, value]);
    headers[kGuard] = "immutable";
    const response = Object.create(Response.prototype);
    response[kState] = {
      status: head.status,
      statusText: head.statusText,
      headers,
      type: "basic",
      url: head.url,
      redirected: head.redirected,
    };
    response[kBody] = body;
    return response;
  }

  const EMPTY = new Uint8Array(0);

  async function fetch(input, init = undefined) {
    const request = new Request(input, init);
    const signal = request.signal;
    if (signal.aborted) throw signal.reason;
    const headers = outgoingHeaders(request.headers);
    let bodyKind = "none";
    let bytes = EMPTY;
    let stream = null;
    if (request[kBody] !== null) {
      if (request.bodyUsed) throw new TypeError("fetch: the request body has already been read");
      const source = request[kSource];
      if (source instanceof Uint8Array) {
        bodyKind = "bytes";
        bytes = source;
      } else if (source instanceof Blob) {
        bodyKind = "bytes";
        bytes = new Uint8Array(await source.arrayBuffer());
      } else {
        bodyKind = "stream";
        stream = request[kBody];
      }
    }
    if (signal.aborted) throw signal.reason;
    let id;
    try {
      id = ops.op_nextrs_fetch_start(
        { method: request.method, url: request.url, headers, bodyKind, redirect: request.redirect },
        bytes,
      );
    } catch (error) {
      throw fetchFailed("fetch failed", error);
    }
    return started(id, request, stream);
  }

  function started(id, request, stream) {
    const signal = request.signal;
    const scope = requestScope.getStore();
    return new Promise((resolve, reject) => {
      // "head": waiting for the response; "body": the body is streaming.
      let phase = "head";
      let controller = null;
      let body = null;
      let closed = false;
      const close = () => {
        if (closed) return;
        closed = true;
        ops.op_nextrs_fetch_close(id);
        signal.removeEventListener("abort", onAbort);
        scope?.fetches?.delete(abort);
        if (body) closeOnCollect.unregister(body);
      };
      const abort = (reason) => {
        if (closed) return;
        close();
        if (phase === "head") reject(reason);
        else if (phase === "body") {
          try {
            controller.error(reason);
          } catch {
            // Already closed or errored.
          }
        }
        phase = "done";
      };
      const onAbort = () => abort(signal.reason);
      signal.addEventListener("abort", onAbort, { once: true });
      if (scope) (scope.fetches ??= new Set()).add(abort);
      if (stream) upload(id, stream, abort);

      ops.op_nextrs_fetch_head(id).then(
        (head) => {
          if (closed) return;
          if (NULL_BODY_STATUS.has(head.status) || request.method === "HEAD") {
            phase = "done";
            close();
            resolve(makeResponse(head, null));
            return;
          }
          phase = "body";
          body = new ReadableStream(
            {
              start(c) {
                controller = c;
              },
              async pull(c) {
                let chunk;
                try {
                  chunk = await ops.op_nextrs_fetch_read(id);
                } catch (error) {
                  if (closed) return;
                  phase = "done";
                  close();
                  c.error(fetchFailed("terminated", error));
                  return;
                }
                if (closed) return;
                if (chunk === null) {
                  phase = "done";
                  close();
                  c.close();
                } else {
                  c.enqueue(chunk);
                }
              },
              cancel() {
                phase = "done";
                close();
              },
            },
            { highWaterMark: 0 },
          );
          closeOnCollect.register(body, id, body);
          resolve(makeResponse(head, body));
        },
        (error) => {
          if (closed) return;
          phase = "done";
          close();
          reject(fetchFailed("fetch failed", error));
        },
      );
    });
  }

  for (const [name, value] of Object.entries({ Headers, Request, Response, FormData, fetch })) {
    Object.defineProperty(globalThis, name, { value, writable: true, enumerable: false, configurable: true });
  }
})(globalThis);
