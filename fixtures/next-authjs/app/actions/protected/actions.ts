"use server";

import { auth } from "../../../auth";

const upstream = (path: string) => `${process.env.NEXTRS_UPSTREAM}${path}`;

// A mutation that checks the session itself.
export async function recordProtected(formData: FormData) {
  const session = await auth();
  if (!session?.user?.id) return { recorded: null, denied: "unauthenticated" };
  const key = String(formData.get("key") ?? "default");
  const id = `${session.user.id}:${String(formData.get("id") ?? "")}`;
  await fetch(upstream(`/mutations?key=${encodeURIComponent(key)}&id=${encodeURIComponent(id)}`), { method: "POST" });
  return { recorded: id, user: session.user.id };
}
