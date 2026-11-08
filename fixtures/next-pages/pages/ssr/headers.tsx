import type { GetServerSideProps } from "next";

type Props = { seen: Record<string, string> };

// What the page's getServerSideProps saw of the request: the internal
// headers a client tries to send must be gone.
export const getServerSideProps: GetServerSideProps<Props> = async ({ req, res }) => {
  const seen: Record<string, string> = {};
  for (const name of ["x-test-user", "x-middleware-subrequest", "x-nextjs-data", "x-now-route-matches", "x-matched-path", "x-proxy-user", "x-invoke-path"]) {
    const value = req.headers[name];
    if (value !== undefined) seen[name] = String(value);
  }
  res.setHeader("set-cookie", ["ssr-a=1; Path=/", "ssr-b=2; Path=/; HttpOnly"]);
  return { props: { seen } };
};

export default function Headers({ seen }: Props) {
  return (
    <>
      <h1 id="title">Headers</h1>
      <p id="result">{JSON.stringify(seen)}</p>
    </>
  );
}
