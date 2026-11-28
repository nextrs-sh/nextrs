import type { NextConfig } from "next";

// The resource harness's fault app: one standalone build, served by the
// Node.js image and by every NextRS image (benchmarks/resource/docker).
const nextConfig: NextConfig = {
  output: "standalone",
};

export default nextConfig;
