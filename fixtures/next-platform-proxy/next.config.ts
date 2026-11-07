import type { NextConfig } from "next";

// External rewrites (to the test upstream, a closed port, an origin the
// network policy does not list, the metadata address, and a destination
// host taken from the path), plus a header rule that next start does not
// apply to config external rewrites. The destinations are fixed at build
// time, so the build and both servers see the same environment.
const upstream = process.env.NEXTRS_UPSTREAM ?? "http://127.0.0.1:4999";
const closed = process.env.NEXTRS_CLOSED_PORT ?? "9";
const unlisted = process.env.NEXTRS_UNLISTED_PORT ?? "10";

const nextConfig: NextConfig = {
  // How long next start's proxy waits for an external rewrite's upstream
  // (and between parts of its body); NextRS reads the same setting.
  experimental: { proxyTimeout: 5000 },
  async headers() {
    return [
      { source: "/ext/:path*", headers: [{ key: "x-config-ext", value: "1" }] },
      { source: "/gate/:path*", headers: [{ key: "x-config-gate", value: "1" }] },
    ];
  },
  async rewrites() {
    return {
      beforeFiles: [
        { source: "/ext/:path*", destination: `${upstream}/echo/:path*` },
        { source: "/ext-query", destination: `${upstream}/echo/q?fixed=a b&x=1` },
        { source: "/ext-redirect", destination: `${upstream}/redirect?to=/echo/after-redirect` },
        { source: "/ext-large/:bytes", destination: `${upstream}/large?bytes=:bytes` },
        { source: "/ext-stream", destination: `${upstream}/stream?chunks=5&gap=100` },
        { source: "/ext-slow", destination: `${upstream}/data?key=ext-slow&delay=3000` },
        // Past experimental.proxyTimeout (5 s): no answer for 7 s, and a
        // stream that goes quiet for 6.5 s after its first line.
        { source: "/ext-too-slow", destination: `${upstream}/data?key=ext-too-slow&delay=7000` },
        { source: "/ext-stall", destination: `${upstream}/stream?chunks=3&gap=6500` },
        { source: "/ext-status/:code", destination: `${upstream}/echo/status?status=:code` },
        { source: "/ext-closed/:path*", destination: `http://127.0.0.1:${closed}/:path*` },
        { source: "/ext-unlisted/:path*", destination: `http://127.0.0.1:${unlisted}/echo/:path*` },
        { source: "/ext-metadata", destination: "http://169.254.169.254/latest/meta-data/" },
        { source: "/ext-host/:host", destination: "http://:host/echo/host" },
      ],
    };
  },
};

export default nextConfig;
