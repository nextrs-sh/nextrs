import { failWith } from '../server';
import { ClientErrors } from './client';

export default function ErrorPage() {
  return (
    <main>
      <h1>Error action</h1>
      <form action={failWith} id="error-form">
        <input type="hidden" name="kind" value="error" />
        <button type="submit" id="error-submit">Fail</button>
      </form>
      <form action={failWith} id="not-found-form">
        <input type="hidden" name="kind" value="not-found" />
        <button type="submit" id="not-found-submit">Not found</button>
      </form>
      <ClientErrors />
    </main>
  );
}
