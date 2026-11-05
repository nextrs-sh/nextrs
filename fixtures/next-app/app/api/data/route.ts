import { fetchOptions, upstream } from "../../lib/upstream";

// One upstream fetch with the cache options given in the query, reported
// as JSON. x-upstream-auth and x-upstream-cookie become the upstream
// request's authorization and cookie headers; ?hold=ms delays the handler
// first (so concurrent requests spread over isolates).
export const dynamic = "force-dynamic";

export async function GET(request: Request) {
  const params = new URL(request.url).searchParams;
  const key = params.get("key") ?? "default";
  const target =
    params.get("target") ??
    upstream(`/data?key=${encodeURIComponent(key)}&delay=${params.get("delay") ?? 0}&status=${params.get("status") ?? 200}`);
  const hold = Number(params.get("hold") ?? 0);
  if (hold) await new Promise((resolve) => setTimeout(resolve, hold));
  const forwarded: Record<string, string> = {};
  const auth = request.headers.get("x-upstream-auth");
  if (auth) forwarded.authorization = auth;
  const cookie = request.headers.get("x-upstream-cookie");
  if (cookie) forwarded.cookie = cookie;
  try {
    const res = await fetch(target, fetchOptions(params, forwarded));
    const text = await res.text();
    let body: unknown = text;
    try {
      body = JSON.parse(text);
    } catch {}
    return Response.json({ status: res.status, body });
  } catch (error) {
    const cause = (error as { cause?: { code?: string } }).cause;
    return Response.json(
      { error: (error as Error).name, message: (error as Error).message, code: cause?.code ?? null },
      { status: 502 },
    );
  }
}
