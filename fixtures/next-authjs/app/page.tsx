import { auth } from "../auth";

export default async function Home() {
  const session = await auth();
  return (
    <main>
      <h1 id="title">Home</h1>
      <p id="info">{JSON.stringify({ user: session?.user?.id ?? null })}</p>
    </main>
  );
}
