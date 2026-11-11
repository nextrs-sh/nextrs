import Link from "next/link";
import { useRouter } from "next/router";

// What Next gave the page (props from the data function) next to what the
// client router reports after hydration, so a suite can check both.
export type LocaleProps = { locale: string | null; locales: readonly string[] | null; defaultLocale: string | null };

export function fromContext(context: { locale?: string; locales?: readonly string[]; defaultLocale?: string }): LocaleProps {
  return { locale: context.locale ?? null, locales: context.locales ?? null, defaultLocale: context.defaultLocale ?? null } as LocaleProps;
}

export function LocaleInfo({ props }: { props: LocaleProps }) {
  const router = useRouter();
  return (
    <>
      <p id="locale">{props.locale ?? "-"}</p>
      <p id="locales">{(props.locales ?? []).join(",")}</p>
      <p id="default-locale">{props.defaultLocale ?? "-"}</p>
      <p id="router-locale">{router.locale ?? "-"}</p>
      <nav>
        <Link id="to-about" href="/about">about</Link>
        <Link id="to-ssr" href="/ssr">ssr</Link>
        <Link id="to-post-fr" href="/posts/hello" locale="fr">post fr</Link>
        <Link id="to-ja" href={router.asPath} locale="ja">ja</Link>
        <Link id="to-en" href={router.asPath} locale="en">en</Link>
      </nav>
    </>
  );
}
