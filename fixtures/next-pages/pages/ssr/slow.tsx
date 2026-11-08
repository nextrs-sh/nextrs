import type { GetServerSideProps } from "next";
import { delay } from "../../lib/upstream";

type Props = { id: string; waited: number };

// An awaited delay inside getServerSideProps: concurrent requests overlap.
export const getServerSideProps: GetServerSideProps<Props> = async ({ query }) => {
  const waited = Math.min(Number(query.ms ?? 20), 20_000);
  await delay(waited);
  return { props: { id: String(query.id ?? "none"), waited } };
};

export default function Slow({ id, waited }: Props) {
  return (
    <>
      <h1 id="title">Slow {id}</h1>
      <p id="slug">{id}</p>
      <p id="info">waited {waited}</p>
    </>
  );
}
