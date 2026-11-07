// A deliberately broken HTTP proxy, for negative controls: put in front of
// NextRS as the suites' candidate, it introduces one defect of the kind the
// suites exist to catch, and the suite must then fail. Test-only; nothing
// of it is in NextRS.
//
//   node tests/compat/negative-proxy.mjs --listen 5800 --target http://127.0.0.1:5801 --mode M
//
// Modes:
//   replay-post        every POST is sent twice (a repeated mutation); the
//                      client gets the first answer
//   drop-action        a Server Action request (Next-Action header) is answered
//                      200 with an empty RSC payload, never forwarded (an action
//                      that did not run)
//   strip-credentials  Cookie and Authorization are removed (authorization
//                      skipped: every user anonymous)
//   swap-identity      u-alice and u-bob are exchanged in response bodies (a
//                      user sees another's data)
//   strip-set-cookie   Set-Cookie is removed from responses (a session never set)
//   wrong-location     every Location header points to /elsewhere (a redirect
//                      to the wrong place)
import http from "node:http";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const target = new URL(args.target);
const mode = args.mode;
const MODES = ["replay-post", "drop-action", "strip-credentials", "swap-identity", "strip-set-cookie", "wrong-location"];
if (!MODES.includes(mode)) {
  console.error(`--mode must be one of ${MODES.join(", ")}`);
  process.exit(2);
}
const agent = new http.Agent({ keepAlive: true, maxSockets: 256 });
let defects = 0;

function forward(req, body) {
  return new Promise((resolve, reject) => {
    // The client's own Host: origin checks and absolute URLs then name the
    // address the suite talks to, as they would without this proxy.
    const headers = { ...req.headers };
    if (mode === "strip-credentials" && (headers.cookie || headers.authorization)) {
      delete headers.cookie;
      delete headers.authorization;
      defects++;
    }
    const out = http.request({ host: target.hostname, port: target.port, method: req.method, path: req.url, headers, agent }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
      res.on("error", reject);
    });
    out.on("error", reject);
    out.end(body);
  });
}

http
  .createServer(async (req, res) => {
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const body = Buffer.concat(chunks);
    try {
      if (mode === "drop-action" && req.method === "POST" && req.headers["next-action"]) {
        defects++;
        res.writeHead(200, { "content-type": "text/x-component" });
        return res.end('0:{"a":"$@1"}\n1:null\n');
      }
      const answer = await forward(req, body);
      if (mode === "replay-post" && req.method === "POST") {
        defects++;
        await forward(req, body).catch(() => {});
      }
      const headers = { ...answer.headers };
      delete headers["content-length"];
      delete headers["transfer-encoding"];
      delete headers.connection;
      delete headers["keep-alive"];
      let out = answer.body;
      if (mode === "strip-set-cookie" && headers["set-cookie"]) {
        delete headers["set-cookie"];
        defects++;
      }
      if (mode === "wrong-location" && headers.location) {
        headers.location = "/elsewhere";
        defects++;
      }
      if (mode === "swap-identity" && !headers["content-encoding"]) {
        const text = out.toString("latin1");
        if (/u-(alice|bob)/.test(text)) {
          defects++;
          out = Buffer.from(text.replace(/u-(alice|bob)/g, (_, who) => (who === "alice" ? "u-bob" : "u-alice")), "latin1");
        }
      }
      headers["content-length"] = String(out.length);
      res.writeHead(answer.status, headers);
      res.end(req.method === "HEAD" ? undefined : out);
    } catch (error) {
      res.writeHead(502, { "content-type": "text/plain" });
      res.end(`negative proxy: ${error.message}`);
    }
  })
  .listen(Number(args.listen), "127.0.0.1", () => console.log(`negative proxy (${mode}) on ${args.listen} -> ${target.href}`));

process.on("SIGTERM", () => {
  console.log(`defects introduced: ${defects}`);
  process.exit(0);
});
