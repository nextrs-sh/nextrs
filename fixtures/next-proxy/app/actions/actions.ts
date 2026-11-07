"use server";

import { cookies, headers } from "next/headers";
import { revalidatePath } from "next/cache";

const upstream = (path: string) => `${process.env.NEXTRS_UPSTREAM}${path}`;

// Reports what the action saw after the proxy, and sets a cookie.
export async function inspect(formData: FormData) {
  const h = await headers();
  const c = await cookies();
  c.set("action-cookie", String(formData.get("value") ?? "set"), { path: "/" });
  return {
    proxyRequest: h.get("x-proxy-request"),
    proxyPathname: h.get("x-proxy-pathname"),
    proxyVisit: c.get("proxy-visit")?.value ?? null,
    user: h.get("x-user"),
    value: String(formData.get("value") ?? ""),
  };
}

// One side effect in the upstream's ledger per execution.
export async function record(formData: FormData) {
  const key = String(formData.get("key") ?? "default");
  const id = String(formData.get("id") ?? "");
  await fetch(upstream(`/mutations?key=${encodeURIComponent(key)}&id=${encodeURIComponent(id)}`), { method: "POST" });
  return { recorded: id };
}

export async function refreshIsr() {
  revalidatePath("/isr");
  return { revalidated: "/isr" };
}
