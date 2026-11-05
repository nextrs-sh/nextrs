import { Info } from "../../lib/info";

export default async function Rewritten({
  params,
  searchParams,
}: {
  params: Promise<{ slug: string }>;
  searchParams: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { slug } = await params;
  const query = await searchParams;
  return (
    <main>
      <h1 id="title">rewritten</h1>
      <p id="slug">{slug}</p>
      <pre id="query">{JSON.stringify(Object.keys(query).sort().map((k) => [k, query[k]]))}</pre>
      <Info />
    </main>
  );
}
