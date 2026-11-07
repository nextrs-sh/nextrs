import { NextResponse, type NextRequest } from "next/server";

// A synthetic identity: the fixture's "logged in" cookie value.
const SESSION = "synthetic-valid-session";
const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// A URL on this app (nextUrl keeps basePath; `new URL(path, request.url)`
// would drop it).
function at(request: NextRequest, path: string) {
  const url = request.nextUrl.clone();
  const [pathname, search] = path.split("?");
  url.pathname = pathname;
  url.search = search ? `?${search}` : "";
  return url;
}

export async function proxy(request: NextRequest) {
  const { searchParams } = request.nextUrl;
  // The same branches with and without trailingSlash.
  const pathname = request.nextUrl.pathname.replace(/(.)\/$/, "$1");

  if (pathname === "/proxy/redirect") return NextResponse.redirect(at(request, "/target"));
  if (pathname === "/proxy/redirect-308") return NextResponse.redirect(at(request, "/target?x=1"), 308);
  if (pathname === "/proxy/redirect-external") return NextResponse.redirect("https://example.com/landing");
  if (pathname === "/proxy/response-redirect") return Response.redirect(at(request, "/target"), 303);
  if (pathname === "/proxy/rewrite") return NextResponse.rewrite(at(request, "/target"));
  if (pathname === "/proxy/rewrite-query") return NextResponse.rewrite(at(request, "/rewritten/from-proxy?added=1"));
  if (pathname.startsWith("/proxy/rewrite-dynamic/")) {
    const slug = pathname.slice("/proxy/rewrite-dynamic/".length);
    return NextResponse.rewrite(at(request, `/rewritten/${slug}${request.nextUrl.search}`));
  }
  if (pathname === "/proxy/rewrite-isr") return NextResponse.rewrite(at(request, "/isr"));
  if (pathname === "/proxy/rewrite-static") return NextResponse.rewrite(at(request, "/static"));
  if (pathname === "/proxy/rewrite-api") return NextResponse.rewrite(at(request, "/api/echo?via=proxy"));
  if (pathname === "/proxy/rewrite-missing") return NextResponse.rewrite(at(request, "/does-not-exist"));
  if (pathname === "/proxy/rewrite-loop") return NextResponse.rewrite(at(request, "/proxy/rewrite-loop"));
  if (pathname === "/proxy/rewrite-protected") return NextResponse.rewrite(at(request, "/protected"));
  if (pathname === "/proxy/rewrite-external") {
    return NextResponse.rewrite(`http://127.0.0.1:${process.env.NEXTRS_CLOSED_PORT ?? "9"}/ext`);
  }
  if (pathname === "/proxy/json") {
    return NextResponse.json(
      { proxied: true, pathname, query: Object.fromEntries(searchParams), protocol: request.nextUrl.protocol },
      { status: 201, headers: { "x-direct": "1" } },
    );
  }
  if (pathname === "/proxy/text") return new Response("teapot from proxy", { status: 418, headers: { "x-direct": "text" } });
  if (pathname === "/proxy/empty") return new NextResponse(null, { status: 204 });
  if (pathname === "/proxy/location-200") return new Response("not a redirect", { status: 200, headers: { location: "/target" } });
  if (pathname === "/proxy/throw") throw new Error("proxy failure (synthetic)");
  // Passes the request's own headers on as response headers: the pattern
  // behind GHSA-4342-x723-ch2f (a client Location header must not redirect).
  if (pathname === "/proxy/reflect") return NextResponse.next({ headers: request.headers });
  if (pathname === "/proxy/rewrite-invalid") return NextResponse.rewrite("not a url");
  if (pathname === "/proxy/spin") {
    // Busy CPU (the stall watchdog's case), not an await.
    const end = Date.now() + Math.min(Number(searchParams.get("ms") ?? 100), 30_000);
    while (Date.now() < end) {
      // spin
    }
  }
  if (pathname === "/proxy/body") {
    const text = await request.text();
    return NextResponse.json({ method: request.method, length: text.length, text });
  }
  if (pathname === "/proxy/body-pass") {
    const text = await request.text();
    const res = NextResponse.rewrite(at(request, "/api/echo"));
    res.headers.set("x-proxy-body-length", String(text.length));
    return res;
  }
  if (pathname === "/proxy/slow") {
    await delay(Math.min(Number(searchParams.get("ms") ?? 100), 30_000));
  }
  if (pathname === "/proxy/async") await delay(25);

  if (pathname === "/protected" || pathname.startsWith("/protected/")) {
    if (request.cookies.get("session")?.value !== SESSION) {
      const login = at(request, "/login");
      login.searchParams.set("next", pathname);
      return NextResponse.redirect(login);
    }
  }

  // Everything else continues to its route, with request headers and
  // cookies changed on the way.
  request.cookies.set("proxy-request-cookie", "injected");
  const requestHeaders = new Headers(request.headers);
  requestHeaders.set("x-proxy-request", "set-by-proxy");
  requestHeaders.set("x-proxy-pathname", pathname);
  requestHeaders.delete("x-remove-me");
  if (pathname === "/protected" || pathname.startsWith("/protected/")) requestHeaders.set("x-user", "synthetic-user");
  else requestHeaders.delete("x-user");
  const response = NextResponse.next({ request: { headers: requestHeaders } });
  response.headers.set("x-proxy-response", "1");
  response.headers.set("x-conflict", "proxy");
  response.cookies.set("proxy-visit", "1", { path: "/" });
  const custom = searchParams.get("setcookie");
  if (custom) response.cookies.set("proxy-custom", custom, { path: "/", httpOnly: true });
  return response;
}

export const config = {
  matcher: [
    "/((?!_next/static|_next/image|excluded|favicon.ico|has-gate|api/health).*)",
    {
      source: "/has-gate/:path*",
      has: [{ type: "header", key: "x-gate", value: "open" }],
      missing: [{ type: "cookie", key: "skip-proxy" }],
    },
  ],
};
