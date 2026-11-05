"use server";

import { revalidateTag, updateTag } from "next/cache";

export async function invalidateBooks(formData: FormData) {
  const how = String(formData.get("how"));
  const tag = String(formData.get("tag") ?? "books");
  if (how === "updateTag") updateTag(tag);
  else if (how === "revalidateTag") revalidateTag(tag, "max");
  else throw new Error(`unknown invalidation ${how}`);
}
