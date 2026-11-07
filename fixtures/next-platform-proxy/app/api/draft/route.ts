import { draftMode } from "next/headers";
import { redirect } from "next/navigation";

// Enables Draft Mode for a post when the test-only secret matches.
export async function GET(request: Request) {
  const url = new URL(request.url);
  if (!process.env.DRAFT_SECRET || url.searchParams.get("secret") !== process.env.DRAFT_SECRET) {
    return Response.json({ error: "invalid secret" }, { status: 401 });
  }
  (await draftMode()).enable();
  redirect(`/posts/${url.searchParams.get("slug") ?? "a"}`);
}
