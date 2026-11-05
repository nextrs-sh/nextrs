import { Suspense } from "react";
import { connection } from "next/server";
import { getBooks } from "../data";

// Dynamic page (connection()) calling a cached function with an argument
// from the query: every request goes through the "use cache" handler.
async function Books({ searchParams }: { searchParams: Promise<{ shelf?: string }> }) {
  await connection();
  const shelf = (await searchParams).shelf ?? "main";
  const books = await getBooks(shelf);
  return <p id="books">{`shelf=${books.shelf} hit=${books.hit} at=${books.at}`}</p>;
}

export default function Page({ searchParams }: { searchParams: Promise<{ shelf?: string }> }) {
  return (
    <main>
      <Suspense fallback={<p>loading</p>}>
        <Books searchParams={searchParams} />
      </Suspense>
    </main>
  );
}
