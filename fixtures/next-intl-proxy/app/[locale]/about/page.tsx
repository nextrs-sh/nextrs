import { useTranslations } from "next-intl";
import { setRequestLocale } from "next-intl/server";
import { use } from "react";
import { Link } from "../../../i18n/navigation";

export default function About({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = use(params);
  setRequestLocale(locale);
  const t = useTranslations("About");
  return (
    <main>
      <h1 id="title">{t("title")}</h1>
      <p id="kind">{locale}</p>
      <Link id="to-home" href="/">home</Link>
    </main>
  );
}
