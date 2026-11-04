import { fetchOptions, upstream } from "../../lib/upstream";

// One upstream fetch with the cache options given in the query, reported
// as JSON. x-upstream-auth becomes the upstream request's authorization.
export const dynamic = "force-dynamic";

export async function GET(request: Request) {
  const params = new URL(request.url).searchParams;
  const key = params.get("key") ?? "default";
  const target =
    params.get("target") ??
    upstream(`/data?key=${encodeURIComponent(key)}&delay=${params.get("delay") ?? 0}&status=${params.get("status") ?? 200}`);
  const auth = request.headers.get("x-upstream-auth");
  try {
    const res = await fetch(target, fetchOptions(params, auth ? { authorization: auth } : undefined));
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
