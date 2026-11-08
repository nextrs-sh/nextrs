import type { GetStaticPaths, GetStaticProps } from "next";
import { useRouter } from "next/router";

type Props = { slug: string; built: number };

// fallback: true: an unknown path first gets the fallback shell, then its
// data is generated and stored.
export const getStaticPaths: GetStaticPaths = async () => ({
  paths: [{ params: { slug: "known" } }],
  fallback: true,
});

export const getStaticProps: GetStaticProps<Props> = async ({ params }) => {
  const slug = String(params?.slug);
  if (slug === "missing") return { notFound: true };
  return { props: { slug, built: Date.now() } };
};

export default function FallbackTrue({ slug, built }: Props) {
  const router = useRouter();
  if (router.isFallback) return <p id="kind">fallback-loading</p>;
  return (
    <>
      <h1 id="title">Fallback true {slug}</h1>
      <p id="kind">fallback-true</p>
      <p id="slug">{slug}</p>
      <p id="info">built-{built}</p>
    </>
  );
}
