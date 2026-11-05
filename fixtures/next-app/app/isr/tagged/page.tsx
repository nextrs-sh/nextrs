import Link from "next/link";
import { renderData } from "../../lib/isr";
import { isrBound, isrInvalidate } from "../actions";

// ISR page (an hour) tagged "isr-tagged", with Server Actions that mutate
// and then revalidate it each way. The hit count is the render count.
export const revalidate = 3600;

export default async function Tagged() {
  const data = await renderData("isr-tagged", ["isr-tagged"]);
  return (
    <main>
      <h1>ISR with actions</h1>
      <p id="stamp">{`stamp-${Date.now()}`}</p>
      <p id="hit">{String(data.hit)}</p>
      {["revalidatePath", "revalidateTag", "updateTag", "none"].map((how) => (
        <form action={isrInvalidate} key={how}>
          <input type="hidden" name="how" value={how} />
          <button type="submit" id={`isr-${how}`}>{how}</button>
        </form>
      ))}
      <form action={isrBound.bind(null, "bound-label")}>
        <button type="submit" id="isr-bound">bound</button>
      </form>
      <Link href="/isr/one" id="to-one">one</Link>
    </main>
  );
}
