// Loads a real compiled Next.js route (`.next/server/app/**/page.js`) inside
// an embedded engine with a minimal CommonJS loader, and records exactly how
// far it gets. It does NOT implement Node.js; each level adds one layer of
// emulation so the first concrete blocker at each layer is visible.
//
//   level 0  bare engine: CommonJS loader only. Node built-ins throw.
//   level 1  + a minimal `process` global (env, cwd, nextTick, versions...).
//   level 2  + Node built-ins replaced by recording proxies (every property
//            access/call is logged and returns another proxy).
//   level 3  + call the route's `handler(req, res, ctx)` with plain objects
//            standing in for IncomingMessage/ServerResponse.
//   level 4  + a real read-only `fs` subset backed by the host
//            (readFileSync, existsSync, promises.readFile).
//
// Host contract: __host.readText(path) -> string|null, __host.isFile(path),
// __host.isDir(path).
(function () {
  const BUILTINS = new Set([
    "assert", "assert/strict", "async_hooks", "buffer", "child_process", "cluster", "console",
    "constants", "crypto", "dgram", "diagnostics_channel", "dns", "dns/promises", "domain",
    "events", "fs", "fs/promises", "http", "http2", "https", "inspector", "module", "net", "os",
    "path", "path/posix", "perf_hooks", "process", "punycode", "querystring", "readline",
    "repl", "stream", "stream/promises", "stream/web", "string_decoder", "timers",
    "timers/promises", "tls", "tty", "url", "util", "util/types", "v8", "vm", "wasi",
    "worker_threads", "zlib",
  ]);

  const NODE_AND_WEB_GLOBALS = [
    "Buffer", "setImmediate", "clearImmediate", "URL", "URLSearchParams", "TextEncoder",
    "TextDecoder", "Headers", "Request", "Response", "FormData", "Blob", "File", "fetch",
    "AbortController", "AbortSignal", "Event", "EventTarget", "crypto", "structuredClone",
    "performance", "ReadableStream", "WritableStream", "TransformStream", "MessageChannel",
    "BroadcastChannel", "WebSocket", "navigator",
  ];

  function normalize(path) {
    const out = [];
    for (const part of path.split("/")) {
      if (part === "" || part === ".") continue;
      if (part === "..") out.pop();
      else out.push(part);
    }
    return "/" + out.join("/");
  }
  const join = (...parts) => normalize(parts.join("/"));
  const dirname = (path) => normalize(path + "/..");

  function run(projectDir, entry, level) {
    const trace = {
      level,
      modulesLoaded: 0,
      bytesEvaluated: 0,
      builtinRequests: [],
      builtinAccess: new Set(),
      globalsStubbed: [],
      fsReads: [],
      lastModules: [],
    };
    const cache = new Map();

    function proxyFor(path) {
      const target = function () {};
      return new Proxy(target, {
        get(_t, prop) {
          if (typeof prop === "symbol") {
            if (prop === Symbol.toPrimitive) return () => `[nextrs-stub ${path}]`;
            return undefined;
          }
          if (prop === "then" || prop === "__esModule") return undefined;
          const member = `${path}.${prop}`;
          trace.builtinAccess.add(member);
          return proxyFor(member);
        },
        apply(_t, _this, _args) {
          trace.builtinAccess.add(`${path}()`);
          return proxyFor(`${path}()`);
        },
        construct(_t, _args) {
          trace.builtinAccess.add(`new ${path}`);
          return proxyFor(`new ${path}`);
        },
      });
    }

    // The one built-in implemented for real at level >= 2: pure string
    // logic that Turbopack's chunk loader cannot work without.
    const posixPath = {
      sep: "/",
      delimiter: ":",
      normalize,
      join,
      dirname,
      isAbsolute: (p) => p.startsWith("/"),
      basename(p, ext) {
        const base = normalize(p).split("/").pop();
        return ext && base.endsWith(ext) ? base.slice(0, -ext.length) : base;
      },
      extname(p) {
        const base = normalize(p).split("/").pop();
        const dot = base.lastIndexOf(".");
        return dot > 0 ? base.slice(dot) : "";
      },
      resolve(...segments) {
        let resolved = "";
        for (let i = segments.length - 1; i >= 0 && !resolved.startsWith("/"); i--) {
          resolved = segments[i] + (resolved ? "/" + resolved : "");
        }
        return normalize(resolved.startsWith("/") ? resolved : projectDir + "/" + resolved);
      },
      relative(from, to) {
        const a = normalize(from).split("/").filter(Boolean);
        const b = normalize(to).split("/").filter(Boolean);
        let i = 0;
        while (i < a.length && a[i] === b[i]) i++;
        return [...a.slice(i).map(() => ".."), ...b.slice(i)].join("/");
      },
    };
    posixPath.posix = posixPath;

    function readOnlyFs(name) {
      const missing = (path) => {
        const error = new Error(`ENOENT: no such file or directory, open '${path}'`);
        error.code = "ENOENT";
        return error;
      };
      const readFileSync = (path, options) => {
        const text = __host.readText(String(path));
        if (text === null || text === undefined) throw missing(path);
        trace.fsReads.push(String(path).replace(projectDir + "/", ""));
        return text;
      };
      const fs = {
        readFileSync,
        existsSync: (path) => __host.isFile(String(path)) || __host.isDir(String(path)),
        promises: { readFile: async (path, options) => readFileSync(path, options) },
      };
      const real = name === "fs/promises" ? fs.promises : fs;
      return new Proxy(real, {
        get(target, prop) {
          if (typeof prop === "symbol" || prop in target) return target[prop];
          trace.builtinAccess.add(`${name}.${prop}`);
          return proxyFor(`${name}.${prop}`);
        },
      });
    }

    function builtin(request) {
      const name = request.replace(/^node:/, "");
      trace.builtinRequests.push(name);
      if (level < 2) {
        const error = new Error(`Node.js built-in module '${name}' is not available in this engine`);
        error.code = "NEXTRS_NO_NODE_BUILTIN";
        throw error;
      }
      if (name === "path" || name === "path/posix") return posixPath;
      if (level >= 4 && (name === "fs" || name === "fs/promises")) return readOnlyFs(name);
      return proxyFor(name);
    }

    function resolveFile(base) {
      for (const candidate of [base, base + ".js", base + ".json", base + ".cjs", base + "/index.js"]) {
        if (__host.isFile(candidate)) return candidate;
      }
      if (__host.isDir(base) && __host.isFile(base + "/package.json")) {
        const pkg = JSON.parse(__host.readText(base + "/package.json"));
        return resolveFile(join(base, pkg.main || "index.js"));
      }
      return null;
    }

    function resolve(request, fromDir) {
      if (request.startsWith("./") || request.startsWith("../") || request.startsWith("/")) {
        const file = resolveFile(request.startsWith("/") ? normalize(request) : join(fromDir, request));
        if (file) return file;
      } else {
        for (let dir = fromDir; ; dir = dirname(dir)) {
          const file = resolveFile(join(dir, "node_modules", request));
          if (file) return file;
          if (dir === "/") break;
        }
      }
      const error = new Error(`Cannot find module '${request}' from ${fromDir}`);
      error.code = "MODULE_NOT_FOUND";
      throw error;
    }

    function load(filename) {
      const cached = cache.get(filename);
      if (cached) return cached.exports;
      const source = __host.readText(filename);
      const module = { exports: {}, filename, id: filename, loaded: false, children: [] };
      cache.set(filename, module);
      trace.modulesLoaded++;
      trace.bytesEvaluated += source.length;
      trace.lastModules.push(filename.replace(projectDir + "/", ""));
      if (trace.lastModules.length > 5) trace.lastModules.shift();
      if (filename.endsWith(".json")) {
        module.exports = JSON.parse(source);
        return module.exports;
      }
      const dir = dirname(filename);
      const localRequire = (request) => {
        if (BUILTINS.has(request.replace(/^node:/, ""))) return builtin(request);
        return load(resolve(request, dir));
      };
      localRequire.resolve = (request) => resolve(request, dir);
      localRequire.cache = {};
      const wrapper = (0, eval)(
        "(function (exports, require, module, __filename, __dirname) {" +
          source.replace(/^#!.*/, "") +
          "\n})\n//# sourceURL=" + filename,
      );
      wrapper.call(module.exports, module.exports, localRequire, module, filename, dir);
      module.loaded = true;
      return module.exports;
    }

    if (level >= 1) {
      const listeners = {};
      globalThis.process = {
        env: { NODE_ENV: "production", NEXT_RUNTIME: "nodejs", __NEXT_PRIVATE_ORIGIN: "" },
        argv: ["nextrs"],
        platform: "linux",
        arch: "x64",
        pid: 1,
        version: "v22.20.0",
        versions: { node: "22.20.0" },
        release: { name: "node" },
        execArgv: [],
        exitCode: undefined,
        cwd: () => projectDir,
        chdir: () => {},
        nextTick: (fn, ...args) => queueMicrotask(() => fn(...args)),
        hrtime: Object.assign(() => [0, 0], { bigint: () => BigInt(Date.now()) * 1000000n }),
        uptime: () => 0,
        memoryUsage: () => ({ rss: 0, heapTotal: 0, heapUsed: 0, external: 0 }),
        emitWarning: () => {},
        on(event, fn) {
          (listeners[event] ||= []).push(fn);
          return this;
        },
        once(event, fn) {
          return this.on(event, fn);
        },
        off() {
          return this;
        },
        removeListener() {
          return this;
        },
        listeners: (event) => listeners[event] || [],
        emit: () => false,
        stdout: { write: () => true, isTTY: false },
        stderr: { write: () => true, isTTY: false },
      };
      if (level >= 2) {
        // Node and web globals the engine lacks become recording proxies.
        globalThis.global = globalThis;
        for (const name of NODE_AND_WEB_GLOBALS) {
          if (typeof globalThis[name] === "undefined") {
            trace.globalsStubbed.push(name);
            globalThis[name] = proxyFor(name);
          }
        }
        // Members this stub lacks become recording proxies, like built-ins.
        const real = globalThis.process;
        globalThis.process = new Proxy(real, {
          get(target, prop) {
            if (typeof prop === "symbol" || prop in target) return target[prop];
            trace.builtinAccess.add(`process.${prop}`);
            return proxyFor(`process.${prop}`);
          },
        });
      }
    }

    const report = (phase, error) => {
      const result = {
        level,
        phase,
        modulesLoaded: trace.modulesLoaded,
        bytesEvaluated: trace.bytesEvaluated,
        builtinsRequested: [...new Set(trace.builtinRequests)].sort(),
        globalsStubbed: trace.globalsStubbed,
        fsReads: trace.fsReads,
        builtinMembersTouched: trace.builtinAccess.size,
        builtinMembersSample: [...trace.builtinAccess].slice(0, 60),
        lastModules: trace.lastModules,
      };
      if (error) {
        result.error = {
          name: error && error.name,
          code: error && error.code,
          message: String((error && error.message) || error),
          stack: String((error && error.stack) || "").split("\n").slice(0, 8),
        };
      }
      return result;
    };

    let mod;
    try {
      // The official adapter bootstrap, then the route entry, as a minimal
      // Node host would load them (see experiments/route-handler-harness).
      load(join(projectDir, "node_modules/next/dist/build/adapter/setup-node-env.external.js"));
      mod = load(join(projectDir, entry));
    } catch (error) {
      return Promise.resolve(report("load", error));
    }
    if (level === 2) {
      const result = report("load", null);
      result.exports = Object.keys(mod);
      return Promise.resolve(result);
    }
    const req = {
      method: "GET",
      url: "/dynamic",
      headers: { host: "127.0.0.1", accept: "text/html" },
      httpVersion: "1.1",
      socket: { encrypted: false, remoteAddress: "127.0.0.1" },
      on() {},
      once() {},
    };
    const chunks = [];
    const res = {
      statusCode: 200,
      headersSent: false,
      finished: false,
      _headers: {},
      setHeader(name, value) {
        this._headers[name.toLowerCase()] = value;
      },
      getHeader(name) {
        return this._headers[name.toLowerCase()];
      },
      removeHeader(name) {
        delete this._headers[name.toLowerCase()];
      },
      write(chunk) {
        chunks.push(String(chunk));
        return true;
      },
      end(chunk) {
        if (chunk) chunks.push(String(chunk));
        this.finished = true;
      },
      on() {},
      once() {},
    };
    return Promise.resolve()
      .then(() => mod.handler(req, res, { waitUntil: () => {} }))
      .then(
        () => {
          const result = report("handler", null);
          result.response = { status: res.statusCode, body: chunks.join("").slice(0, 200) };
          return result;
        },
        (error) => report("handler", error),
      );
  }

  globalThis.__nextHost = { run };
})();
