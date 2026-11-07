import type { NextConfig } from "next";

// One fixture, three builds: NEXTRS_VARIANT=base-path adds basePath: "/base",
// NEXTRS_VARIANT=trailing-slash sets trailingSlash: true. The routing rules
// are the same in every build. `next start` must run with the same
// NEXTRS_VARIANT as the build (it reads distDir from this file).
const variant = process.env.NEXTRS_VARIANT ?? "default";
// A port nothing listens on: the external rewrite and redirect targets.
const closed = process.env.NEXTRS_CLOSED_PORT ?? "9";

const nextConfig: NextConfig = {
  // Each variant builds into its own directory, so all three can be served
  // side by side from one checkout.
  distDir: variant === "default" ? ".next" : `.next-${variant}`,
  basePath: variant === "base-path" ? "/base" : undefined,
  trailingSlash: variant === "trailing-slash",
  async headers() {
    return [
      { source: "/:path*", headers: [{ key: "x-config-all", value: "1" }] },
      { source: "/blog/:slug", headers: [{ key: "x-blog-slug", value: ":slug" }] },
      { source: "/static", has: [{ type: "query", key: "h" }], headers: [{ key: "x-has-query", value: "yes" }] },
      { source: "/target", missing: [{ type: "header", key: "x-no-header" }], headers: [{ key: "x-missing-header", value: "absent" }] },
      { source: "/target", headers: [{ key: "set-cookie", value: "config-cookie=1; Path=/" }] },
      { source: "/proxy/:path*", headers: [{ key: "x-conflict", value: "config" }, { key: "x-config-proxy", value: "from-config" }] },
      { source: "/cookie-header", has: [{ type: "cookie", key: "theme", value: "(?<theme>dark|light)" }], headers: [{ key: "x-theme", value: ":theme" }] },
    ];
  },
  async redirects() {
    return [
      { source: "/old-blog/:slug", destination: "/blog/:slug", permanent: true },
      { source: "/temp-redirect", destination: "/target", permanent: false },
      { source: "/status-redirect", destination: "/target", statusCode: 301 },
      { source: "/query-redirect", has: [{ type: "query", key: "to", value: "(?<to>[a-z]+)" }], destination: "/blog/:to", permanent: false },
      { source: "/keep-query", destination: "/target?fixed=1", permanent: false },
      { source: "/external-redirect/:path*", destination: "https://example.com/:path*", permanent: false },
      { source: "/proxy/config-wins", destination: "/target", permanent: false },
      { source: "/cookie-redirect", has: [{ type: "cookie", key: "redir" }], destination: "/target", permanent: false },
      { source: "/host-redirect", has: [{ type: "host", value: "127.0.0.1" }], destination: "/target", permanent: false },
      { source: "/wildcard/:path*", destination: "/blog/:path*", permanent: false },
      { source: "/regex/:id(\\d{1,})", destination: "/blog/num-:id", permanent: false },
      { source: "/missing-redirect", missing: [{ type: "header", key: "x-stay" }], destination: "/target", permanent: false },
    ];
  },
  async rewrites() {
    return {
      beforeFiles: [
        { source: "/bf-static", destination: "/target" },
        { source: "/static", has: [{ type: "query", key: "bf" }], destination: "/target" },
      ],
      afterFiles: [
        { source: "/af-chain", destination: "/af/chained" },
        { source: "/af/:slug", destination: "/blog/:slug" },
        { source: "/static", destination: "/target" },
        { source: "/af-isr", destination: "/isr" },
        { source: "/af-api/:path*", destination: "/api/echo?alias=:path*" },
        { source: "/af-query", destination: "/rewritten/q?fixed=1" },
        { source: "/af-protected", destination: "/protected" },
        { source: "/external-rewrite/:path*", destination: `http://127.0.0.1:${closed}/:path*` },
      ],
      fallback: [
        { source: "/fb/:path*", destination: "/blog/:path*" },
        { source: "/blog/:slug", destination: "/target" },
      ],
    };
  },
};

export default nextConfig;
