import type { GetStaticPaths, GetStaticProps } from "next";

type Props = { slug: string; built: number };

// fallback: "blocking": an unknown path is rendered on the first request
// (no shell), then stored and revalidated.
export const getStaticPaths: GetStaticPaths = async () => ({
  paths: [{ params: { slug: "known" } }],
  fallback: "blocking",
});

export const getStaticProps: GetStaticProps<Props> = async ({ params }) => {
  const slug = String(params?.slug);
  if (slug === "missing") return { notFound: true };
  if (slug === "moved") return { redirect: { destination: "/posts/alpha", permanent: false } };
  return { props: { slug, built: Date.now() }, revalidate: 60 };
};

export default function FallbackBlocking({ slug, built }: Props) {
  return (
    <>
      <h1 id="title">Fallback blocking {slug}</h1>
      <p id="kind">fallback-blocking</p>
      <p id="slug">{slug}</p>
      <p id="info">built-{built}</p>
    </>
  );
}
