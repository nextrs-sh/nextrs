// Reads the request's AsyncLocalStorage store after every kind of async
// boundary the runtime schedules, and reports what it saw.
const { AsyncLocalStorage, AsyncResource } = require("node:async_hooks");
const { Readable, PassThrough } = require("node:stream");
const { EventEmitter } = require("node:events");
const timersPromises = require("node:timers/promises");

const storage = new AsyncLocalStorage();
// A resource created outside any request must not see request stores.
const outside = new AsyncResource("outside");

exports.handler = (req, res) =>
  storage.run({ id: req.headers["x-id"] }, async () => {
    const seen = [];
    const check = (label) => seen.push(`${label}=${storage.getStore()?.id}`);
    const delay = Number(req.headers["x-delay"] ?? 0);

    check("sync");
    await new Promise((resolve) => setTimeout(resolve, delay));
    check("setTimeout");
    await new Promise((resolve) => setImmediate(resolve));
    check("setImmediate");
    await new Promise((resolve) => process.nextTick(resolve));
    check("nextTick");
    await new Promise((resolve) => queueMicrotask(resolve));
    check("queueMicrotask");
    await timersPromises.setTimeout(delay % 7);
    check("timers/promises");
    await new Promise((resolve) => {
      const interval = setInterval(() => {
        check("setInterval");
        clearInterval(interval);
        resolve();
      }, 1);
    });
    for await (const _chunk of Readable.from(["a", "b"])) check("node-stream");
    const through = new PassThrough();
    const done = new Promise((resolve) => through.on("data", () => { check("stream-event"); resolve(); }));
    setTimeout(() => through.write("x"), delay % 5);
    await done;
    const reader = new ReadableStream({
      async pull(controller) {
        await new Promise((resolve) => setTimeout(resolve, 1));
        controller.enqueue("chunk");
        controller.close();
      },
    }).getReader();
    await reader.read();
    check("web-stream");
    const emitter = new EventEmitter();
    setTimeout(() => emitter.emit("ping"), 1);
    await new Promise((resolve) => emitter.once("ping", resolve));
    check("event-emitter");
    const snapshot = AsyncLocalStorage.snapshot();
    seen.push(`outside-resource=${outside.runInAsyncScope(() => storage.getStore()?.id)}`);
    seen.push(`snapshot=${await new Promise((resolve) => setTimeout(() => resolve(snapshot(() => storage.getStore()?.id)), 1))}`);
    seen.push(`exit=${storage.exit(() => storage.getStore()?.id)}`);
    try {
      await Promise.reject(new Error("expected"));
    } catch {
      check("after-rejection");
    }
    res.setHeader("content-type", "text/plain");
    res.end(seen.join("\n"));
  });
