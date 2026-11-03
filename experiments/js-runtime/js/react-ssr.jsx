// React DOM SSR (not Next.js): the same React 19.3.1 the fixture uses.
import "./web-shim.js";
import { Suspense, use } from "react";
import { renderToReadableStream, renderToString } from "react-dom/server.edge";

const shelf = ["Noli Me Tangere", "El Filibusterismo", "Florante at Laura"];

function Slow({ promise }) {
  return <p id="slow">{use(promise)}</p>;
}

function App({ promise }) {
  return (
    <html lang="en">
      <body>
        <main>
          <h1 id="title">Dynamic server component</h1>
          <ol id="shelf">
            {shelf.map((title) => (
              <li key={title}>{title}</li>
            ))}
          </ol>
          <Suspense fallback={<p id="slow">Loading…</p>}>
            <Slow promise={promise} />
          </Suspense>
        </main>
      </body>
    </html>
  );
}

const delay = (ms, value) => new Promise((resolve) => setTimeout(() => resolve(value), ms));

export function renderSync() {
  return renderToString(<App promise={new Promise(() => {})} />);
}

// Streams the shell first, then the Suspense boundary once the timer fires.
export async function renderStream() {
  const stream = await renderToReadableStream(
    <App promise={delay(50, "Streamed after Suspense boundary")} />,
  );
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  const chunks = [];
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(decoder.decode(value));
  }
  return { chunks: chunks.length, html: chunks.join("") };
}

globalThis.NextrsReactSSR = { renderSync, renderStream };
