import type { GetServerSideProps } from "next";
import { cookieValue } from "../../lib/upstream";

// Checks the session itself as well as the proxy.
export const getServerSideProps: GetServerSideProps<{ secret: string }> = async (context) => {
  if (cookieValue(context.req.headers.cookie, "session") !== "synthetic-valid-session") return { notFound: true };
  return { props: { secret: "soak-secret" } };
};

export default function PagesSecure({ secret }: { secret: string }) {
  return <p id="secret">{secret}</p>;
}
