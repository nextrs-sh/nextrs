'use client';

import { useActionState } from 'react';

type Greeting = { greeting: string } | null;

// Receives Server Actions created (and bound) by a server component.
export function BoundForms({
  greet,
  closure,
}: {
  greet: (state: Greeting, formData: FormData) => Promise<Greeting>;
  closure: (state: Greeting, formData: FormData) => Promise<Greeting>;
}) {
  const [bound, boundAction] = useActionState(greet, null);
  const [closed, closureAction] = useActionState(closure, null);
  return (
    <>
      <form action={boundAction} id="bound-form">
        <input name="name" id="bound-name" defaultValue="Ada" />
        <button type="submit" id="bound-submit">Bound</button>
        <pre id="bound-result">{bound === null ? 'none' : JSON.stringify(bound)}</pre>
      </form>
      <form action={closureAction} id="closure-form">
        <input name="name" id="closure-name" defaultValue="Grace" />
        <button type="submit" id="closure-submit">Closure</button>
        <pre id="closure-result">{closed === null ? 'none' : JSON.stringify(closed)}</pre>
      </form>
    </>
  );
}
