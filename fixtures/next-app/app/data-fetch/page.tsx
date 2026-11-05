import { headers } from "next/headers";
import { connection } from "next/server";
import { upstream } from "../lib/upstream";

// Server rendering with an uncached upstream fetch. The request identity
// read after the fetch must still be this request's.
export default async function DataFetch() {
  await connection();
  const id = (await headers()).get("x-nextrs-id") ?? "(none)";
  const res = await fetch(upstream(`/data?key=${encodeURIComponent(id)}&delay=${id.length % 7}`), { cache: "no-store" });
  const data = (await res.json()) as { key: string; hit: number };
  const after = (await headers()).get("x-nextrs-id") ?? "(none)";
  return (
    <main>
      <p id="data">
        id={id} key={data.key} hit={data.hit} after={after}
      </p>
    </main>
  );
}
