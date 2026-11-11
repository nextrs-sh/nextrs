import { headers } from "next/headers";

// Reached through proxy.ts, which forwards the token as x-soak-proxied.
export default async function Proxied({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const h = await headers();
  return <p id="proxied">{`${slug}:${h.get("x-soak-proxied") ?? "missing"}`}</p>;
}
