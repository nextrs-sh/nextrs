import { cacheLife, cacheTag } from "next/cache";

// Cached server functions ("use cache"), keyed by Next on their arguments.
// Each call that is not served from the cache makes one upstream request,
// so the upstream's hit count for the key is the number of executions.
async function upstreamHit(key: string): Promise<number> {
  const base = process.env.NEXTRS_UPSTREAM;
  if (!base) return 0;
  const res = await fetch(`${base}/data?key=${encodeURIComponent(key)}`);
  return ((await res.json()) as { hit: number }).hit;
}

export async function getBooks(shelf: string) {
  "use cache";
  cacheLife("minutes");
  cacheTag("books", `shelf-${shelf}`);
  return { shelf, hit: await upstreamHit(`cc-books-${shelf}`), at: Date.now() };
}

export async function getShort(key: string) {
  "use cache";
  cacheLife("short");
  cacheTag("short");
  return { key, hit: await upstreamHit(`cc-short-${key}`), at: Date.now() };
}
