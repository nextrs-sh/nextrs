import { useRouter } from "next/router";
import { LocaleInfo } from "../lib/locale";

// No data function: Next writes one HTML file per locale.
export default function About() {
  const { locale, locales, defaultLocale } = useRouter();
  return (
    <>
      <h1 id="title">About</h1>
      <p id="kind">static</p>
      <LocaleInfo props={{ locale: locale ?? null, locales: locales ?? null, defaultLocale: defaultLocale ?? null }} />
    </>
  );
}
