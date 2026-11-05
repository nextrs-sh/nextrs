// Drives the cache handler NextRS offers to Next.js
// (globalThis[Symbol.for("@next/cache-handlers")].FetchCache) the way
// IncrementalCache does, for tests/cache.rs. ?op=set|get|revalidate|
// handler|read-file|set-page|get-page|lease, with key/value/tags/kind/
// expire/path parameters.
globalThis.__isolateId ??= Math.random().toString(36).slice(2);
const fs = require("node:fs");

exports.handler = async (req, res) => {
  const p = new URL(req.url, "http://localhost").searchParams;
  // Barrier for the write-visibility test: the first chunk is sent after
  // set() was called but before its promise (the disk write) settled; the
  // response ends once it has.
  if (p.get("op") === "set-then-answer") {
    const Handler = globalThis[Symbol.for("@next/cache-handlers")].FetchCache;
    const data = { kind: "FETCH", data: { headers: {}, body: p.get("value"), status: 200, url: "" }, revalidate: 60 };
    const pending = new Handler({}).set(p.get("key"), data, { fetchCache: true, tags: [] });
    res.setHeader("x-isolate", globalThis.__isolateId);
    res.write("set-called\n");
    await new Promise((resolve) => setTimeout(resolve, Number(p.get("hold") ?? 0)));
    await pending;
    res.end("persisted\n");
    return;
  }
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
      // Response-cache (ISR) entries, as ResponseCache stores an APP_PAGE.
      case "set-page": {
        const value = {
          kind: "APP_PAGE",
          html: p.get("value"),
          rscData: Buffer.from(`rsc:${p.get("value")}`),
          headers: { "x-next-cache-tags": tags.join(",") },
          status: 200,
          segmentData: new Map([["/_tree", Buffer.from([0, 255, 1])]]),
        };
        await new Handler({}).set(p.get("key"), value, { kind: "APP_PAGE", cacheControl: { revalidate: 60, expire: 600 } });
        break;
      }
      case "get-page": {
        const entry = await new Handler({}).get(p.get("key"), { kind: "APP_PAGE", isFallback: false });
        out.entry = entry && {
          html: entry.value.html,
          rsc: entry.value.rscData.toString(),
          segment: [...entry.value.segmentData.get("/_tree")],
          revalidate: entry.cacheControl?.revalidate,
          lastModified: entry.lastModified,
        };
        break;
      }
      // Regeneration single flight: take the lease for key, hold it ms.
      case "lease": {
        const handler = new Handler({});
        const started = Date.now();
        const entry = await handler.get(p.get("key"), { kind: "APP_PAGE", isFallback: false });
        out.waitedMs = Date.now() - started;
        out.entry = entry && { html: entry.value.html };
        if (!entry && p.get("then-set")) {
          await new Promise((resolve) => setTimeout(resolve, Number(p.get("hold") ?? 0)));
          await handler.set(p.get("key"), { kind: "APP_PAGE", html: p.get("then-set"), headers: {}, status: 200 }, { kind: "APP_PAGE", cacheControl: { revalidate: 60 } });
        }
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
