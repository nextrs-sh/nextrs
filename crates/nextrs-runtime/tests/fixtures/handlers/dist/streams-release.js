// Releasing WHATWG stream readers and writers, checked against Node. The
// same file runs under Node (`node streams-release.js`), which prints the
// reference results that tests/streams_release.rs compares against
// (streams-release.expected.json). Error messages differ between Node and
// deno_web and are not compared; types, identity (Node reuses one error
// instance per kind) and stream behavior are.
"use strict";

const settle = (promise) =>
  promise.then(
    (value) => ({ ok: true, value: value === undefined ? null : value }),
    (error) => ({ ok: false, error }),
  );
const describe = (r) => (r.ok ? { ok: true } : { ok: false, type: r.error?.constructor?.name, isTypeError: r.error instanceof TypeError });

async function run() {
  const out = {};

  // The `closed` promise of a reader released while the stream is readable,
  // and of one released after the stream closed.
  const closedOpen = [];
  for (let i = 0; i < 2; i++) {
    const stream = new ReadableStream({ start: (c) => c.enqueue(i) });
    const reader = stream.getReader();
    const closed = settle(reader.closed);
    reader.releaseLock();
    closedOpen.push(await closed);
  }
  const closedDone = [];
  for (let i = 0; i < 2; i++) {
    const stream = new ReadableStream({ start: (c) => c.close() });
    const reader = stream.getReader();
    await reader.closed;
    reader.releaseLock();
    closedDone.push(await settle(reader.closed));
  }
  out.closedWhileReadable = closedOpen.map(describe);
  out.closedAfterClose = closedDone.map(describe);
  out.closedSameAcrossStreams = closedOpen[0].error === closedOpen[1].error;
  out.closedSameReadableAndClosed = closedOpen[0].error === closedDone[0].error;

  // Pending reads when the reader is released (default and BYOB readers).
  const pending = [];
  for (let i = 0; i < 2; i++) {
    const reader = new ReadableStream().getReader();
    const read = settle(reader.read());
    reader.releaseLock();
    pending.push(await read);
  }
  const byob = [];
  for (let i = 0; i < 2; i++) {
    const reader = new ReadableStream({ type: "bytes" }).getReader({ mode: "byob" });
    const read = settle(reader.read(new Uint8Array(4)));
    reader.releaseLock();
    byob.push(await read);
  }
  out.pendingRead = pending.map(describe);
  out.pendingByobRead = byob.map(describe);
  out.pendingSameAcrossStreams = pending[0].error === pending[1].error;
  out.pendingSameDefaultAndByob = pending[0].error === byob[0].error;
  out.pendingDiffersFromClosed = pending[0].error !== closedOpen[0].error;

  // Writer release: `ready` and `closed` reject with one error.
  const writers = [];
  for (let i = 0; i < 2; i++) {
    const writer = new WritableStream().getWriter();
    const closed = settle(writer.closed);
    writer.releaseLock();
    writers.push({ closed: await closed, ready: await settle(writer.ready) });
  }
  out.writerClosed = writers.map((w) => describe(w.closed));
  out.writerReady = writers.map((w) => describe(w.ready));
  out.writerReadyIsClosed = writers[0].ready.error === writers[0].closed.error;
  out.writerSameAcrossStreams = writers[0].closed.error === writers[1].closed.error;

  // Using a released reader still fails, and the stream can be read again.
  {
    const stream = new ReadableStream({ start: (c) => (c.enqueue("a"), c.enqueue("b"), c.close()) });
    const first = stream.getReader();
    out.firstRead = await first.read();
    first.releaseLock();
    out.readAfterRelease = describe(await settle(first.read()));
    const second = stream.getReader();
    out.secondReader = [await second.read(), await second.read()];
  }

  // Async iteration: full consumption, then early exit (cancels the source).
  {
    const chunks = [];
    for await (const chunk of new ReadableStream({ start: (c) => (c.enqueue(1), c.enqueue(2), c.close()) })) chunks.push(chunk);
    out.iterated = chunks;
    let cancelled = null;
    const stream = new ReadableStream({ start: (c) => (c.enqueue(1), c.enqueue(2)), cancel: (reason) => { cancelled = reason === undefined ? "undefined" : String(reason); } });
    for await (const chunk of stream) {
      out.earlyFirst = chunk;
      break;
    }
    out.earlyCancelled = cancelled;
    out.lockedAfterBreak = stream.locked;
    const again = await settle(stream.getReader().read());
    out.readAfterBreak = again.ok ? again.value : describe(again);
  }

  // pipeThrough / pipeTo release their locks at the end.
  {
    const source = new ReadableStream({ start: (c) => (c.enqueue("x"), c.enqueue("y"), c.close()) });
    const upper = source.pipeThrough(new TransformStream({ transform: (chunk, c) => c.enqueue(chunk.toUpperCase()) }));
    const got = [];
    await upper.pipeTo(new WritableStream({ write: (chunk) => { got.push(chunk); } }));
    out.piped = got;
    out.sourceLockedAfterPipe = source.locked;
    out.upperLockedAfterPipe = upper.locked;
  }

  // tee: both branches see every chunk.
  {
    const [a, b] = new ReadableStream({ start: (c) => (c.enqueue(1), c.enqueue(2), c.close()) }).tee();
    const all = async (s) => { const r = []; for await (const x of s) r.push(x); return r; };
    out.tee = await Promise.all([all(a), all(b)]);
  }

  // A Response body read to the end (the path Next's renderers take).
  out.responseText = await new Response(new ReadableStream({ start: (c) => (c.enqueue(new TextEncoder().encode("hé")), c.close()) })).text();

  // Many releases: still one instance, and no unhandled rejections.
  {
    const seen = new Set();
    for (let i = 0; i < 200; i++) {
      const reader = new ReadableStream().getReader();
      const closed = settle(reader.closed);
      reader.releaseLock();
      seen.add((await closed).error);
    }
    out.distinctErrorsOver200Releases = seen.size;
  }
  return out;
}

exports.run = run;
exports.handler = async (req, res) => {
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(await run()));
};

if (require.main === module) {
  run().then((r) => console.log(JSON.stringify(r, null, 1)));
}
