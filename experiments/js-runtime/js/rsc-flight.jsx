// React Server Components Flight serialization (react-server condition),
// with an async server component, outside Next.js.
import "./web-shim.js";
import { renderToReadableStream } from "react-server-dom-webpack/server.edge";

async function Shelf() {
  await new Promise((resolve) => setTimeout(resolve, 10));
  return (
    <ol>
      {["Noli Me Tangere", "El Filibusterismo"].map((t) => (
        <li key={t}>{t}</li>
      ))}
    </ol>
  );
}

function Page() {
  return (
    <main>
      <h1>Flight payload</h1>
      <Shelf />
    </main>
  );
}

export async function renderFlight() {
  // Empty client manifest: this tree has no "use client" boundaries.
  const stream = renderToReadableStream(<Page />, {});
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let text = "";
  let chunks = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks++;
    text += decoder.decode(value);
  }
  return { chunks, flight: text };
}

globalThis.NextrsFlight = { renderFlight };
