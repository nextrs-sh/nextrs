import { revalidateTag } from "next/cache";

// Expires a fetch-cache tag at once (route handlers cannot use updateTag).
export const dynamic = "force-dynamic";

export async function GET(request: Request) {
  const tag = new URL(request.url).searchParams.get("tag") ?? "none";
  revalidateTag(tag, { expire: 0 });
  return Response.json({ revalidated: tag });
}
