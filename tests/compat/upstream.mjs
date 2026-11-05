// A deterministic upstream API for the data-fetching fixtures: both servers
// under comparison fetch from it, and it counts hits per key, which is how
// the suites tell a cached fetch from an uncached one.
//
//   node tests/compat/upstream.mjs --port 4999
//
//   GET /data?key=K[&delay=ms][&status=N]  -> {"key","hit","auth","cookie"}
//       hit counts requests for K (1 for the first); auth/cookie echo the
//       request's authorization and cookie headers.
//   GET /hits?key=K                         -> {"key","hits","aborted"}
//       aborted counts /data responses whose client left before the end.
//   GET /stream?chunks=N&gap=ms             -> N lines, one every gap ms
//   POST /mutations?key=K&id=I              -> records one side effect ({"key","count"})
//   GET /mutations?key=K                    -> {"key","ids"} in arrival order
//   GET /flag?name=N, POST /flag?name=N&on=1 -> {"name","on"}: switches the
//       fixtures read (an ISR page fails its regeneration while one is on)
//       The Server Action suites count side effects here, outside every
//       isolate, to tell an executed mutation from a replayed or lost one.
import http from "node:http";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const port = Number(args.port ?? 4999);
const hits = new Map();
const aborted = new Map();
const bump = (map, key) => map.set(key, (map.get(key) ?? 0) + 1);
const mutations = new Map();
const flags = new Map();

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, "http://upstream");
  const p = url.searchParams;
  const key = p.get("key") ?? "default";
  if (url.pathname === "/data") {
    bump(hits, key);
    const hit = hits.get(key);
    const delay = Number(p.get("delay") ?? 0);
    let closed = false;
    res.on("close", () => {
      if (!res.writableFinished) {
        closed = true;
        bump(aborted, key);
      }
    });
    if (delay) await new Promise((resolve) => setTimeout(resolve, delay));
    if (closed) return;
    const body = JSON.stringify({
      key,
      hit,
      auth: req.headers.authorization ?? null,
      cookie: req.headers.cookie ?? null,
    });
    res.writeHead(Number(p.get("status") ?? 200), { "content-type": "application/json" });
    res.end(body);
  } else if (url.pathname === "/hits") {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ key, hits: hits.get(key) ?? 0, aborted: aborted.get(key) ?? 0 }));
  } else if (url.pathname === "/mutations") {
    res.writeHead(200, { "content-type": "application/json" });
    if (req.method === "POST") {
      const ids = mutations.get(key) ?? [];
      ids.push(p.get("id") ?? "");
      mutations.set(key, ids);
      // Only the count: the ledger grows without bound under benchmarks,
      // and nothing reads it from the POST.
      res.end(JSON.stringify({ key, count: ids.length }));
      return;
    }
    res.end(JSON.stringify({ key, ids: mutations.get(key) ?? [] }));
  } else if (url.pathname === "/flag") {
    if (req.method === "POST") flags.set(p.get("name") ?? "", p.get("on") === "1");
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ name: p.get("name"), on: flags.get(p.get("name") ?? "") ?? false }));
  } else if (url.pathname === "/stream") {
    const chunks = Number(p.get("chunks") ?? 5);
    const gap = Number(p.get("gap") ?? 50);
    res.writeHead(200, { "content-type": "text/plain" });
    for (let i = 0; i < chunks && !res.destroyed; i++) {
      res.write(`line ${i}\n`);
      await new Promise((resolve) => setTimeout(resolve, gap));
    }
    res.end();
  } else {
    res.writeHead(404).end();
  }
});
server.keepAliveTimeout = 5000;
server.listen(port, "127.0.0.1", () => console.log(`upstream listening on ${port}`));
