import { redirectTo, revalidateThenRedirect } from '../server';

export default function RedirectPage() {
  return (
    <main>
      <h1>Redirect action</h1>
      <form action={redirectTo} id="redirect-form">
        <input type="hidden" name="to" value="/actions/redirect/done?from=action" />
        <button type="submit" id="redirect-submit">Go</button>
      </form>
      <form action={redirectTo} id="redirect-dynamic-form">
        <input type="hidden" name="to" value="/books/7" />
        <button type="submit" id="redirect-dynamic-submit">Go dynamic</button>
      </form>
      <form action={revalidateThenRedirect} id="revalidate-redirect-form">
        <input type="hidden" name="to" value="/actions/static-time" />
        <button type="submit" id="revalidate-redirect-submit">Revalidate and go</button>
      </form>
    </main>
  );
}
