import type { GetServerSideProps } from "next";
import { cookieValue } from "../../lib/upstream";
import { fromContext, type LocaleProps } from "../../lib/locale";

const SESSION = "synthetic-valid-session";

type Props = LocaleProps & { secret: string; proxy: string | null };

// Checks the session itself as well as the proxy: a config rewrite into
// this path skips the proxy (next start does the same), so the proxy alone
// would not protect it.
export const getServerSideProps: GetServerSideProps<Props> = async (context) => {
  if (cookieValue(context.req.headers.cookie, "session") !== SESSION) {
    return { redirect: { destination: "/about?login=page", permanent: false } };
  }
  const proxy = (context.req.headers["x-proxy-user"] as string | undefined) ?? null;
  return { props: { ...fromContext(context), secret: `secret-for-${context.locale}`, proxy } };
};

export default function Secret(props: Props) {
  return (
    <>
      <h1 id="title">Secret</h1>
      <p id="kind">protected</p>
      <p id="secret">{`${props.secret} proxy=${props.proxy ?? "-"}`}</p>
      <p id="locale">{props.locale}</p>
    </>
  );
}
