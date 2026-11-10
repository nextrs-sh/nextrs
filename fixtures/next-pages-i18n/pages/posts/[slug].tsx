import type { GetStaticPaths, GetStaticProps } from "next";
import { useRouter } from "next/router";
import { fromContext, LocaleInfo, type LocaleProps } from "../../lib/locale";

type Props = LocaleProps & { slug: string; built: number };

// Localized paths: "hello" in en and fr, "bonjour" with no locale (Next
// assigns the default), nothing for ja. Others render on demand.
export const getStaticPaths: GetStaticPaths = async () => ({
  paths: [
    { params: { slug: "hello" }, locale: "en" },
    { params: { slug: "hello" }, locale: "fr" },
    { params: { slug: "bonjour" } },
  ],
  fallback: "blocking",
});

export const getStaticProps: GetStaticProps<Props> = async (context) => {
  const slug = String(context.params?.slug);
  if (slug === "missing") return { notFound: true };
  if (slug === "gone") return { redirect: { destination: "/about", permanent: false } };
  return { props: { ...fromContext(context), slug, built: Date.now() } };
};

export default function Post(props: Props) {
  const router = useRouter();
  return (
    <>
      <h1 id="title">Post {props.slug}</h1>
      <p id="kind">ssg-dynamic</p>
      <p id="slug">{props.slug}</p>
      <p id="fallback">{String(router.isFallback)}</p>
      <LocaleInfo props={props} />
    </>
  );
}
