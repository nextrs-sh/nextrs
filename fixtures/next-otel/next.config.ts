import type { NextConfig } from "next";

// OpenTelemetry as the Next.js instrumentation guide sets it up:
// @vercel/otel's registerOTel in instrumentation.ts, exporting OTLP (JSON
// over HTTP) to a collector; Next's own spans plus the application's.
const nextConfig: NextConfig = {};

export default nextConfig;
