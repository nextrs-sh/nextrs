import type { NextConfig } from "next";

// An application that authenticates in proxy.ts the way the Next.js
// authentication guide does (jose, an HS256 session cookie, an optimistic
// check in the proxy and a data access layer that checks again).
const nextConfig: NextConfig = {
  async rewrites() {
    // An alias the proxy's path list does not name: the proxy runs on
    // `/me`, before this rewrite, so only the data access layer protects it.
    return [{ source: "/me", destination: "/dashboard" }];
  },
};

export default nextConfig;
