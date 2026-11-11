import type { GetStaticProps } from "next";
import { fromContext, LocaleInfo, type LocaleProps } from "../lib/locale";

const GREETING: Record<string, string> = { en: "Hello", fr: "Bonjour", ja: "こんにちは" };

export const getStaticProps: GetStaticProps<LocaleProps & { greeting: string }> = async (context) => ({
  props: { ...fromContext(context), greeting: GREETING[context.locale ?? "en"] ?? "?" },
});

export default function Home(props: LocaleProps & { greeting: string }) {
  return (
    <>
      <h1 id="title">{props.greeting}</h1>
      <p id="kind">ssg-index</p>
      <LocaleInfo props={props} />
    </>
  );
}
