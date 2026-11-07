import type { NextConfig } from "next";
import createNextIntlPlugin from "next-intl/plugin";

// next-intl's App Router setup without Next's own i18n config (which NextRS
// refuses): locale routing is entirely the library's middleware (proxy.ts)
// and the [locale] segment.
const withNextIntl = createNextIntlPlugin();

const nextConfig: NextConfig = {};

export default withNextIntl(nextConfig);
