// Busy-loops on the CPU for ?ms= milliseconds (at most 30 s) without
// yielding, then answers. Used to inject executor stalls under load: with
// a short --js-stall-timeout, NextRS terminates and replaces the isolate.
export async function GET(request: Request) {
  const ms = Math.min(Number(new URL(request.url).searchParams.get("ms") ?? 0), 30_000);
  const until = Date.now() + ms;
  let spins = 0;
  while (Date.now() < until) spins++;
  return Response.json({ ms, spins: spins > 0 });
}
