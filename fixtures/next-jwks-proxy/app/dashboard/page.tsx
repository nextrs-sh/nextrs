import { headers } from "next/headers";
import { verifySession } from "../lib/dal";
import { logout } from "../login/actions";

export default async function Dashboard() {
  const session = await verifySession();
  const proxyUser = (await headers()).get("x-auth-sub");
  return (
    <main>
      <h1 id="title">Dashboard</h1>
      <p id="secret">dashboard of {session.name}</p>
      <p id="info">{JSON.stringify({ userId: session.userId, proxyUser })}</p>
      <form action={logout}>
        <button id="logout" type="submit">Log out</button>
      </form>
    </main>
  );
}
