import type { GetServerSideProps } from "next";
import { upstream } from "../../lib/upstream";

type Props = { name: string; hit: number; auth: string | null };

// Outbound fetch from getServerSideProps, with the caller's Authorization
// forwarded (the upstream echoes it back).
export const getServerSideProps: GetServerSideProps<Props> = async ({ query, req }) => {
  const key = String(query.key ?? "ssr-fetch");
  const headers: Record<string, string> = {};
  if (req.headers.authorization) headers.authorization = req.headers.authorization;
  const res = await fetch(`${upstream()}/data?key=${encodeURIComponent(key)}&delay=${Number(query.delay ?? 0)}`, { headers });
  const data = await res.json();
  return { props: { name: data.key, hit: data.hit, auth: data.auth ?? null } };
};

// `key` is reserved by React, so the fetched key is passed as `name`.
export default function Fetched({ name, hit, auth }: Props) {
  return (
    <>
      <h1 id="title">Fetched {name}</h1>
      <p id="kind">ssr-fetch</p>
      <p id="result">{JSON.stringify({ key: name, hit, auth })}</p>
    </>
  );
}
