export const dynamic = "force-dynamic";

// Test-only: keeps its executor busy for `ms` (default 5 s), longer than
// the runtime's stall limit in the suites, so the executor is restarted.
export function GET(req: Request) {
  const ms = Number(new URL(req.url).searchParams.get("ms") ?? 5000);
  const end = Date.now() + ms;
  while (Date.now() < end) {
    // busy
  }
  return Response.json({ stalled: ms });
}
