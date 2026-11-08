import { NextResponse, type NextRequest } from "next/server";

// A synthetic identity: the fixture's "logged in" cookie value.
const SESSION = "synthetic-valid-session";

// nextUrl keeps basePath (new URL(path, request.url) would drop it).
function at(request: NextRequest, path: string) {
  const url = request.nextUrl.clone();
  const [pathname, search] = path.split("?");
  url.pathname = pathname;
  url.search = search ? `?${search}` : "";
  return url;
}

export function proxy(request: NextRequest) {
  const pathname = request.nextUrl.pathname.replace(/(.)\/$/, "$1");
  if (pathname === "/protected" || pathname.startsWith("/protected/")) {
    if (request.cookies.get("session")?.value !== SESSION) {
      const login = at(request, "/about");
      login.searchParams.set("login", "required");
      return NextResponse.redirect(login);
    }
    const headers = new Headers(request.headers);
    headers.set("x-proxy-user", "verified");
    return NextResponse.next({ request: { headers } });
  }
  if (pathname === "/mw/rewrite-book") return NextResponse.rewrite(at(request, "/books/99?from=proxy"));
  if (pathname === "/mw/rewrite-post") return NextResponse.rewrite(at(request, "/posts/beta"));
  if (pathname === "/mw/redirect") return NextResponse.redirect(at(request, "/posts/alpha"));
  if (pathname === "/mw/json") return NextResponse.json({ fromProxy: true }, { status: 202 });
  const response = NextResponse.next();
  response.headers.set("x-proxy-seen", pathname);
  return response;
}

export const config = {
  matcher: ["/protected/:path*", "/mw/:path*", "/books/:id", "/api/echo"],
};
