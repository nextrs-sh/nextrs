import type { NextConfig } from "next";

// One fixture, two builds. NEXTRS_VARIANT=alt adds basePath "/base",
// allows SVG, answers inline and drops localPatterns (every local path).
// `next start` must run with the build's NEXTRS_VARIANT (distDir).
const variant = process.env.NEXTRS_VARIANT ?? "default";
const alt = variant === "alt";
// The deterministic local image server the suites start (never a public
// host). The port is fixed at build time: remotePatterns are compiled in.
const upstreamPort = process.env.NEXTRS_IMAGE_UPSTREAM_PORT ?? "4317";

const nextConfig: NextConfig = {
  distDir: variant === "default" ? ".next" : `.next-${variant}`,
  basePath: alt ? "/base" : undefined,
  images: {
    formats: ["image/avif", "image/webp"],
    qualities: [50, 75, 90],
    // Short, so the suites can observe stale entries being revalidated.
    minimumCacheTTL: 2,
    maximumResponseBody: 5_000_000,
    // The upstream is on loopback; NextRS also requires --net-allow-addr.
    dangerouslyAllowLocalIP: true,
    dangerouslyAllowSVG: alt,
    contentDispositionType: alt ? "inline" : "attachment",
    remotePatterns: [
      { protocol: "http", hostname: "127.0.0.1", port: upstreamPort, pathname: "/img/**" },
    ],
    localPatterns: alt
      ? undefined
      : [
          { pathname: "/photos/**", search: "" },
          { pathname: "/_next/static/media/**", search: "" },
          { pathname: "/api/image/**" },
          { pathname: "/rewritten/**", search: "" },
        ],
  },
  async rewrites() {
    return [
      // A local image source behind a rewrite (routing applies to the
      // optimizer's own request), and one that points back at the endpoint.
      { source: "/rewritten/:file", destination: "/photos/:file" },
      { source: "/rewritten-loop/:file", destination: "/_next/image?url=/photos/:file&w=64&q=75" },
    ];
  },
};

export default nextConfig;
