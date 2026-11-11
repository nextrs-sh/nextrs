import type { GetStaticProps } from "next";
import { upstream } from "../lib/upstream";
import { fromContext, LocaleInfo, type LocaleProps } from "../lib/locale";

type Props = LocaleProps & { generation: number; built: number };

// Regenerated only on demand (res.revalidate): each locale's entry keeps
// its generation until its own path is revalidated.
export const getStaticProps: GetStaticProps<Props> = async (context) => {
  const locale = context.locale ?? "-";
  const data = await fetch(`${upstream()}/data?key=i18n-on-demand-${process.env.NEXTRS_SIDE ?? "x"}-${locale}`, { cache: "no-store" }).then((r) => r.json());
  return { props: { ...fromContext(context), generation: data.hit, built: Date.now() } };
};

export default function OnDemand(props: Props) {
  return (
    <>
      <h1 id="title">On demand {props.locale}</h1>
      <p id="kind">on-demand</p>
      <p id="info">{`generation=${props.generation}`}</p>
      <p id="built">{`built=${props.built}`}</p>
      <LocaleInfo props={props} />
    </>
  );
}
