"use server";

import { currentSession } from "../../lib/dal";

const upstream = (path: string) => `${process.env.NEXTRS_UPSTREAM}${path}`;

// A mutation that checks the session itself: a Server Action can be posted
// to any page, so the proxy's path check alone does not protect it.
export async function recordProtected(formData: FormData) {
  const session = await currentSession();
  if (!session) return { recorded: null, denied: "unauthenticated" };
  const key = String(formData.get("key") ?? "default");
  const id = `${session.userId}:${String(formData.get("id") ?? "")}`;
  await fetch(upstream(`/mutations?key=${encodeURIComponent(key)}&id=${encodeURIComponent(id)}`), { method: "POST" });
  return { recorded: id, user: session.userId };
}
