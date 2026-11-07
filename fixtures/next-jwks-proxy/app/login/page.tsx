import { login } from "./actions";
import { safeNext } from "../lib/users";

export default async function Login({ searchParams }: { searchParams: Promise<{ next?: string; error?: string }> }) {
  const { next, error } = await searchParams;
  return (
    <main>
      <h1 id="title">Log in</h1>
      <p id="query">{JSON.stringify({ next: next ?? null, error: error ?? null })}</p>
      <form action={login}>
        <input type="hidden" name="next" value={safeNext(next)} />
        <input id="username" name="username" />
        <input id="password" name="password" type="password" />
        <button id="submit" type="submit">Log in</button>
      </form>
    </main>
  );
}
