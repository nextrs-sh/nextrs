import { cookies } from 'next/headers';
import { setFlavor } from '../server';

// Dynamic (reads cookies): the action sets or deletes a cookie and the
// page re-renders with it.
export default async function CookiesPage() {
  // A cookie deleted by the action reads as an empty value in the re-render.
  const flavor = (await cookies()).get('nextrs-flavor')?.value || 'unset';
  return (
    <main>
      <h1>Cookie action</h1>
      <p id="flavor">{flavor}</p>
      <form action={setFlavor} id="cookie-form">
        <input name="flavor" id="flavor-input" defaultValue="mint" />
        <button type="submit" id="cookie-submit">Set</button>
      </form>
      <form action={setFlavor} id="cookie-clear-form">
        <input type="hidden" name="flavor" value="" />
        <button type="submit" id="cookie-clear">Clear</button>
      </form>
    </main>
  );
}
