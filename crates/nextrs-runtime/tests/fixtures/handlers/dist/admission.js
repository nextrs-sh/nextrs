// Memory-aware admission tests (RE-4B). Counts executions per x-id
// (x-report: 1 answers the count instead of running), optionally holds
// x-retain-mib MiB of heap in a global (1 MiB arrays of doubles) and
// waits x-delay ms (in 50 ms steps) before answering. x-log appends x-id to a shared
// order log; x-report-order: 1 answers that log.
globalThis.__admission ??= { runs: new Map(), retained: [], order: [] };

exports.handler = async (req, res) => {
  const state = globalThis.__admission;
  const id = req.headers["x-id"] ?? "anonymous";
  if (req.headers["x-report"] === "1") {
    res.end(JSON.stringify(state.runs.get(id) ?? 0));
    return;
  }
  if (req.headers["x-report-order"] === "1") {
    res.end(JSON.stringify(state.order));
    return;
  }
  state.runs.set(id, (state.runs.get(id) ?? 0) + 1);
  if (req.headers["x-log"] === "1") state.order.push(id);
  const retain = Number(req.headers["x-retain-mib"] ?? 0);
  for (let i = 0; i < retain; i++) state.retained.push(new Array(131072).fill(0.5));
  const delay = Number(req.headers["x-delay"] ?? 0);
  // In 50 ms steps, so the event loop turns (and samples the heap).
  for (let waited = 0; waited < delay; waited += 50) {
    await new Promise((resolve) => setTimeout(resolve, Math.min(50, delay - waited)));
  }
  res.end("ok");
};
