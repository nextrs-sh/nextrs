import type { GetStaticPaths, GetStaticProps } from "next";

type Props = { slug: string; built: number };

// fallback: false: only the listed paths exist.
export const getStaticPaths: GetStaticPaths = async () => ({
  paths: [{ params: { slug: "alpha" } }, { params: { slug: "beta" } }],
  fallback: false,
});

export const getStaticProps: GetStaticProps<Props> = async ({ params }) => ({
  props: { slug: String(params?.slug), built: Date.now() },
});

export default function Post({ slug, built }: Props) {
  return (
    <>
      <h1 id="title">Post {slug}</h1>
      <p id="kind">ssg-paths</p>
      <p id="slug">{slug}</p>
      <p id="info">built-{built}</p>
    </>
  );
}
