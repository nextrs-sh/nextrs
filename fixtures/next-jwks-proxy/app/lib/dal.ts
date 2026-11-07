import { cache } from "react";
import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import { SESSION_COOKIE, decrypt } from "./session";

// The data access layer: pages, route handlers and Server Actions check the
// session again; the proxy's check is optimistic.
export const verifySession = cache(async () => {
  const session = await decrypt((await cookies()).get(SESSION_COOKIE)?.value);
  if (!session) redirect("/login");
  return { userId: session.userId, name: session.name };
});

export const currentSession = cache(async () => decrypt((await cookies()).get(SESSION_COOKIE)?.value));
