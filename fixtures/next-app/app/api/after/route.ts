import { after } from "next/server";
import { headers } from "next/headers";

// after() callbacks record which request identity they observed, after
// the response was sent. ?report=a,b,c returns those observations.
const observed = new Map<string, string>();

export async function GET(request: Request) {
  const params = new URL(request.url).searchParams;
  const report = params.get("report");
  if (report !== null) {
    return Response.json(Object.fromEntries(report.split(",").map((id) => [id, observed.get(id) ?? null])));
  }
  const id = params.get("id") ?? "(none)";
  after(async () => {
    await new Promise((resolve) => setTimeout(resolve, 5 + (id.length % 7)));
    const seen = (await headers()).get("x-nextrs-id") ?? "(none)";
    observed.set(id, seen);
  });
  return Response.json({ id });
}
