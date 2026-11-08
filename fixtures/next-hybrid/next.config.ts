import type { NextConfig } from "next";

// App Router and Pages Router in one application (app/ and pages/).
const nextConfig: NextConfig = {
  async rewrites() {
    return [{ source: "/to-pages/:id", destination: "/pages-items/:id" }];
  },
};

export default nextConfig;
