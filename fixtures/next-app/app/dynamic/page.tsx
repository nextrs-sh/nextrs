import { Suspense } from "react";
import { connection } from "next/server";

async function loadShelf(): Promise<string[]> {
  await new Promise((resolve) => setTimeout(resolve, 10));
  return ["Noli Me Tangere", "El Filibusterismo", "Florante at Laura"];
}

async function SlowSection() {
  await new Promise((resolve) => setTimeout(resolve, 50));
  return <p id="slow">Streamed after Suspense boundary</p>;
}

export default async function DynamicPage() {
  // Opts the route out of prerendering: rendering waits for a real request.
  await connection();
  const shelf = await loadShelf();
  return (
    <main>
      <h1 id="title">Dynamic server component</h1>
      <ol id="shelf">
        {shelf.map((title) => (
          <li key={title}>{title}</li>
        ))}
      </ol>
      <Suspense fallback={<p id="slow">Loading…</p>}>
        <SlowSection />
      </Suspense>
    </main>
  );
}
