import { flag, renderData } from "../../lib/isr";

// Regeneration that fails while the upstream flag "isr-flaky" is on: Next
// keeps serving the last good page. While "isr-spin" is on, a regeneration
// keeps the CPU busy for 5 s (NextRS's stall watchdog then terminates the
// isolate mid-regeneration).
export const revalidate = 1;

export default async function Flaky() {
  if (await flag("isr-flaky")) throw new Error("ISR regeneration failed on purpose");
  if (await flag("isr-spin")) {
    const end = Date.now() + 5000;
    while (Date.now() < end) {
      // busy on purpose
    }
  }
  const data = await renderData("isr-flaky");
  return (
    <main>
      <p id="stamp">{`stamp-${Date.now()}`}</p>
      <p id="hit">{String(data.hit)}</p>
    </main>
  );
}
