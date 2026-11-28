// RE-23B.7 scratch experiment (not part of the repository).
// x-mode: heap-single  one JS-heap object of x-mib MiB (a FixedDoubleArray)
//         heap-string  one flat one-byte string of x-mib MiB
//         external     ArrayBuffers of 32 MiB each up to x-mib MiB total (native memory)
const hold = [];
exports.handler = async (req, res) => {
  const mode = req.headers["x-mode"];
  const mib = Number(req.headers["x-mib"] ?? "0");
  if (mode === "whoami") return res.end("ok");
  const t0 = Date.now();
  try {
    if (mode === "heap-single") {
      const n = mib * 131072; // 8-byte doubles
      const big = new Array(n).fill(0.5); // as in tests' faults.js "oom-large"
      hold.push(big);
    } else if (mode === "heap-string") {
      // repeat() may build a cons string; toUpperCase() allocates one flat
      // sequential one-byte string of the full length.
      const s = "a".repeat(mib * 1024 * 1024).toUpperCase();
      hold.push(s);
    } else if (mode === "external") {
      let total = 0;
      while (total < mib) { const b = new ArrayBuffer(32 * 1024 * 1024); new Uint8Array(b).fill(1); hold.push(b); total += 32; }
    }
  } catch (e) {
    return res.end(JSON.stringify({ caught: String(e && e.name), message: String(e && e.message).slice(0, 120), ms: Date.now() - t0 }));
  }
  res.end(JSON.stringify({ ok: true, held: hold.length, ms: Date.now() - t0 }));
};
