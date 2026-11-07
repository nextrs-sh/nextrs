import { redirect } from "next/navigation";
import { auth, signOut } from "../../auth";

export default async function Dashboard() {
  // The page checks again: the proxy's check is not its authorization.
  const session = await auth();
  if (!session?.user) redirect("/api/auth/signin");
  return (
    <main>
      <h1 id="title">Dashboard</h1>
      <p id="secret">dashboard of {session.user.name}</p>
      <p id="info">{JSON.stringify({ userId: session.user.id, email: session.user.email })}</p>
      <form
        action={async () => {
          "use server";
          await signOut({ redirectTo: "/" });
        }}
      >
        <button id="logout" type="submit">Sign out</button>
      </form>
    </main>
  );
}
