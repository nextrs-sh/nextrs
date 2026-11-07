import type { NextConfig } from "next";

// An API and web application whose users come from an OpenID provider:
// bearer access tokens are verified against the provider's JSON Web Key
// Set with jose's createRemoteJWKSet (in proxy.ts and again in route
// handlers), and the application's own session cookie is an ES256 JWT it
// signs with its private key. Asymmetric keys only.
//
// For the soak's mixed traffic it also forwards /ext/* to the test upstream
// (an external rewrite behind the proxy; the destination is fixed at build
// time), streams a route handler's body and does work after the response.
const upstream = process.env.NEXTRS_UPSTREAM ?? "http://127.0.0.1:4999";

const nextConfig: NextConfig = {
  async rewrites() {
    return { beforeFiles: [{ source: "/ext/:path*", destination: `${upstream}/echo/:path*` }] };
  },
};

export default nextConfig;
