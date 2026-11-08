import type { GetServerSideProps } from "next";
import { upstream } from "../../lib/upstream";

type Props = { key: string; hit: number; auth: string | null };

// Outbound fetch from getServerSideProps, with the caller's Authorization
// forwarded (the upstream echoes it back).
export const getServerSideProps: GetServerSideProps<Props> = async ({ query, req }) => {
  const key = String(query.key ?? "ssr-fetch");
  const headers: Record<string, string> = {};
  if (req.headers.authorization) headers.authorization = req.headers.authorization;
  const res = await fetch(`${upstream()}/data?key=${encodeURIComponent(key)}&delay=${Number(query.delay ?? 0)}`, { headers });
  const data = await res.json();
  return { props: { key: data.key, hit: data.hit, auth: data.auth ?? null } };
};

export default function Fetched({ key, hit, auth }: Props) {
  return (
    <>
      <h1 id="title">Fetched {key}</h1>
      <p id="kind">ssr-fetch</p>
      <p id="result">{JSON.stringify({ key, hit, auth })}</p>
    </>
  );
}
