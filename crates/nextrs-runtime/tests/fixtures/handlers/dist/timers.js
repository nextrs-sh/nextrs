// Runs timer-probe.js inside the isolate and answers its JSON (Phase 3C
// timer analysis). x-iterations sets the per-case sample count.
const probe = require("./timer-probe.js");

exports.handler = async (req, res) => {
  const n = Number(req.headers["x-iterations"] ?? 400);
  const result = await probe.run({ n });
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(result));
};
