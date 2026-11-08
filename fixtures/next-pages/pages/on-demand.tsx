import type { GetStaticProps } from "next";
import { upstream } from "../lib/upstream";

type Props = { generation: number };

// Regenerated only on demand (res.revalidate from /api/revalidate).
export const getStaticProps: GetStaticProps<Props> = async () => {
  const data = await fetch(`${upstream()}/data?key=pages-on-demand`, { cache: "no-store" }).then((r) => r.json());
  return { props: { generation: data.hit } };
};

export default function OnDemand({ generation }: Props) {
  return (
    <>
      <h1 id="title">On-demand ISR</h1>
      <p id="result">generation-{generation}</p>
    </>
  );
}
