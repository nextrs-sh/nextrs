import type { GetStaticProps } from "next";

const TEXT: Record<string, string> = { en: "Not found", fr: "Introuvable", ja: "見つかりません" };

// Localized 404: Next writes one per locale.
export const getStaticProps: GetStaticProps<{ text: string; locale: string }> = async (context) => ({
  props: { text: TEXT[context.locale ?? "en"] ?? "?", locale: context.locale ?? "-" },
});

export default function NotFound({ text, locale }: { text: string; locale: string }) {
  return (
    <>
      <h1 id="title">{text}</h1>
      <p id="kind">404</p>
      <p id="locale">{locale}</p>
    </>
  );
}
