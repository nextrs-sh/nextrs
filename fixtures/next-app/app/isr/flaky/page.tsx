import { flag, renderData } from "../../lib/isr";

// Regeneration that fails while the upstream flag "isr-flaky" is on: Next
// keeps serving the last good page.
export const revalidate = 1;

export default async function Flaky() {
  if (await flag("isr-flaky")) throw new Error("ISR regeneration failed on purpose");
  const data = await renderData("isr-flaky");
  return (
    <main>
      <p id="stamp">{`stamp-${Date.now()}`}</p>
      <p id="hit">{String(data.hit)}</p>
    </main>
  );
}
