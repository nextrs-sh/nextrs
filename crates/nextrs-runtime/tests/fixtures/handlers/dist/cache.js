// Drives the cache handler NextRS offers to Next.js
// (globalThis[Symbol.for("@next/cache-handlers")].FetchCache) the way
// IncrementalCache does, for tests/cache.rs. ?op=set|get|revalidate|
// handler|read-file, with key/value/tags/kind/expire/path parameters.
globalThis.__isolateId ??= Math.random().toString(36).slice(2);
const fs = require("node:fs");

exports.handler = async (req, res) => {
  const p = new URL(req.url, "http://localhost").searchParams;
  const Handler = globalThis[Symbol.for("@next/cache-handlers")]?.FetchCache;
  const tags = (p.get("tags") ?? "").split(",").filter(Boolean);
  const out = { isolate: globalThis.__isolateId };
  try {
    switch (p.get("op")) {
      case "handler":
        out.installed = typeof Handler === "function";
        break;
      case "set": {
        const kind = p.get("kind") ?? "FETCH";
        const data =
          p.get("value") === null
            ? null
            : { kind, data: { headers: {}, body: p.get("value"), status: 200, url: "" }, revalidate: 60 };
        await new Handler({}).set(p.get("key"), data, { fetchCache: true, tags });
        break;
      }
      case "get": {
        const entry = await new Handler({}).get(p.get("key"), {
          kind: p.get("kind") ?? "FETCH",
          tags,
          softTags: [],
        });
        out.entry = entry && { body: entry.value.data.body, lastModified: entry.lastModified };
        break;
      }
      case "revalidate": {
        const expire = p.get("expire");
        await new Handler({}).revalidateTag(tags, expire === null ? undefined : { expire: Number(expire) });
        break;
      }
      case "read-file":
        out.content = fs.readFileSync(p.get("path"), "utf8").slice(0, 20);
        break;
    }
  } catch (error) {
    out.error = { code: error.code ?? null, message: String(error.message) };
  }
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(out));
};
