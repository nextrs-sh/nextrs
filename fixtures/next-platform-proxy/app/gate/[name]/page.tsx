import { cookies, headers } from "next/headers";

export default async function Gate({ params }: { params: Promise<{ name: string }> }) {
  const { name } = await params;
  const h = await headers();
  const c = await cookies();
  return (
    <main>
      <h1 id="title">Gate</h1>
      <p id="slug">{name}</p>
      <p id="info">
        {JSON.stringify({
          flag: h.get("x-flag"),
          identity: h.get("x-identity"),
          seen: h.get("x-identity-seen"),
          identityCookie: c.get("identity")?.value ?? null,
        })}
      </p>
    </main>
  );
}
