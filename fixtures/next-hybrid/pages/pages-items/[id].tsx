import type { GetServerSideProps } from "next";
import Link from "next/link";

type Props = { id: string; user: string };

export const getServerSideProps: GetServerSideProps<Props> = async ({ params, req }) => {
  const id = String(params?.id);
  if (id === "missing") return { notFound: true };
  const user = /(?:^|; )user=([^;]*)/.exec(req.headers.cookie ?? "")?.[1] ?? "-";
  return { props: { id, user } };
};

export default function PagesItem({ id, user }: Props) {
  return (
    <>
      <h1 id="title">Pages item {id}</h1>
      <p id="kind">pages-ssr</p>
      <p id="slug">{id}</p>
      <p id="secret">user={user}</p>
      <Link href="/app-items/4" id="to-app-dynamic">To App item 4</Link>
    </>
  );
}
