import type { NextConfig } from "next";

// NEXT_OUTPUT=standalone produces .next/standalone for artifact inspection.
// The default build is what `next start` and `nextrs start` consume.
const nextConfig: NextConfig = {
  output: process.env.NEXT_OUTPUT === "standalone" ? "standalone" : undefined,
};

export default nextConfig;
