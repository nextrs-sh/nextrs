// Phase 0 experiment: call Next.js per-route `handler(req, res, ctx)` exports
// directly from a minimal Node host, without `next start` or next-server's
// router. Records every Node built-in, file read and env var the handlers use.
//
//   node harness.cjs <next-project-dir> [port]
//
// The routing here is deliberately naive (manifest regexes only). The point is
// to measure the per-route execution contract a non-Node host must satisfy.
"use strict";

const Module = require("node:module");
const fs = require("node:fs");
const path = require("node:path");
const http = require("node:http");

const projectDir = path.resolve(process.argv[2] ?? ".");
const port = Number(process.argv[3] ?? 3100);
const trace = { builtins: new Set(), files: new Set(), env: new Set() };
let tracing = false;

const originalLoad = Module._load;
Module._load = function load(request, parent, isMain) {
  if (tracing && Module.isBuiltin(request)) trace.builtins.add(request.replace(/^node:/, ""));
  return originalLoad.call(this, request, parent, isMain);
};
for (const name of ["readFileSync", "existsSync", "statSync", "readdirSync", "openSync"]) {
  const original = fs[name];
  fs[name] = function traced(file, ...rest) {
    if (tracing && typeof file === "string") trace.files.add(path.relative(projectDir, file));
    return original.call(this, file, ...rest);
  };
}
for (const name of ["readFile", "stat", "access", "open"]) {
  const original = fs.promises[name];
  fs.promises[name] = function traced(file, ...rest) {
    if (tracing && typeof file === "string") trace.files.add(path.relative(projectDir, file));
    return original.call(this, file, ...rest);
  };
}
const realEnv = process.env;
process.env = new Proxy(realEnv, {
  get(target, key) {
    if (tracing && typeof key === "string") trace.env.add(key);
    return target[key];
  },
  // Turbopack's runtime defines properties on process.env; Node's env object
  // only accepts plain data descriptors, so forward the value directly.
  defineProperty(target, key, descriptor) {
    target[key] = descriptor.value;
    return true;
  },
});

process.chdir(projectDir);
tracing = true;
// The official adapter bootstrap: installs AsyncLocalStorage, crypto, require hooks.
require(path.join(projectDir, "node_modules/next/dist/build/adapter/setup-node-env.external.js"));

const distDir = path.join(projectDir, ".next");
const readJson = (file) => JSON.parse(fs.readFileSync(path.join(distDir, file), "utf8"));
const routes = readJson("routes-manifest.json");
const appPaths = readJson("server/app-paths-manifest.json");
const appPathRoutes = readJson("app-path-routes-manifest.json");
const entryForRoute = Object.fromEntries(
  Object.entries(appPathRoutes).map(([appPath, route]) => [route, appPaths[appPath]]),
);
const matchers = [...routes.staticRoutes, ...routes.dynamicRoutes].map((r) => ({
  page: r.page,
  regex: new RegExp(r.namedRegex),
}));

function resolveEntry(pathname) {
  for (const { page, regex } of matchers) {
    const match = regex.exec(pathname);
    if (match && entryForRoute[page]) return { page, file: entryForRoute[page] };
  }
  return null;
}

const modules = new Map();
function loadEntry(file) {
  if (!modules.has(file)) modules.set(file, require(path.join(distDir, "server", file)));
  return modules.get(file);
}

const server = http.createServer(async (req, res) => {
  const { pathname } = new URL(req.url, "http://localhost");
  const entry = resolveEntry(pathname);
  if (!entry) {
    res.statusCode = 404;
    res.end("harness: no route");
    return;
  }
  try {
    const mod = loadEntry(entry.file);
    const pending = [];
    await mod.handler(req, res, {
      waitUntil: (promise) => pending.push(promise),
      requestMeta: { relativeProjectDir: ".", distDir },
    });
    await Promise.allSettled(pending);
  } catch (error) {
    console.error(`[harness] ${pathname}:`, error);
    if (!res.headersSent) res.statusCode = 500;
    res.end("harness error");
  }
});

server.listen(port, "127.0.0.1", () => console.log(`[harness] listening on 127.0.0.1:${port}`));

function report() {
  const sorted = (set) => [...set].sort();
  const files = sorted(trace.files).filter((f) => !f.startsWith("node_modules/"));
  return {
    builtins: sorted(trace.builtins),
    projectFiles: files,
    nodeModulesFilesRead: sorted(trace.files).filter((f) => f.startsWith("node_modules/")).length,
    env: sorted(trace.env),
  };
}

process.on("SIGUSR2", () => {
  fs.writeFileSync(path.join(__dirname, "trace-report.json"), JSON.stringify(report(), null, 2));
  console.log("[harness] wrote trace-report.json");
});
