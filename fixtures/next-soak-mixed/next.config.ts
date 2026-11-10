import type { NextConfig } from "next";

// The mixed soak's application (benchmarks/phase2g_soak.mjs): the App
// Router soak routes and next/image sources of fixtures/next-image beside a
// Pages Router (SSR, SSG, ISR, data routes, Preview Mode, API routes), one
// proxy in front of both. Images come from scripts/make-images.mjs.
const upstreamPort = process.env.NEXTRS_IMAGE_UPSTREAM_PORT ?? "4317";

const nextConfig: NextConfig = {
  images: {
    formats: ["image/avif", "image/webp"],
    qualities: [50, 75, 90],
    minimumCacheTTL: 2,
    maximumResponseBody: 5_000_000,
    dangerouslyAllowLocalIP: true,
    remotePatterns: [{ protocol: "http", hostname: "127.0.0.1", port: upstreamPort, pathname: "/img/**" }],
    localPatterns: [{ pathname: "/photos/**", search: "" }],
  },
};

export default nextConfig;
