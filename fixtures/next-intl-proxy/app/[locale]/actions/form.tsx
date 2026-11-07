"use client";

import { useActionState } from "react";
import { greet } from "./actions";

type Result = Awaited<ReturnType<typeof greet>> | null;

export function GreetForm() {
  const [result, action, pending] = useActionState(async (_: Result, data: FormData) => greet(data), null);
  return (
    <form action={action}>
      <input id="name" name="name" defaultValue="Ada" />
      <button id="greet" type="submit" disabled={pending}>Greet</button>
      <p id="result">{result ? JSON.stringify(result) : ""}</p>
    </form>
  );
}
