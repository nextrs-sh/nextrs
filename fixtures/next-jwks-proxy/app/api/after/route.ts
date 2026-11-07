import { after } from "next/server";

export const dynamic = "force-dynamic";

// Work after the response: records the verified subject in the upstream's
// ledger once the response has gone.
export function GET(req: Request) {
  const key = new URL(req.url).searchParams.get("key") ?? "after";
  const sub = req.headers.get("x-auth-sub") ?? "anonymous";
  after(async () => {
    await fetch(`${process.env.NEXTRS_UPSTREAM}/mutations?key=${encodeURIComponent(key)}&id=${encodeURIComponent(sub)}`, { method: "POST" });
  });
  return Response.json({ scheduled: key, sub });
}
