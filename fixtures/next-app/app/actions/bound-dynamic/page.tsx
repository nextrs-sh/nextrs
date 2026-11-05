import { connection } from 'next/server';
import { greet } from '../bound/actions';
import { BoundForms } from '../bound/forms';

// Rendered per request: the closure is encrypted by the runtime (an isolate
// of nextrs) and may be decrypted by another isolate.
export default async function BoundDynamicPage() {
  await connection();
  const rendered = `render-${Date.now()}`;
  async function closure(_state: { greeting: string } | null, formData: FormData) {
    'use server';
    return { greeting: `${String(formData.get('name'))} from ${rendered}` };
  }
  return (
    <main>
      <h1>Bound actions (dynamic)</h1>
      <p id="rendered">{rendered}</p>
      <BoundForms greet={greet.bind(null, 'Hi')} closure={closure} />
    </main>
  );
}
