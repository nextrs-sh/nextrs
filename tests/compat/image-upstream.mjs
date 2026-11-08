// A deterministic image upstream for the remote-image scenarios: no public
// hosts. Serves the fixture's own images under /img/ with scripted
// behaviours (redirects, slow heads and bodies, oversized and compressed
// bodies, wrong types) and counts every request by path, so a suite can
// assert that a forbidden source was never fetched.
//
//   node tests/compat/image-upstream.mjs <photos dir> <port>
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { gzipSync, brotliCompressSync } from "node:zlib";

export function startUpstream(photos, port, host = "0.0.0.0") {
  const hits = new Map();
  const file = (name) => readFileSync(join(photos, name));
  let version = 1;
  const server = createServer((req, res) => {
    const url = new URL(req.url, "http://upstream");
    const path = url.pathname;
    hits.set(path, (hits.get(path) ?? 0) + 1);
    const send = (status, body, headers = {}) => {
      res.writeHead(status, { "content-length": Buffer.byteLength(body), ...headers });
      res.end(body);
    };
    const image = (name, extra = {}) =>
      send(200, file(name), { "content-type": "image/jpeg", "cache-control": "public, max-age=60", ...extra });
    switch (path) {
      case "/img/photo.jpg":
        return image("photo.jpg", { etag: '"photo-v1"' });
      case "/img/alpha.png":
        return image("alpha.png", { "content-type": "image/png" });
      case "/img/no-cache-headers.jpg":
        return send(200, file("odd.jpg"));
      case "/img/s-maxage.jpg":
        return image("odd.jpg", { "cache-control": "public, max-age=5, s-maxage=30" });
      case "/img/versioned.jpg": {
        // The ETag changes when /control/bump is called: revalidation must
        // pick the new source up.
        const name = version % 2 ? "photo.jpg" : "odd.jpg";
        return image(name, { etag: `"v${version}"`, "cache-control": "public, max-age=1" });
      }
      case "/control/bump":
        version += 1;
        return send(200, String(version));
      case "/control/reset":
        version = 1;
        return send(200, "1");
      case "/img/redirect.jpg":
        return send(302, "", { location: "/img/photo.jpg" });
      case "/img/redirect-chain-3.jpg":
        return send(302, "", { location: "/img/redirect-chain-2.jpg" });
      case "/img/redirect-chain-2.jpg":
        return send(302, "", { location: "/img/redirect-chain-1.jpg" });
      case "/img/redirect-chain-1.jpg":
        return send(302, "", { location: "/img/redirect.jpg" });
      case "/img/redirect-loop.jpg":
        return send(307, "", { location: "/img/redirect-loop.jpg" });
      // To a loopback address NextRS's --net-allow-addr does not include
      // (next start, with dangerouslyAllowLocalIP, follows it).
      case "/img/redirect-other-addr.jpg":
        return send(302, "", { location: `http://127.0.0.2:${port}/forbidden/secret.jpg` });
      // To a host name the network policy does not allow.
      case "/img/redirect-other-host.jpg":
        return send(302, "", { location: `http://localhost:${port}/forbidden/secret.jpg` });
      case "/img/redirect-metadata.jpg":
        return send(302, "", { location: "http://169.254.169.254/latest/meta-data/" });
      case "/img/missing.jpg":
        return send(404, "not found", { "content-type": "text/plain" });
      case "/img/server-error.jpg":
        return send(503, "unavailable", { "content-type": "text/plain" });
      case "/img/text.jpg":
        return send(200, "this is not an image", { "content-type": "image/jpeg" });
      case "/img/html.jpg":
        return send(200, "<!doctype html><title>x</title>", { "content-type": "text/html" });
      case "/img/corrupt.jpg":
        return image("corrupt.jpg");
      case "/img/empty.jpg":
        return send(200, "", { "content-type": "image/jpeg" });
      case "/img/wrong-type.png":
        // A JPEG labelled as PNG: Next sniffs the bytes.
        return image("photo.jpg", { "content-type": "image/png" });
      case "/img/oversized.jpg": {
        // Above the fixture's maximumResponseBody (5,000,000), chunked.
        res.writeHead(200, { "content-type": "image/jpeg" });
        const chunk = Buffer.alloc(64 * 1024, 0xff);
        let sent = 0;
        const pump = () => {
          while (sent < 8_000_000) {
            sent += chunk.length;
            if (!res.write(chunk)) return res.once("drain", pump);
          }
          res.end();
        };
        return pump();
      }
      case "/img/gzip.jpg":
        return send(200, gzipSync(file("photo.jpg")), { "content-type": "image/jpeg", "content-encoding": "gzip" });
      case "/img/brotli.jpg":
        return send(200, brotliCompressSync(file("photo.jpg")), { "content-type": "image/jpeg", "content-encoding": "br" });
      case "/img/slow-head.jpg":
        // Headers after 9 s: past Next's 7 s upstream timeout.
        return setTimeout(() => image("photo.jpg"), 9000).unref();
      case "/img/slow-body.jpg": {
        const body = file("photo.jpg");
        res.writeHead(200, { "content-type": "image/jpeg", "content-length": body.length });
        res.write(body.subarray(0, 1000));
        return setTimeout(() => res.end(body.subarray(1000)), 9000).unref();
      }
      case "/img/reset.jpg":
        return req.socket.destroy();
      case "/img/animated.gif":
        return image("animated.gif", { "content-type": "image/gif" });
      case "/img/icon.svg":
        return image("icon.svg", { "content-type": "image/svg+xml" });
      default:
        if (path.startsWith("/other/") || path.startsWith("/forbidden/")) return image("photo.jpg");
        return send(404, "not found");
    }
  });
  return new Promise((resolve) => {
    server.listen(port, host, () =>
      resolve({
        server,
        hits,
        count: (path) => hits.get(path) ?? 0,
        reset: () => hits.clear(),
        close: () => new Promise((r) => { server.closeAllConnections?.(); server.close(r); }),
      }),
    );
  });
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [photos, port] = process.argv.slice(2);
  await startUpstream(photos, Number(port));
  console.log(`image upstream on ${port}`);
}
