// Heap-limit faults (RE-23B.7, kept for RE-4E). x-mode:
//   heap-single       one JS-heap object of x-mib MiB (a FixedDoubleArray)
//   heap-string       one flat one-byte string of x-mib MiB
//   external          32 MiB ArrayBuffers up to x-mib MiB (outside the heap)
//   refuse-then-grow  one ArrayBuffer the external budget refuses (caught),
//                     then the heap grown by small objects until its limit
//   retain-then-refuse  keep x-retain-mib MiB of small objects, then one
//                     x-mib ArrayBuffer (the external budget refuses it)
//   whoami            "ok"
const hold = [];

exports.handler = async (req, res) => {
  const mode = req.headers["x-mode"];
  const mib = Number(req.headers["x-mib"] ?? "0");
  if (mode === "whoami") return res.end("ok");
  const started = Date.now();
  let refused = null;
  try {
    if (mode === "heap-single") {
      hold.push(new Array(mib * 131072).fill(0.5));
    } else if (mode === "heap-string") {
      // repeat() may build a cons string; toUpperCase() allocates one flat
      // sequential one-byte string of the full length.
      hold.push("a".repeat(mib * 1024 * 1024).toUpperCase());
    } else if (mode === "external") {
      for (let total = 0; total < mib; total += 32) {
        const buffer = new ArrayBuffer(32 * 1024 * 1024);
        new Uint8Array(buffer).fill(1);
        hold.push(buffer);
      }
    } else if (mode === "retain-then-refuse") {
      const retain = Number(req.headers["x-retain-mib"] ?? "0");
      // About 1 MiB per 128 arrays of 1024 doubles.
      for (let i = 0; i < retain * 128; i++) hold.push(new Array(1024).fill(i + 0.5));
      hold.push(new ArrayBuffer(mib * 1024 * 1024));
    } else if (mode === "refuse-then-grow") {
      try {
        hold.push(new ArrayBuffer(mib * 1024 * 1024));
      } catch (error) {
        refused = error.name;
      }
      for (;;) hold.push(new Array(1024).fill({ refused }));
    }
  } catch (error) {
    return res.end(
      JSON.stringify({ caught: String(error?.name), refused, ms: Date.now() - started }),
    );
  }
  res.end(JSON.stringify({ ok: true, refused, held: hold.length, ms: Date.now() - started }));
};
