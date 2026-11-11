import type { GetStaticProps } from "next";
import { fromContext, LocaleInfo, type LocaleProps } from "../lib/locale";

type Props = LocaleProps & { preview: boolean; previewData: string | null; built: number };

export const getStaticProps: GetStaticProps<Props> = async (context) => ({
  props: {
    ...fromContext(context),
    preview: Boolean(context.preview),
    previewData: context.previewData ? JSON.stringify(context.previewData) : null,
    built: Date.now(),
  },
});

export default function Preview(props: Props) {
  return (
    <>
      <h1 id="title">Preview {props.locale}</h1>
      <p id="kind">ssg-preview</p>
      <p id="info">{`preview=${props.preview} data=${props.previewData ?? "-"}`}</p>
      <LocaleInfo props={props} />
    </>
  );
}
