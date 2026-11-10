import type { NextConfig } from "next";

// A Pages Router application with Next's native i18n routing. One fixture,
// several builds (NEXTRS_VARIANT, also given to `next start`, which reads
// distDir from this file):
//   default     sub-path locales en (default), fr, ja; detection on
//   no-detect   the same with localeDetection: false, and no proxy
//               (proxy.mw.ts is only a page extension in the others)
//   base-path   basePath "/base" with trailingSlash
//   domains     domain locales: example.com (en), example.fr (fr),
//               example.jp (ja, served over http)
const variant = process.env.NEXTRS_VARIANT ?? "default";

const i18n: NextConfig["i18n"] = {
  locales: ["en", "fr", "ja"],
  defaultLocale: "en",
  localeDetection: variant === "no-detect" ? false : undefined,
  domains:
    variant === "domains"
      ? [
          { domain: "example.com", defaultLocale: "en" },
          { domain: "example.fr", defaultLocale: "fr" },
          { domain: "example.jp", defaultLocale: "ja", http: true },
        ]
      : undefined,
};

const nextConfig: NextConfig = {
  distDir: variant === "default" ? ".next" : `.next-${variant}`,
  pageExtensions: variant === "no-detect" ? ["tsx", "ts"] : ["tsx", "ts", "mw.ts"],
  basePath: variant === "base-path" ? "/base" : undefined,
  trailingSlash: variant === "base-path",
  i18n,
  async headers() {
    return [
      { source: "/about", headers: [{ key: "x-config-about", value: "1" }] },
      // locale: false matches the path exactly as written (with its locale).
      { source: "/fr/about", locale: false, headers: [{ key: "x-config-fr-about", value: "1" }] },
    ];
  },
  async redirects() {
    return [
      { source: "/old-about", destination: "/about", permanent: false },
      { source: "/old-post/:slug", destination: "/posts/:slug", permanent: true },
      { source: "/fr/ancien", destination: "/fr/about", locale: false, permanent: false },
    ];
  },
  async rewrites() {
    return {
      beforeFiles: [{ source: "/bf-about", destination: "/about" }],
      afterFiles: [
        { source: "/rw-ssr", destination: "/ssr?from=rewrite" },
        { source: "/rw-post/:slug", destination: "/posts/:slug" },
        // A config rewrite into a protected path: the proxy does not run
        // for the destination (next start's documented behaviour), so the
        // page itself must check the session.
        { source: "/rw-secret", destination: "/protected/secret" },
        // A rewrite that changes the locale (locale: false: written out).
        { source: "/en/to-fr", destination: "/fr/ssr", locale: false },
      ],
      fallback: [{ source: "/fb/:path*", destination: "/posts/:path*" }],
    };
  },
};

export default nextConfig;
