exports.handler = (req, res) => {
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(globalThis.__flood?.get(req.headers["x-id"]) ?? null));
};
