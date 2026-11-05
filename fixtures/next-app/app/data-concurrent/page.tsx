import { Suspense } from "react";
import { connection } from "next/server";
import { upstream } from "../lib/upstream";

// Several sections fetch the same URL during one render (Next.js dedupes
// them into one upstream request) and two fetch distinct keys.
async function Section({ name, url }: { name: string; url: string }) {
  const res = await fetch(url, { cache: "no-store" });
  const data = (await res.json()) as { key: string; hit: number };
  return (
    <p className="section">
      {name}:{data.key}:{data.hit}
    </p>
  );
}

export default async function DataConcurrent({ searchParams }: { searchParams: Promise<{ key?: string }> }) {
  await connection();
  const key = (await searchParams).key ?? "concurrent";
  const shared = upstream(`/data?key=${encodeURIComponent(key)}&delay=20`);
  return (
    <main>
      {["a", "b", "c"].map((name) => (
        <Suspense key={name} fallback={<p>loading {name}</p>}>
          <Section name={name} url={shared} />
        </Suspense>
      ))}
      {["x", "y"].map((name) => (
        <Suspense key={name} fallback={<p>loading {name}</p>}>
          <Section name={name} url={upstream(`/data?key=${encodeURIComponent(`${key}-${name}`)}`)} />
        </Suspense>
      ))}
    </main>
  );
}
