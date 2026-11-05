import { connection } from 'next/server';
import { upstream } from '../../lib/upstream';
import { invalidate } from '../server';

// Shows cached upstream data (tagged), and offers each cache invalidation
// a Server Action can call.
export default async function RevalidatePage() {
  await connection();
  const res = await fetch(upstream('/data?key=actions-revalidate'), { cache: 'force-cache', next: { tags: ['actions-revalidate'] } });
  const data = (await res.json()) as { hit: number };
  return (
    <main>
      <h1>Revalidation from actions</h1>
      <p id="hit">{data.hit}</p>
      {['revalidatePath', 'revalidateTag', 'updateTag', 'refresh', 'revalidateStaticPath'].map((how) => (
        <form action={invalidate} key={how}>
          <input type="hidden" name="how" value={how} />
          <button type="submit" id={`do-${how}`}>{how}</button>
        </form>
      ))}
    </main>
  );
}
