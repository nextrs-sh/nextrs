import type { GetStaticProps } from "next";

export const getStaticProps: GetStaticProps<{ preview: boolean; who: string }> = async (context) => ({
  props: { preview: Boolean(context.preview), who: (context.previewData as { who?: string } | undefined)?.who ?? "-" },
});

export default function PagesPreview({ preview, who }: { preview: boolean; who: string }) {
  return <p id="preview">{`${preview}:${who}`}</p>;
}
