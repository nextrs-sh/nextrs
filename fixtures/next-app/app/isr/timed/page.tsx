import { renderData } from "../../lib/isr";

// Time-based ISR: stale 2 s after each generation, then regenerated in
// the background while the stale page is served.
export const revalidate = 2;

export default async function Timed() {
  const data = await renderData("isr-timed");
  return (
    <main>
      <h1>ISR, revalidate 2</h1>
      <p id="stamp">{`stamp-${Date.now()}`}</p>
      <p id="hit">{String(data.hit)}</p>
    </main>
  );
}
