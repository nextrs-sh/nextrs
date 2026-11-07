import { getTranslations } from "next-intl/server";
import { GreetForm } from "./form";

export default async function Actions({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  const t = await getTranslations({ locale, namespace: "Action" });
  return (
    <main>
      <h1 id="title">{t("title")}</h1>
      <GreetForm />
    </main>
  );
}
