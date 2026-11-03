// Ways for JavaScript to take its isolate down, or to hang (x-mode).
// Also reports this isolate's identity, so tests can tell isolates and
// generations apart: x-mode "whoami".
globalThis.__isolateId ??= Math.random().toString(36).slice(2);
globalThis.__served ??= 0;

exports.handler = async (req, res) => {
  const mode = req.headers["x-mode"];
  globalThis.__served++;
  switch (mode) {
    case "whoami": {
      const delay = Number(req.headers["x-delay"] ?? 0);
      if (delay) await new Promise((resolve) => setTimeout(resolve, delay));
      return res.end(`${globalThis.__isolateId}:${globalThis.__served}`);
    }
    // Node exits on these when nothing listens; so does the isolate.
    case "uncaught":
      setTimeout(() => {
        throw new Error("uncaught from a timer");
      }, 1);
      return res.end("scheduled");
    case "rejection":
      Promise.reject(new Error("unhandled rejection"));
      return res.end("scheduled");
    // CPU-bound: only the stall watchdog stops these.
    case "loop":
      for (;;) {}
    case "microtask-loop":
      for (;;) await null;
    case "oom": {
      const hoard = [];
      for (;;) hoard.push(new Array(100000).fill(Math.random()));
    }
    // Never settles; only the request deadline ends it.
    case "never":
      return new Promise(() => {});
    case "never-after-head":
      res.writeHead(200, { "content-type": "text/plain" });
      res.write("started");
      return new Promise(() => {});
    case "slow": {
      await new Promise((resolve) => setTimeout(resolve, Number(req.headers["x-delay"] ?? 200)));
      return res.end("slow done");
    }
    case "panic":
      globalThis.__nextrsFault("panic");
      return new Promise(() => {});
    case "drop-responses":
      res.write("before");
      await new Promise((resolve) => setTimeout(resolve, 5));
      globalThis.__nextrsFault("drop-responses");
      return new Promise(() => {});
    default:
      return res.end("ok");
  }
};
