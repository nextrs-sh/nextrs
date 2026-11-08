// Blocks its isolate (soak restart injection: the server's
// --js-stall-timeout terminates and replaces it).
export const dynamic = "force-dynamic";

export function GET(request: Request) {
  const ms = Number(new URL(request.url).searchParams.get("ms") ?? 0);
  const end = Date.now() + ms;
  while (Date.now() < end) {}
  return new Response("spun");
}
