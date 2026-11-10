import type { NextConfig } from "next";

// App Router and Pages Router in one application (app/ and pages/).
// NEXTRS_VARIANT names another build directory (.next-<variant>), for the
// same application built with a deployment adapter; `next start` must run
// with the same NEXTRS_VARIANT.
const variant = process.env.NEXTRS_VARIANT;
const nextConfig: NextConfig = {
  distDir: variant ? `.next-${variant}` : ".next",
  async rewrites() {
    return [{ source: "/to-pages/:id", destination: "/pages-items/:id" }];
  },
};

export default nextConfig;
