"use client";

import { useActionState } from "react";
import { recordProtected } from "./actions";

type Result = Awaited<ReturnType<typeof recordProtected>> | null;

export function ProtectedForm({ ledgerKey }: { ledgerKey: string }) {
  const [result, action, pending] = useActionState(async (_: Result, data: FormData) => recordProtected(data), null);
  return (
    <form action={action}>
      <input type="hidden" name="key" value={ledgerKey} />
      <input id="mutation-id" name="id" defaultValue="browser" />
      <button id="record" type="submit" disabled={pending}>Record</button>
      <p id="result">{result ? JSON.stringify(result) : ""}</p>
    </form>
  );
}
