// Probes what application code can reach. Each line is name=result.
const fs = require("node:fs");
const path = require("node:path");
exports.handler = (req, res) => {
  const probe = (name, fn) => {
    try {
      return `${name}=ok:${String(fn()).slice(0, 40)}`;
    } catch (error) {
      return `${name}=${error.code ?? error.name}`;
    }
  };
  const here = __dirname;
  const lines = [
    probe("read-own", () => fs.readFileSync(__filename, "utf8").length > 0),
    probe("read-traversal", () => fs.readFileSync(path.join(here, "../secret.txt"), "utf8")),
    probe("read-encoded-traversal", () => fs.readFileSync(`${here}/%2e%2e/secret.txt`, "utf8")),
    probe("read-etc-passwd", () => fs.readFileSync("/etc/passwd", "utf8")),
    probe("exists-outside", () => fs.existsSync("/etc/passwd")),
    probe("stat-missing-outside", () => fs.statSync("/nonexistent-nextrs-probe")),
    probe("require-outside", () => require("../secret.txt")),
    probe("write", () => fs.writeFileSync(path.join(here, "x.txt"), "x")),
    probe("env-keys", () => Object.keys(process.env).sort().join(",")),
    probe("deno-global", () => typeof globalThis.Deno),
    probe("internal-global", () => typeof globalThis.__nextrsInternal),
    probe("process-exit", () => process.exit(1)),
    probe("vm-context", () => {
      const sandbox = { input: 2 };
      require("node:vm").runInNewContext("globalThis.out = input * 21; var leaked = 1;", sandbox);
      return `${sandbox.out},${typeof globalThis.leaked},${typeof globalThis.out}`;
    }),
    probe("crypto-sha256", () => require("node:crypto").createHash("sha256").update("nextrs").digest("hex")),
    probe("crypto-hmac", () => require("node:crypto").createHmac("sha256", "key").update("nextrs").digest("base64url")),
  ];
  res.end(lines.join("\n"));
};
