"use client";

import { useActionState } from "react";
import { inspect } from "./actions";

export function InspectForm() {
  const [state, action, pending] = useActionState((_previous: unknown, formData: FormData) => inspect(formData), null);
  return (
    <form action={action}>
      <input name="value" defaultValue="from-browser" />
      <button id="inspect" type="submit" disabled={pending}>inspect</button>
      <pre id="result">{state ? JSON.stringify(state) : ""}</pre>
    </form>
  );
}
