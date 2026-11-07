import type { NextConfig } from "next";
import createNextIntlPlugin from "next-intl/plugin";

// next-intl's App Router setup without Next's own i18n config (which NextRS
// refuses): locale routing is entirely the library's middleware (proxy.ts)
// and the [locale] segment.
//
// NEXTRS_VARIANT (default, as-needed, pathnames, domains) picks next-intl's
// routing mode (i18n/routing.ts) and the build directory; `next start`
// must run with the same NEXTRS_VARIANT as the build.
const variant = process.env.NEXTRS_VARIANT ?? "default";
const withNextIntl = createNextIntlPlugin();

const nextConfig: NextConfig = {
  distDir: variant === "default" ? ".next" : `.next-${variant}`,
  env: { NEXTRS_INTL_VARIANT: variant },
};

export default withNextIntl(nextConfig);
