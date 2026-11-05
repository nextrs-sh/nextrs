import { cookies, headers } from "next/headers";

export const dynamic = "force-dynamic";

async function echo(request: Request) {
  const h = await headers();
  const c = await cookies();
  const url = new URL(request.url);
  const body = request.method === "GET" || request.method === "HEAD" ? null : await request.text();
  return Response.json({
    method: request.method,
    pathname: url.pathname,
    query: [...url.searchParams].sort(),
    proxyRequest: h.get("x-proxy-request"),
    proxyPathname: h.get("x-proxy-pathname"),
    removeMe: h.get("x-remove-me"),
    middlewareHeaders: [...h.keys()].filter((k) => k.startsWith("x-middleware")).sort(),
    headerNames: [...h.keys()].filter((k) => !["host", "connection", "user-agent", "accept", "accept-encoding", "accept-language", "sec-fetch-mode", "x-forwarded-for", "x-forwarded-host", "x-forwarded-port", "x-forwarded-proto"].includes(k)).sort(),
    cookies: c.getAll().map((x) => [x.name, x.value]).sort(),
    body,
  });
}

export const GET = echo;
export const POST = echo;
