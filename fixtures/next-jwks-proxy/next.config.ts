import type { NextConfig } from "next";

// An API and web application whose users come from an OpenID provider:
// bearer access tokens are verified against the provider's JSON Web Key
// Set with jose's createRemoteJWKSet (in proxy.ts and again in route
// handlers), and the application's own session cookie is an ES256 JWT it
// signs with its private key. Asymmetric keys only.
const nextConfig: NextConfig = {};

export default nextConfig;
