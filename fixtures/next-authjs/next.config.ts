import type { NextConfig } from "next";

// Auth.js (next-auth v5) as its documentation sets it up for the App
// Router: auth.ts, the route handler at /api/auth/[...nextauth], `auth` as
// the proxy with an `authorized` callback, and `auth()` in pages, route
// handlers and Server Actions. JWT sessions (Auth.js's default without a
// database adapter): an encrypted JWT (JWE, dir + A256CBC-HS512) in a
// cookie.
const nextConfig: NextConfig = {};

export default nextConfig;
