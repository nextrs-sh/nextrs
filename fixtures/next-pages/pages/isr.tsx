import type { GetStaticProps } from "next";
import { upstream } from "../lib/upstream";

type Props = { built: number; generation: number };

// ISR: revalidate after 2 seconds. Each regeneration fetches the upstream
// counter, so the suites can count regenerations; while the upstream's
// "isr-fail" flag is on, regeneration fails (the stale page stays).
export const getStaticProps: GetStaticProps<Props> = async () => {
  const flag = await fetch(`${upstream()}/flag?name=pages-isr-fail`, { cache: "no-store" }).then((r) => r.json());
  if (flag.on) throw new Error("ISR regeneration failure (synthetic)");
  const data = await fetch(`${upstream()}/data?key=pages-isr`, { cache: "no-store" }).then((r) => r.json());
  return { props: { built: Date.now(), generation: data.hit }, revalidate: 2 };
};

export default function Isr({ built, generation }: Props) {
  return (
    <>
      <h1 id="title">Pages ISR</h1>
      <p id="kind">isr</p>
      <p id="result">generation-{generation}</p>
      <p id="info">built-{built}</p>
    </>
  );
}
