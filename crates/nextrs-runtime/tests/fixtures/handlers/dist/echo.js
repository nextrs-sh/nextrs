// Consumes the request body incrementally and answers with what arrived:
// byte count, number of reads, SHA-256 and timing. x-stream-back: 1 streams
// the body back instead, chunk by chunk, as it is read.
const { createHash } = require("node:crypto");

exports.handler = async (req, res) => {
  if (req.headers["x-skip-body"] === "1") return res.end("ignored body");
  if (req.headers["x-stream-back"] === "1") {
    res.setHeader("content-type", "application/octet-stream");
    for await (const chunk of req) {
      if (!res.write(chunk)) await new Promise((resolve) => res.once("drain", resolve));
    }
    return res.end();
  }
  const hash = createHash("sha256");
  let bytes = 0;
  let reads = 0;
  try {
    for await (const chunk of req) {
      hash.update(chunk);
      bytes += chunk.length;
      reads++;
    }
  } catch (error) {
    res.statusCode = error.status ?? 400;
    return res.end(JSON.stringify({ error: error.code, message: error.message, aborted: req.aborted, bytes }));
  }
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify({ bytes, reads, sha256: hash.digest("hex"), complete: req.complete }));
};
