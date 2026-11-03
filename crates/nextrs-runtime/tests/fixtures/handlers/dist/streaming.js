// Writes chunks over time; reports whether 'close' arrived before finish.
exports.closes = [];
exports.handler = (req, res) =>
  new Promise((resolve) => {
    res.statusCode = 201;
    res.setHeader("x-chunks", "3");
    res.setHeader("set-cookie", ["a=1", "b=2"]);
    let sent = 0;
    const timer = setInterval(() => {
      res.write(`chunk-${sent}\n`);
      if (++sent === 3) {
        clearInterval(timer);
        res.end();
      }
    }, 20);
    res.on("close", () => {
      clearInterval(timer);
      globalThis.__closedEarly = (globalThis.__closedEarly ?? 0) + (res.writableFinished ? 0 : 1);
      resolve();
    });
  });
