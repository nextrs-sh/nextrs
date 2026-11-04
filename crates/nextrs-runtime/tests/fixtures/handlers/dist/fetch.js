// Outbound fetch scenarios for tests/fetch.rs. ?case=<name>&target=<url>
// (an upstream started by the test). Answers JSON; failures as
// { error: { name, message, code } } with status 200 so the test sees them.
const { AsyncLocalStorage } = require("node:async_hooks");

const als = new AsyncLocalStorage();
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function describe(error) {
  return {
    name: error?.name ?? null,
    message: String(error?.message ?? error),
    code: error?.cause?.code ?? error?.code ?? null,
    cause: error?.cause ? String(error.cause.message) : null,
  };
}

async function body(stream, limit = Infinity) {
  const reader = stream.getReader();
  let length = 0;
  let chunks = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    chunks += 1;
    if (length >= limit) {
      await reader.cancel();
      break;
    }
  }
  return { length, chunks };
}

const cases = {
  async get(p) {
    const r = await fetch(p.get("target"));
    return {
      status: r.status,
      ok: r.ok,
      statusText: r.statusText,
      type: r.type,
      url: r.url,
      redirected: r.redirected,
      contentType: r.headers.get("content-type"),
      contentEncoding: r.headers.get("content-encoding"),
      contentLength: r.headers.get("content-length"),
      acceptEncodingSent: r.headers.get("x-accept-encoding"),
      headersImmutable: (() => {
        try {
          r.headers.set("x", "y");
          return false;
        } catch {
          return true;
        }
      })(),
      body: await r.text(),
    };
  },
  async method(p) {
    const init = { method: p.get("method"), headers: { "x-probe": "1", "content-type": "text/plain" } };
    if (p.has("body")) init.body = p.get("body");
    const r = await fetch(p.get("target"), init);
    return { status: r.status, echo: r.status === 200 && p.get("method") !== "HEAD" ? await r.json() : null };
  },
  async init(p) {
    // Request headers a caller sets, and Node's defaults.
    const r = await fetch(p.get("target"), { headers: JSON.parse(p.get("headers") ?? "{}") });
    return await r.json();
  },
  async redirect(p) {
    const init = { redirect: p.get("mode") ?? "follow", method: p.get("method") ?? "GET" };
    if (p.has("body")) init.body = p.get("body");
    if (p.has("headers")) init.headers = JSON.parse(p.get("headers"));
    if (p.get("stream") === "1") {
      init.body = new ReadableStream({
        start(c) {
          c.enqueue(new TextEncoder().encode("streamed"));
          c.close();
        },
      });
      init.duplex = "half";
    }
    const r = await fetch(p.get("target"), init);
    const text = await r.text();
    return {
      status: r.status,
      url: r.url,
      redirected: r.redirected,
      location: r.headers.get("location"),
      echo: r.headers.get("content-type") === "application/json" ? JSON.parse(text) : text,
    };
  },
  async abort(p) {
    const controller = new AbortController();
    setTimeout(() => controller.abort(), Number(p.get("ms") ?? 50));
    const started = Date.now();
    try {
      await fetch(p.get("target"), { signal: controller.signal });
      return { aborted: false };
    } catch (error) {
      return { aborted: true, ms: Date.now() - started, error: describe(error) };
    }
  },
  async "abort-before"(p) {
    const controller = new AbortController();
    controller.abort(new Error("custom reason"));
    try {
      await fetch(p.get("target"), { signal: controller.signal });
      return { aborted: false };
    } catch (error) {
      return { aborted: true, error: describe(error) };
    }
  },
  async "abort-body"(p) {
    const controller = new AbortController();
    const r = await fetch(p.get("target"), { signal: controller.signal });
    const reader = r.body.getReader();
    const first = await reader.read();
    controller.abort();
    try {
      await reader.read();
      return { first: first.value.byteLength, errored: false };
    } catch (error) {
      return { first: first.value.byteLength, errored: true, error: describe(error) };
    }
  },
  async "cancel-body"(p) {
    const r = await fetch(p.get("target"));
    const reader = r.body.getReader();
    const first = await reader.read();
    await reader.cancel();
    return { first: first.value.byteLength };
  },
  async "read-all"(p) {
    const r = await fetch(p.get("target"));
    return { status: r.status, ...(await body(r.body)) };
  },
  async "no-read"(p) {
    // The body is left unread; the request's end releases it.
    const r = await fetch(p.get("target"));
    return { status: r.status };
  },
  async orphan(p) {
    // Not awaited: aborted when the request ends.
    fetch(p.get("target")).then(
      (r) => r.arrayBuffer(),
      () => {},
    ).catch(() => {});
    await sleep(Number(p.get("ms") ?? 100));
    return { started: true };
  },
  async "slow-reader"(p) {
    // Reads one chunk, then waits: the upstream must stall, not buffer.
    const r = await fetch(p.get("target"));
    const reader = r.body.getReader();
    await reader.read();
    await sleep(Number(p.get("ms") ?? 1000));
    await reader.cancel();
    return { done: true };
  },
  async upload(p) {
    const chunks = Number(p.get("chunks") ?? 8);
    const size = Number(p.get("size") ?? 65536);
    let sent = 0;
    const stream = new ReadableStream({
      pull(c) {
        if (sent === chunks) return c.close();
        c.enqueue(new Uint8Array(size).fill(sent % 251));
        sent += 1;
      },
    });
    const r = await fetch(p.get("target"), { method: "POST", body: stream, duplex: "half" });
    return { status: r.status, echo: await r.json() };
  },
  async context(p, req) {
    const id = req.headers["x-id"];
    return als.run({ id }, async () => {
      const r = await fetch(`${p.get("target")}?v=${id}&delay=${Number(p.get("delay") ?? 0)}`);
      const text = await r.text();
      await sleep(1);
      return { id, text, store: als.getStore()?.id ?? null, header: req.headers["x-id"] };
    });
  },
  async "forbidden-header"(p) {
    await fetch(p.get("target"), { headers: { [p.get("name")]: "x" } });
    return { sent: true };
  },
  async "no-await"(p) {
    // Starts many fetches at once and reads them all.
    const n = Number(p.get("n") ?? 20);
    const results = await Promise.all(
      Array.from({ length: n }, (_, i) => fetch(`${p.get("target")}?v=${i}`).then((r) => r.text())),
    );
    return { all: results.every((text, i) => text === String(i)), n };
  },
};

exports.handler = async (req, res) => {
  const params = new URL(req.url, "http://localhost").searchParams;
  const name = params.get("case");
  if (name === "stream") {
    // Upstream chunks are forwarded as they arrive.
    const r = await fetch(params.get("target"));
    res.writeHead(r.status, { "content-type": "application/octet-stream" });
    for await (const chunk of r.body) {
      if (!res.write(chunk)) await new Promise((resolve) => res.once("drain", resolve));
    }
    res.end();
    return;
  }
  let result;
  try {
    result = await cases[name](params, req);
  } catch (error) {
    result = { error: describe(error) };
  }
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(result));
};
