import { fetchOptions, upstream } from "../lib/upstream";

// A dynamic page whose upstream fetch uses the cache options in its query
// (cache=force-cache, revalidate=N, tag=T). The rendered hit count shows
// whether the data came from the cache.
export default async function DataCache({ searchParams }: { searchParams: Promise<Record<string, string>> }) {
  const params = new URLSearchParams(await searchParams);
  const key = params.get("key") ?? "page";
  const res = await fetch(upstream(`/data?key=${encodeURIComponent(key)}`), fetchOptions(params));
  const data = (await res.json()) as { key: string; hit: number };
  return (
    <main>
      <p id="data">
        key={data.key} hit={data.hit}
      </p>
    </main>
  );
}
