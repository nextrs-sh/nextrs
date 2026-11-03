// Registers waitUntil work: "never" never settles, otherwise it records
// completion after x-delay ms for wait-until-report.js.
globalThis.__waited ??= [];
exports.handler = (req, res, ctx) => {
  const mode = req.headers["x-mode"];
  if (mode === "report") return res.end(JSON.stringify(globalThis.__waited));
  ctx.waitUntil(
    mode === "never"
      ? new Promise(() => {})
      : new Promise((resolve) => setTimeout(resolve, Number(req.headers["x-delay"] ?? 20))).then(() =>
          globalThis.__waited.push(req.headers["x-id"]),
        ),
  );
  res.end("responded");
};
