import type { NextConfig } from "next";

// One fixture, two builds. NEXTRS_VARIANT=alt adds basePath "/base",
// allows SVG, answers inline and drops localPatterns (every local path).
// `next start` must run with the build's NEXTRS_VARIANT (distDir).
const variant = process.env.NEXTRS_VARIANT ?? "default";
const alt = variant === "alt";
// The deterministic local image server the suites start (never a public
// host). The port is fixed at build time: remotePatterns are compiled in.
const upstreamPort = process.env.NEXTRS_IMAGE_UPSTREAM_PORT ?? "4317";
// HTTPS upstreams (tests/compat/image-https.mjs): a certificate for
// "localhost" from the suite's test CA on this port, one from an untrusted
// CA on the next.
const httpsPort = process.env.NEXTRS_IMAGE_HTTPS_PORT ?? String(Number(upstreamPort) + 1);
const untrustedPort = String(Number(httpsPort) + 1);

// assetPrefix builds (tests/compat/image-asset-prefix.mjs): a path, and an
// absolute origin (a local stand-in for a CDN that serves .next/static).
const cdnPort = process.env.NEXTRS_IMAGE_CDN_PORT ?? String(Number(upstreamPort) + 3);
const assetPrefix =
  variant === "asset-prefix" ? "/cdn" : variant === "asset-prefix-url" ? `http://127.0.0.1:${cdnPort}` : undefined;

const nextConfig: NextConfig = {
  distDir: variant === "default" ? ".next" : `.next-${variant}`,
  basePath: alt ? "/base" : undefined,
  assetPrefix,
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
      { protocol: "https", hostname: "localhost", port: httpsPort, pathname: "/img/**" },
      // The certificate names localhost only: a name mismatch.
      { protocol: "https", hostname: "127.0.0.1", port: httpsPort, pathname: "/img/**" },
      { protocol: "https", hostname: "localhost", port: untrustedPort, pathname: "/img/**" },
    ],
    localPatterns: alt
      ? undefined
      : [
          { pathname: "/photos/**", search: "" },
          { pathname: "/_next/static/media/**", search: "" },
          { pathname: "/api/image/**" },
          { pathname: "/rewritten/**", search: "" },
          // Static imports carry a path assetPrefix; localPatterns apply to
          // the prefixed path.
          ...(variant === "asset-prefix" ? [{ pathname: "/cdn/_next/static/media/**", search: "" }] : []),
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
