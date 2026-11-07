"use server";

import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import { SESSION_COOKIE, encrypt } from "../lib/session";
import { USERS, safeNext } from "../lib/users";

export async function login(formData: FormData) {
  const username = String(formData.get("username") ?? "");
  const password = String(formData.get("password") ?? "");
  const next = safeNext(formData.get("next"));
  const user = USERS[username];
  if (!user || user.password !== password) {
    redirect(`/login?error=1&next=${encodeURIComponent(next)}`);
  }
  const expires = new Date(Date.now() + 60 * 60 * 1000);
  const session = await encrypt({ userId: user.userId, name: user.name }, expires);
  (await cookies()).set(SESSION_COOKIE, session, { httpOnly: true, sameSite: "lax", path: "/", expires });
  redirect(next);
}

export async function logout() {
  (await cookies()).delete(SESSION_COOKIE);
  redirect("/login");
}
