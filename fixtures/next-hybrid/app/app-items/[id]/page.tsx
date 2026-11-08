import { cookies } from "next/headers";
import Link from "next/link";

export const dynamic = "force-dynamic";

export default async function AppItem({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const user = (await cookies()).get("user")?.value ?? "-";
  return (
    <>
      <h1 id="title">App item {id}</h1>
      <p id="kind">app-dynamic</p>
      <p id="slug">{id}</p>
      <p id="secret">user={user}</p>
      <Link href="/pages-items/7" id="to-pages-ssr">To Pages item 7</Link>
    </>
  );
}
