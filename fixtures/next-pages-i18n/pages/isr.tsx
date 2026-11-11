import type { GetStaticProps } from "next";
import { upstream } from "../lib/upstream";
import { fromContext, LocaleInfo, type LocaleProps } from "../lib/locale";

type Props = LocaleProps & { generation: number; built: number };

// ISR per locale: each locale's page is its own cache entry, and its
// generation counts the upstream hits for that locale's key.
export const getStaticProps: GetStaticProps<Props> = async (context) => {
  const locale = context.locale ?? "-";
  const flag = await fetch(`${upstream()}/flag?name=i18n-isr-fail`, { cache: "no-store" }).then((r) => r.json());
  if (flag.on) throw new Error("ISR regeneration failure (synthetic)");
  const data = await fetch(`${upstream()}/data?key=i18n-isr-${process.env.NEXTRS_SIDE ?? "x"}-${locale}`, { cache: "no-store" }).then((r) => r.json());
  return { props: { ...fromContext(context), generation: data.hit, built: Date.now() }, revalidate: 2 };
};

export default function Isr(props: Props) {
  return (
    <>
      <h1 id="title">ISR {props.locale}</h1>
      <p id="kind">isr</p>
      <p id="info">{`generation=${props.generation}`}</p>
      <LocaleInfo props={props} />
    </>
  );
}
