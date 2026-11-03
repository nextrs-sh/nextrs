// Streams `x-total` bytes in `x-chunk`-byte chunks as fast as the response
// takes them, honouring write()'s return value and 'drain' the way Next's
// pipeToNodeResponse does (unless x-ignore-backpressure: 1). Each chunk
// starts with its index (u32 BE) and is filled with index % 251, so the
// consumer can check order and integrity. Progress per x-id is kept for
// flood-progress.js.
globalThis.__flood ??= new Map();

function chunk(index, size) {
  const buffer = Buffer.alloc(size, index % 251);
  buffer.writeUInt32BE(index, 0);
  return buffer;
}

exports.handler = (req, res) =>
  new Promise((resolve, reject) => {
    const id = req.headers["x-id"] ?? "flood";
    const total = Number(req.headers["x-total"] ?? 1 << 20);
    const size = Number(req.headers["x-chunk"] ?? 16384);
    const ignoreBackpressure = req.headers["x-ignore-backpressure"] === "1";
    const throwAfter = Number(req.headers["x-throw-after"] ?? -1);
    const state = { produced: 0, drains: 0, closed: false, finished: false };
    globalThis.__flood.set(id, state);
    let index = 0;
    res.on("close", () => {
      state.closed = true;
      resolve();
    });
    res.setHeader("content-type", "application/octet-stream");
    function pump() {
      while (state.produced < total) {
        if (index === throwAfter) {
          reject(new Error("boom while streaming"));
          return;
        }
        const ok = res.write(chunk(index++, size));
        state.produced += size;
        if (!ok && !ignoreBackpressure) {
          state.drains++;
          res.once("drain", pump);
          return;
        }
      }
      res.end(() => {
        state.finished = true;
        resolve();
      });
    }
    pump();
  });
