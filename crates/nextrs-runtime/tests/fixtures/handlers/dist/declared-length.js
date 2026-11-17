// Declares its length, writes all of it, and ends on a later tick (as
// Node-style res.end(data) does). x-mode "report" answers what the last
// such request saw when it ended: "ok", or "destroyed" if it was told the
// client had gone.
globalThis.__declared ??= "none";

exports.handler = async (req, res) => {
  if (req.headers["x-mode"] === "report") return res.end(globalThis.__declared);
  globalThis.__declared = "pending";
  res.setHeader("content-length", "5");
  res.write("hello");
  await new Promise((resolve) => setTimeout(resolve, 30));
  globalThis.__declared = res.destroyed ? "destroyed" : "ok";
  res.end();
};
