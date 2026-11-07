import { getTranslations } from "next-intl/server";

export default async function Search({ params, searchParams }: { params: Promise<{ locale: string }>; searchParams: Promise<Record<string, string | string[]>> }) {
  const { locale } = await params;
  const query = await searchParams;
  const t = await getTranslations({ locale, namespace: "Search" });
  return (
    <main>
      <h1 id="title">{t("title", { q: String(query.q ?? "") })}</h1>
      <p id="query">{JSON.stringify(query)}</p>
    </main>
  );
}
