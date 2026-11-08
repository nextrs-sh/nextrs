import type { NextConfig } from "next";

// A Pages Router application (no app/ directory). One fixture, several
// builds: NEXTRS_VARIANT=base-path adds basePath "/base", trailing-slash
// sets trailingSlash. `next start` must run with the same NEXTRS_VARIANT
// as the build (it reads distDir from this file).
const variant = process.env.NEXTRS_VARIANT ?? "default";
const withBase = variant === "base-path";
const withSlash = variant === "trailing-slash";

const nextConfig: NextConfig = {
  distDir: variant === "default" ? ".next" : `.next-${variant}`,
  basePath: withBase ? "/base" : undefined,
  trailingSlash: withSlash,
  async headers() {
    return [
      { source: "/about", headers: [{ key: "x-config-about", value: "1" }] },
      { source: "/books/:id", headers: [{ key: "x-config-book", value: ":id" }] },
      { source: "/api/:path*", headers: [{ key: "x-config-api", value: "1" }] },
    ];
  },
  async redirects() {
    return [
      { source: "/old-about", destination: "/about", permanent: true },
      { source: "/old-book/:id", destination: "/books/:id", permanent: false },
    ];
  },
  async rewrites() {
    return {
      beforeFiles: [{ source: "/bf-about", destination: "/about" }],
      afterFiles: [
        { source: "/rw-book/:id", destination: "/books/:id" },
        { source: "/rw-post/:slug", destination: "/posts/:slug" },
        { source: "/rw-api/:path*", destination: "/api/echo?alias=:path*" },
        { source: "/rw-secret", destination: "/protected/secret" },
      ],
      fallback: [{ source: "/fb/:path*", destination: "/books/:path*" }],
    };
  },
};

export default nextConfig;
