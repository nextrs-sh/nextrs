import type { GetStaticProps } from "next";

type Props = { preview: boolean; data: string; built: number };

// An SSG page that renders differently in Preview Mode.
export const getStaticProps: GetStaticProps<Props> = async (context) => ({
  props: {
    preview: Boolean(context.preview),
    data: context.previewData ? JSON.stringify(context.previewData) : "none",
    built: Date.now(),
  },
});

export default function Preview({ preview, data, built }: Props) {
  return (
    <>
      <h1 id="title">Preview page</h1>
      <p id="kind">{preview ? "preview" : "published"}</p>
      <p id="secret">{data}</p>
      <p id="info">built-{built}</p>
    </>
  );
}
