// Data for the ISR fixtures. One uncached upstream request per render
// (fetch without cache options: Next's "auto no cache" at runtime), so the
// upstream's hit count for a key tells how many times a page was rendered.
// `next.tags` still tags the page itself. Without NEXTRS_UPSTREAM (a build
// for the Rust tests only) nothing is fetched.
export async function renderData(key: string, tags: string[] = []): Promise<{ hit: number | "none" }> {
  const base = process.env.NEXTRS_UPSTREAM;
  if (!base) return { hit: "none" };
  const res = await fetch(`${base}/data?key=${encodeURIComponent(key)}`, tags.length ? { next: { tags } } : undefined);
  return (await res.json()) as { hit: number };
}

// A switch the tests flip in the upstream (POST /flag?name=N&on=1).
export async function flag(name: string): Promise<boolean> {
  const base = process.env.NEXTRS_UPSTREAM;
  if (!base) return false;
  const res = await fetch(`${base}/flag?name=${encodeURIComponent(name)}`);
  return ((await res.json()) as { on: boolean }).on;
}
