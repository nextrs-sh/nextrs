import { Suspense, cache } from "react";
import { headers } from "next/headers";
import { connection } from "next/server";

// React.cache memoizes per request: every section of one render must see
// the same nonce, and no two requests may share one.
const identity = cache(async () => {
  const id = (await headers()).get("x-nextrs-id") ?? "(none)";
  return { id, nonce: Math.random().toString(36).slice(2, 10) };
});

async function Section({ n }: { n: number }) {
  await new Promise((resolve) => setTimeout(resolve, (n * 7) % 13));
  const { id, nonce } = await identity();
  const again = (await headers()).get("x-nextrs-id");
  return (
    <p className="section">
      section-{n}={id}/{nonce}/{again}
    </p>
  );
}

export default async function CachePage() {
  await connection();
  const { id, nonce } = await identity();
  return (
    <main>
      <p id="top">
        top={id}/{nonce}
      </p>
      {[0, 1, 2, 3, 4].map((n) => (
        <Suspense key={n} fallback={<p>loading {n}</p>}>
          <Section n={n} />
        </Suspense>
      ))}
    </main>
  );
}
