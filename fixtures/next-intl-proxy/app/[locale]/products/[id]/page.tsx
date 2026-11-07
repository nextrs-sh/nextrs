import { getTranslations } from "next-intl/server";
import { headers } from "next/headers";

// Locale-aware dynamic route: rendered per request, params from the path.
export default async function Product({ params }: { params: Promise<{ locale: string; id: string }> }) {
  const { locale, id } = await params;
  const t = await getTranslations({ locale, namespace: "Product" });
  const h = await headers();
  return (
    <main>
      <h1 id="title">{t("title", { id })}</h1>
      <p id="slug">{id}</p>
      <p id="kind">{locale}</p>
      <p id="info">{JSON.stringify({ acceptLanguage: h.get("accept-language"), nextUrl: h.get("next-url") })}</p>
    </main>
  );
}
