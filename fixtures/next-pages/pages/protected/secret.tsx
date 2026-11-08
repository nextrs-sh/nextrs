import type { GetServerSideProps } from "next";
import { cookieValue } from "../../lib/upstream";

type Props = { user: string; secret: string };

// Protected by proxy.ts (session cookie). The page itself trusts the
// proxy: if the proxy were bypassed (for example through the data route),
// the secret would leak.
export const getServerSideProps: GetServerSideProps<Props> = async ({ req }) => {
  const user = cookieValue(req.headers.cookie, "user") ?? "nobody";
  return { props: { user, secret: `top-secret-for-${user}` } };
};

export default function Secret({ user, secret }: Props) {
  return (
    <>
      <h1 id="title">Protected</h1>
      <p id="slug">{user}</p>
      <p id="secret">{secret}</p>
    </>
  );
}
