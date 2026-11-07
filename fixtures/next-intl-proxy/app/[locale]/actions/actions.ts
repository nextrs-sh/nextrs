"use server";

import { getLocale, getTranslations } from "next-intl/server";

// A locale-aware Server Action: the locale comes from the request the
// middleware routed.
export async function greet(formData: FormData) {
  const locale = await getLocale();
  const t = await getTranslations("Action");
  return { recorded: t("done", { name: String(formData.get("name") ?? "") }), locale };
}
