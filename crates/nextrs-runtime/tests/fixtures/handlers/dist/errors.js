exports.handler = async (req, res) => {
  const mode = req.headers["x-mode"];
  if (mode === "before-head") throw new Error("boom before head");
  if (mode === "after-head") {
    res.write("partial");
    await new Promise((resolve) => setTimeout(resolve, 1));
    throw new Error("boom after head");
  }
  if (mode === "unsupported") {
    try {
      require("node:child_process").spawn("ls");
    } catch (error) {
      return res.end(`${error.code}|${error.message}`);
    }
  }
  if (mode === "fetch") {
    return fetch("https://example.com").then(
      () => res.end("fetched"),
      (error) => res.end(`${error.code}|${error.message}`),
    );
  }
  res.end("ok");
};
