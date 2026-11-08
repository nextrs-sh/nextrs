import type { GetServerSideProps } from "next";
import { cookieValue } from "../../lib/upstream";

type Props = { id: string; query: Record<string, string | string[]>; user: string | null; header: string | null; stamp: number };

// getServerSideProps: params, query, a request header and a cookie, all
// echoed back so the suites can tell one request's props from another's.
export const getServerSideProps: GetServerSideProps<Props> = async (context) => {
  const id = String(context.params?.id);
  // The request's own query; Next adds the route param to it.
  const query = context.query as Record<string, string | string[]>;
  const header = (context.req.headers["x-test-user"] as string | undefined) ?? null;
  const user = cookieValue(context.req.headers.cookie, "user");
  context.res.setHeader("x-book-id", id);
  return { props: { id, query, user, header, stamp: Date.now() } };
};

export default function Book({ id, query, user, header, stamp }: Props) {
  return (
    <>
      <h1 id="title">Book {id}</h1>
      <p id="kind">ssr</p>
      <p id="slug">{id}</p>
      <p id="query">{JSON.stringify(query)}</p>
      <p id="secret">{`user=${user ?? "-"} header=${header ?? "-"}`}</p>
      <p id="info">stamp-{stamp}</p>
    </>
  );
}
