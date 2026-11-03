// Engine-agnostic test drivers. The host provides: __report(string),
// setTimeout/clearTimeout, console.log, and for `next` the __host fs API.
(function () {
  const globalsOfInterest = [
    "process", "require", "module", "Buffer", "setTimeout", "setImmediate", "queueMicrotask",
    "TextEncoder", "TextDecoder", "ReadableStream", "URL", "URLSearchParams", "Headers",
    "Request", "Response", "fetch", "crypto", "AbortController", "structuredClone",
    "AsyncLocalStorage", "performance", "WebAssembly", "SharedArrayBuffer", "Atomics",
  ];

  async function hello() {
    const globals = {};
    for (const name of globalsOfInterest) globals[name] = typeof globalThis[name] !== "undefined";
    return { value: 6 * 7, globals };
  }

  async function asyncTimers() {
    const start = Date.now();
    await new Promise((resolve) => setTimeout(resolve, 20));
    const waited = Date.now() - start;
    let fired = 0;
    await Promise.all(
      Array.from({ length: 100 }, (_, i) => new Promise((r) => setTimeout(() => (fired++, r()), i % 10))),
    );
    return { waitedMs: waited, timersFired: fired, ok: waited >= 19 && fired === 100 };
  }

  async function react() {
    const ssr = globalThis.NextrsReactSSR;
    const t0 = Date.now();
    const sync = ssr.renderSync();
    const t1 = Date.now();
    const streamed = await ssr.renderStream();
    const t2 = Date.now();
    const markers = {
      title: streamed.html.includes('<h1 id="title">Dynamic server component</h1>'),
      fallbackInShell: streamed.html.includes("Loading…"),
      streamedBoundary: streamed.html.includes("Streamed after Suspense boundary"),
    };
    return {
      ok: Object.values(markers).every(Boolean) && streamed.chunks > 1,
      renderToStringBytes: sync.length,
      renderToStringMs: t1 - t0,
      streamChunks: streamed.chunks,
      streamBytes: streamed.html.length,
      streamMs: t2 - t1,
      markers,
      head: streamed.html.slice(0, 120),
    };
  }

  async function rsc() {
    const result = await globalThis.NextrsFlight.renderFlight();
    return {
      ok: result.flight.includes("Flight payload") && result.flight.includes("El Filibusterismo"),
      chunks: result.chunks,
      bytes: result.flight.length,
      flight: result.flight.slice(0, 400),
    };
  }

  async function next(projectDir, entry, level) {
    return globalThis.__nextHost.run(projectDir, entry, level);
  }

  globalThis.__tests = { hello, async: asyncTimers, react, rsc, next };
  globalThis.__runTest = function (name, args) {
    Promise.resolve()
      .then(() => globalThis.__tests[name](...args))
      .then(
        (result) => globalThis.__report(JSON.stringify(Object.assign({ ok: true }, result))),
        (error) =>
          globalThis.__report(
            JSON.stringify({ ok: false, error: String((error && error.stack) || error) }),
          ),
      );
  };
})();
