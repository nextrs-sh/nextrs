import { headers } from "next/headers";

// Requests with an odd x-nextrs-seq throw inside the page; error.tsx
// renders the boundary. Even ones render their own identity.
export default async function BoundaryPage() {
  const list = await headers();
  const id = list.get("x-nextrs-id") ?? "(none)";
  const seq = Number(list.get("x-nextrs-seq") ?? 0);
  await new Promise((resolve) => setTimeout(resolve, seq % 5));
  if (seq % 2 === 1) throw new Error(`boundary failure for ${id}`);
  return (
    <main>
      <p id="ok">ok={id}</p>
    </main>
  );
}
