exports.handler = (req, res) => res.end(String(globalThis.__closedEarly ?? 0));
