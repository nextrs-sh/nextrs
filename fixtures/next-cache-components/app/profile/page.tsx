import { Suspense } from "react";
import { connection } from "next/server";
import { getShort } from "../data";

// A cached function with a short custom profile (revalidate 2 s).
async function Short({ searchParams }: { searchParams: Promise<{ key?: string }> }) {
  await connection();
  const key = (await searchParams).key ?? "k";
  const value = await getShort(key);
  return <p id="short">{`key=${value.key} hit=${value.hit} at=${value.at}`}</p>;
}

export default function Page({ searchParams }: { searchParams: Promise<{ key?: string }> }) {
  return (
    <main>
      <Suspense fallback={<p>loading</p>}>
        <Short searchParams={searchParams} />
      </Suspense>
    </main>
  );
}
