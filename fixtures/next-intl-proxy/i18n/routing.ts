import { defineRouting } from "next-intl/routing";

// One fixture, several of next-intl's routing modes (next.config.ts inlines
// the build's NEXTRS_VARIANT as NEXTRS_INTL_VARIANT):
//   default    localePrefix "always" (Phase 2B)
//   as-needed  the default locale has no prefix
//   pathnames  localized pathnames (prefix always)
//   domains    a default locale per domain (prefix as-needed)
// Each mode is next-intl's own defineRouting call; the pages are typed
// against the default mode's (string) hrefs, which every mode accepts.
const variant = process.env.NEXTRS_INTL_VARIANT ?? "default";
const locales = ["en", "de", "fr"] as const;

const defaults = defineRouting({ locales, defaultLocale: "en" });

const modes = {
  "as-needed": () => defineRouting({ locales, defaultLocale: "en", localePrefix: "as-needed" }),
  pathnames: () =>
    defineRouting({
      locales,
      defaultLocale: "en",
      pathnames: {
        "/": "/",
        "/about": { en: "/about", de: "/ueber-uns", fr: "/a-propos" },
        "/products/[id]": { en: "/products/[id]", de: "/produkte/[id]", fr: "/produits/[id]" },
        "/search": { en: "/search", de: "/suche", fr: "/recherche" },
        "/actions": "/actions",
      },
    }),
  domains: () =>
    defineRouting({
      locales,
      defaultLocale: "en",
      localePrefix: "as-needed",
      domains: [
        { domain: "shop.example.de", defaultLocale: "de", locales: ["de", "en"] },
        { domain: "shop.example.com", defaultLocale: "en", locales: ["en", "fr"] },
      ],
    }),
};

export const routing = (variant in modes ? modes[variant as keyof typeof modes]() : defaults) as unknown as typeof defaults;
