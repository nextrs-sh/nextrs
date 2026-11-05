import { greet } from './actions';
import { BoundForms } from './forms';

// Prerendered: the inline action's closure (secret, edition) is encrypted
// with the build's Server Actions key at build time, and decrypted by the
// runtime when the form is submitted.
export default function BoundPage() {
  const secret = 'closure-secret-1d';
  const edition = { number: 3, title: 'Third' };
  async function closure(_state: { greeting: string } | null, formData: FormData) {
    'use server';
    return { greeting: `${String(formData.get('name'))} read edition ${edition.number} (${edition.title}) with ${secret}` };
  }
  return (
    <main>
      <h1>Bound actions</h1>
      <BoundForms greet={greet.bind(null, 'Hello')} closure={closure} />
    </main>
  );
}
