import { useTranslations } from "next-intl";
import { setRequestLocale } from "next-intl/server";
import { use } from "react";
import { Link } from "../../i18n/navigation";

// Locale-aware static route (prerendered per locale).
export default function Home({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = use(params);
  setRequestLocale(locale);
  const t = useTranslations("Home");
  return (
    <main>
      <h1 id="title">{t("title")}</h1>
      <p id="kind">{locale}</p>
      <Link id="to-about" href="/about">{t("about")}</Link> <Link id="to-product" href="/products/42">42</Link>{" "}
      <Link id="to-de" href="/" locale="de">de</Link>
    </main>
  );
}
