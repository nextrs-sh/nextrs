import { cookies } from "next/headers";

// Declares ISR but reads a cookie: Next renders it per request and never
// caches it. Each user must see only their own value.
export const revalidate = 60;

export default async function Private() {
  const user = (await cookies()).get("isr-user")?.value ?? "anonymous";
  return (
    <main>
      <p id="user">{user}</p>
      <p id="stamp">{`stamp-${Date.now()}`}</p>
    </main>
  );
}
