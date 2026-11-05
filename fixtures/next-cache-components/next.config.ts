import type { NextConfig } from "next";

// Cache Components: "use cache", cacheLife and cacheTag. A separate fixture,
// because the flag changes how every route of an application renders.
const nextConfig: NextConfig = {
  cacheComponents: true,
  cacheLife: {
    // Short enough for the tests to watch a cached value go stale and expire.
    short: { stale: 1, revalidate: 2, expire: 30 },
  },
};

export default nextConfig;
