// NextRS bootstrap, part 1: Web API globals (from deno_web), timers and
// process.nextTick that carry the async context, `process`, and
// AsyncLocalStorage on V8's continuation-preserved embedder data.
//
// Shared state for the later bootstrap scripts lives on
// globalThis.__nextrsInternal, which 03_server.js deletes.
"use strict";

((globalThis) => {
  const core = globalThis.Deno.core;
  const ops = core.ops;
  const boot = ops.op_nextrs_boot_config();
  const load = (name) => core.loadExtScript(`ext:deno_web/${name}.js`);

  // ---------------------------------------------------------------------
  // Unsupported-API errors. Never fabricate a result: throw this instead.
  // ---------------------------------------------------------------------
  class NextRSUnsupportedError extends Error {
    constructor(api, operation, detail) {
      const caller = callerOf(new Error().stack);
      super(
        `[NextRS] ${api}${operation ? `.${operation}` : ""} is not supported by the NextRS embedded runtime` +
          (detail ? `: ${detail}` : "") +
          (caller ? ` (called from ${caller})` : ""),
      );
      this.name = "NextRSUnsupportedError";
      this.code = "ERR_NEXTRS_UNSUPPORTED";
      this.api = api;
      this.operation = operation;
      this.caller = caller;
      this.compatibilityStatus = "unsupported";
      ops.op_nextrs_diagnostic("unsupported", this.message);
    }
  }

  // First stack frame outside the NextRS bootstrap scripts.
  function callerOf(stack) {
    for (const line of String(stack || "").split("\n").slice(1)) {
      if (!line.includes("ext:nextrs/") && !line.includes("ext:deno_web/") && !line.includes("ext:core/")) {
        return line.trim().replace(/^at /, "");
      }
    }
    return undefined;
  }

  const unsupported = (api, operation, detail) => {
    throw new NextRSUnsupportedError(api, operation, detail);
  };

  // ---------------------------------------------------------------------
  // Web APIs: deno_web's implementations, installed like Node 22 does.
  // ---------------------------------------------------------------------
  const url = load("00_url");
  const domException = load("01_dom_exception");
  const event = load("02_event");
  const structuredCloneModule = load("02_structured_clone");
  const abortSignal = load("03_abort_signal");
  const base64 = load("05_base64");
  const streams = load("06_streams");
  const encoding = load("08_text_encoding");
  const file = load("09_file");
  const messagePort = load("13_message_port");
  const perf = load("15_performance");
  const consoleModule = load("01_console");

  event.saveGlobalThisReference(globalThis);
  perf.setTimeOrigin();

  const define = (name, value) =>
    Object.defineProperty(globalThis, name, { value, writable: true, enumerable: false, configurable: true });

  for (const [name, value] of Object.entries({
    URL: url.URL,
    URLSearchParams: url.URLSearchParams,
    DOMException: domException.DOMException,
    Event: event.Event,
    EventTarget: event.EventTarget,
    CustomEvent: event.CustomEvent,
    MessageEvent: event.MessageEvent,
    structuredClone: structuredCloneModule.structuredClone,
    AbortController: abortSignal.AbortController,
    AbortSignal: abortSignal.AbortSignal,
    atob: base64.atob,
    btoa: base64.btoa,
    ReadableStream: streams.ReadableStream,
    ReadableStreamDefaultReader: streams.ReadableStreamDefaultReader,
    ReadableStreamBYOBReader: streams.ReadableStreamBYOBReader,
    ReadableByteStreamController: streams.ReadableByteStreamController,
    ReadableStreamDefaultController: streams.ReadableStreamDefaultController,
    WritableStream: streams.WritableStream,
    WritableStreamDefaultWriter: streams.WritableStreamDefaultWriter,
    TransformStream: streams.TransformStream,
    ByteLengthQueuingStrategy: streams.ByteLengthQueuingStrategy,
    CountQueuingStrategy: streams.CountQueuingStrategy,
    TextEncoder: encoding.TextEncoder,
    TextDecoder: encoding.TextDecoder,
    TextEncoderStream: encoding.TextEncoderStream,
    TextDecoderStream: encoding.TextDecoderStream,
    Blob: file.Blob,
    File: file.File,
    MessageChannel: messagePort.MessageChannel,
    MessagePort: messagePort.MessagePort,
    performance: perf.performance,
    Performance: perf.Performance,
    PerformanceEntry: perf.PerformanceEntry,
    PerformanceMark: perf.PerformanceMark,
    PerformanceMeasure: perf.PerformanceMeasure,
  })) {
    define(name, value);
  }

  // Node's default abort reason (`controller.abort()`, `AbortSignal.abort()`)
  // is DOMException("This operation was aborted", "AbortError"); Deno's
  // message differs, and applications report or match on it.
  {
    const nodeAbortReason = () => new domException.DOMException("This operation was aborted", "AbortError");
    const abort = abortSignal.AbortController.prototype.abort;
    Object.defineProperty(abortSignal.AbortController.prototype, "abort", {
      value: {
        abort(reason) {
          return abort.call(this, reason === undefined ? nodeAbortReason() : reason);
        },
      }.abort,
      writable: true,
      configurable: true,
    });
    const staticAbort = abortSignal.AbortSignal.abort;
    Object.defineProperty(abortSignal.AbortSignal, "abort", {
      value: {
        abort(reason) {
          return staticAbort.call(this, reason === undefined ? nodeAbortReason() : reason);
        },
      }.abort,
      writable: true,
      configurable: true,
    });
  }
  define("global", globalThis);

  const console = new consoleModule.Console((message, level) => core.print(message, level > 1));
  define("console", console);

  // ---------------------------------------------------------------------
  // Async context. V8 carries the embedder data across promise reactions;
  // everything the host schedules (timers, immediates, nextTick) must
  // capture it when scheduled and restore it when run.
  // ---------------------------------------------------------------------
  const getContext = core.getAsyncContext;
  const setContext = core.setAsyncContext;

  function runInContext(context, fn, thisArg, args) {
    const previous = getContext();
    setContext(context);
    try {
      return args === undefined ? fn.call(thisArg) : fn.apply(thisArg, args);
    } finally {
      setContext(previous);
    }
  }

  // Each store maps an AsyncLocalStorage instance to its value. Frames are
  // immutable: run()/enterWith() install a copy, so a captured frame never
  // observes later changes made by other requests.
  const EMPTY_FRAME = new Map();
  const currentFrame = () => {
    const frame = getContext();
    return frame instanceof Map ? frame : EMPTY_FRAME;
  };

  class AsyncLocalStorage {
    #enabled = false;

    static bind(fn) {
      return AsyncLocalStorage.snapshot().bind(undefined, fn);
    }

    static snapshot() {
      const frame = getContext();
      return function runInSnapshot(fn, ...args) {
        return runInContext(frame, fn, this, args);
      };
    }

    getStore() {
      return this.#enabled ? currentFrame().get(this) : undefined;
    }

    run(store, callback, ...args) {
      this.#enabled = true;
      const frame = new Map(currentFrame());
      frame.set(this, store);
      return runInContext(frame, callback, undefined, args);
    }

    exit(callback, ...args) {
      const frame = new Map(currentFrame());
      frame.delete(this);
      return runInContext(frame, callback, undefined, args);
    }

    enterWith(store) {
      this.#enabled = true;
      const frame = new Map(currentFrame());
      frame.set(this, store);
      setContext(frame);
    }

    disable() {
      this.#enabled = false;
      const frame = currentFrame();
      if (frame.has(this)) {
        const next = new Map(frame);
        next.delete(this);
        setContext(next);
      }
    }
  }

  class AsyncResource {
    #frame = getContext();
    #type;

    constructor(type) {
      if (typeof type !== "string") throw new TypeError('The "type" argument must be of type string');
      this.#type = type;
    }

    static bind(fn, type, thisArg) {
      return new AsyncResource(type || fn.name || "bound-anonymous-fn").bind(fn, thisArg);
    }

    runInAsyncScope(fn, thisArg, ...args) {
      return runInContext(this.#frame, fn, thisArg, args);
    }

    bind(fn, thisArg) {
      const resource = this;
      return function bound(...args) {
        return resource.runInAsyncScope(fn, thisArg === undefined ? this : thisArg, ...args);
      };
    }

    emitDestroy() {
      return this;
    }

    asyncId() {
      return unsupported("async_hooks.AsyncResource", "asyncId", "async IDs are not tracked");
    }

    triggerAsyncId() {
      return unsupported("async_hooks.AsyncResource", "triggerAsyncId", "async IDs are not tracked");
    }
  }

  // ---------------------------------------------------------------------
  // Timers (Node semantics on deno_core's libuv-style loop).
  // ---------------------------------------------------------------------
  const kTimer = Symbol("nextrs.timer");
  const timersById = new Map();

  function checkCallback(callback) {
    if (typeof callback !== "function") {
      const error = new TypeError(`The "callback" argument must be of type function. Received ${typeof callback}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
  }

  class Timeout {
    #refed = true;

    constructor(callback, delay, args, repeat) {
      checkCallback(callback);
      const context = getContext();
      this._onTimeout = callback;
      this._repeat = repeat;
      const timeout = this;
      this[kTimer] = core.createTimer(
        () => {
          if (!repeat) timersById.delete(timeout[kTimer]._timerId);
          runInContext(context, callback, timeout, args);
        },
        delay,
        undefined,
        repeat,
        true,
      );
      timersById.set(this[kTimer]._timerId, this);
    }

    ref() {
      if (!this.#refed) {
        this.#refed = true;
        core.refTimer(this[kTimer]);
      }
      return this;
    }

    unref() {
      if (this.#refed) {
        this.#refed = false;
        core.unrefTimer(this[kTimer]);
      }
      return this;
    }

    hasRef() {
      return this.#refed;
    }

    refresh() {
      core.refreshTimer(this[kTimer]);
      return this;
    }

    close() {
      clearTimeout(this);
      return this;
    }

    [Symbol.toPrimitive]() {
      return this[kTimer]._timerId;
    }

    [Symbol.dispose]() {
      clearTimeout(this);
    }
  }

  function setTimeout(callback, delay, ...args) {
    return new Timeout(callback, delay, args.length ? args : undefined, false);
  }

  function setInterval(callback, delay, ...args) {
    return new Timeout(callback, delay, args.length ? args : undefined, true);
  }

  function clearTimeout(timer) {
    if (timer == null) return;
    const timeout = timer instanceof Timeout ? timer : timersById.get(Number(timer));
    if (!timeout) return;
    timersById.delete(timeout[kTimer]._timerId);
    core.cancelTimer(timeout[kTimer]);
  }

  class Immediate {
    constructor(callback, args) {
      checkCallback(callback);
      const context = getContext();
      this._idleNext = null;
      this._idlePrev = null;
      this._onImmediate = () => runInContext(context, callback, this, args);
      this._argv = undefined;
      this._destroyed = false;
      this.asyncId = 0;
      this.triggerAsyncId = 0;
      this[core.kRefed] = true;
      core.immediateRefCount(true);
      core.queueImmediate(this);
    }

    ref() {
      if (!this[core.kRefed] && !this._destroyed) {
        this[core.kRefed] = true;
        core.immediateRefCount(true);
      }
      return this;
    }

    unref() {
      if (this[core.kRefed]) {
        this[core.kRefed] = false;
        core.immediateRefCount(false);
      }
      return this;
    }

    hasRef() {
      return Boolean(this[core.kRefed]);
    }

    [Symbol.dispose]() {
      clearImmediate(this);
    }
  }

  function setImmediate(callback, ...args) {
    return new Immediate(callback, args.length ? args : undefined);
  }

  function clearImmediate(immediate) {
    if (immediate instanceof Immediate) core.clearImmediate(immediate);
  }

  function nextTick(callback, ...args) {
    checkCallback(callback);
    core.queueNextTick({
      callback,
      args: args.length ? args : undefined,
      snapshot: getContext(),
      asyncId: 0,
      triggerAsyncId: 0,
    });
  }

  for (const [name, value] of Object.entries({
    setTimeout,
    setInterval,
    clearTimeout,
    clearInterval: clearTimeout,
    setImmediate,
    clearImmediate,
  })) {
    define(name, value);
  }

  // ---------------------------------------------------------------------
  // process. Becomes an EventEmitter once the vendored `events` is loaded
  // (02_node.js); readable-stream needs `process` to exist before that.
  // ---------------------------------------------------------------------
  const env = new Proxy(Object.assign(Object.create(null), boot.env), {
    set(target, key, value) {
      target[key] = String(value);
      return true;
    },
    defineProperty(target, key, descriptor) {
      // Node's env only holds strings; Turbopack defines plain values.
      target[key] = String("value" in descriptor ? descriptor.value : descriptor.get?.());
      return true;
    },
  });

  const hrtime = (previous) => {
    const nanos = BigInt(Math.round(perf.performance.now() * 1e6)) + BigInt(Math.round(perf.performance.timeOrigin * 1e6));
    let seconds = Number(nanos / 1_000_000_000n);
    let rest = Number(nanos % 1_000_000_000n);
    if (previous) {
      seconds -= previous[0];
      rest -= previous[1];
      if (rest < 0) {
        seconds -= 1;
        rest += 1e9;
      }
    }
    return [seconds, rest];
  };
  hrtime.bigint = () =>
    BigInt(Math.round(perf.performance.now() * 1e6)) + BigInt(Math.round(perf.performance.timeOrigin * 1e6));

  const writer = (stderr) => ({
    isTTY: false,
    fd: stderr ? 2 : 1,
    write(chunk, encoding, callback) {
      core.print(typeof chunk === "string" ? chunk : new TextDecoder().decode(chunk), stderr);
      if (typeof encoding === "function") encoding();
      else if (typeof callback === "function") callback();
      return true;
    },
  });

  const process = {
    title: "nextrs",
    version: `v${boot.nodeVersion}`,
    versions: { node: boot.nodeVersion, v8: "nextrs-embedded", nextrs: boot.nextrsVersion },
    release: { name: "node" },
    platform: "linux",
    arch: "x64",
    pid: 1,
    ppid: 0,
    argv: ["nextrs"],
    argv0: "nextrs",
    execArgv: [],
    execPath: "/nextrs",
    exitCode: undefined,
    env,
    features: {},
    config: { variables: {} },
    stdout: writer(false),
    stderr: writer(true),
    cwd: () => boot.projectDir,
    chdir: (dir) => unsupported("process", "chdir", `cannot change directory to ${dir}`),
    exit: (code) => unsupported("process", "exit", `exit(${code}) would stop the shared runtime`),
    abort: () => unsupported("process", "abort"),
    kill: () => unsupported("process", "kill"),
    umask: () => 0o022,
    // A virtual, unprivileged identity, like pid 1 above: never the host's
    // own ids. Libraries probe these (an os.homedir fallback checks for
    // root with getuid()).
    getuid: () => 65534,
    geteuid: () => 65534,
    getgid: () => 65534,
    getegid: () => 65534,
    nextTick,
    hrtime,
    uptime: () => perf.performance.now() / 1000,
    memoryUsage: Object.assign(
      () => {
        const usage = core.memoryUsage();
        return { rss: usage.physicalTotal, heapTotal: usage.heapTotal, heapUsed: usage.heapUsed, external: usage.external, arrayBuffers: 0 };
      },
      { rss: () => core.memoryUsage().physicalTotal },
    ),
    cpuUsage: () => unsupported("process", "cpuUsage"),
    emitWarning(warning, type) {
      const message = typeof warning === "string" ? warning : warning?.message;
      core.print(`(nextrs) ${type && typeof type === "string" ? type : "Warning"}: ${message}\n`, true);
    },
  };
  define("process", process);

  core.setUnhandledPromiseRejectionHandler((promise, reason) => {
    if (typeof process.listenerCount === "function" && process.listenerCount("unhandledRejection") > 0) {
      process.emit("unhandledRejection", reason, promise);
      return true;
    }
    return false;
  });
  core.setReportExceptionCallback((error) => {
    if (typeof process.listenerCount === "function" && process.listenerCount("uncaughtException") > 0) {
      process.emit("uncaughtException", error, "uncaughtException");
      return;
    }
    core.reportUnhandledException(error);
  });

  globalThis.__nextrsInternal = {
    core,
    ops,
    boot,
    NextRSUnsupportedError,
    unsupported,
    AsyncLocalStorage,
    AsyncResource,
    Timeout,
    Immediate,
    streams,
    inspect: consoleModule.inspect,
    format: (...args) => consoleModule.inspectArgs(args, { colors: false }),
  };
})(globalThis);
