import { after } from "next/server";
import { headers } from "next/headers";

// after() callbacks record which request identity they observed, after
// the response was sent. ?report=a,b,c returns those observations and
// forgets the ones it returned, so long runs do not grow the map.
const observed = new Map<string, string>();

export async function GET(request: Request) {
  const params = new URL(request.url).searchParams;
  const report = params.get("report");
  if (report !== null) {
    const ids = report.split(",");
    const result = Object.fromEntries(ids.map((id) => [id, observed.get(id) ?? null]));
    for (const id of ids) observed.delete(id);
    return Response.json(result);
  }
  const id = params.get("id") ?? "(none)";
  after(async () => {
    await new Promise((resolve) => setTimeout(resolve, 5 + (id.length % 7)));
    const seen = (await headers()).get("x-nextrs-id") ?? "(none)";
    observed.set(id, seen);
  });
  return Response.json({ id });
}
