import type { GetServerSideProps } from "next";
import { cookieValue, delay } from "../../lib/upstream";

type Props = { user: string; secret: string; marker: string };

// A per-user page: the identity comes from the request's cookie, and the
// "secret" is derived from it. Two users' pages must never cross.
export const getServerSideProps: GetServerSideProps<Props> = async ({ req, query }) => {
  const user = cookieValue(req.headers.cookie, "user") ?? "anonymous";
  await delay(Number(query.ms ?? 0));
  return { props: { user, secret: `secret-of-${user}`, marker: String(query.marker ?? "") } };
};

export default function WhoAmI({ user, secret, marker }: Props) {
  return (
    <>
      <h1 id="title">Who am I</h1>
      <p id="slug">{user}</p>
      <p id="secret">{secret}</p>
      <p id="info">{marker}</p>
    </>
  );
}
