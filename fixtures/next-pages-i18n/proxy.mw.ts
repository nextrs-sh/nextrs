import { NextResponse, type NextRequest } from "next/server";

// proxy.mw.ts: picked up through next.config's pageExtensions, so the
// no-detect build has no proxy (Next's data routes then behave differently).

// A synthetic identity: the fixture's "logged in" cookie value.
export const SESSION = "synthetic-valid-session";

// With i18n, Next gives the proxy the pathname without its locale
// (nextUrl.locale holds it), and the matcher accepts every locale prefix.
export function proxy(request: NextRequest) {
  const { pathname, locale } = request.nextUrl;
  const path = pathname.replace(/(.)\/$/, "$1");
  if (path === "/protected" || path.startsWith("/protected/")) {
    if (request.cookies.get("session")?.value !== SESSION) {
      const login = request.nextUrl.clone();
      login.pathname = "/about";
      login.search = "?login=required";
      return NextResponse.redirect(login);
    }
    const headers = new Headers(request.headers);
    headers.set("x-proxy-user", "verified");
    return NextResponse.next({ request: { headers } });
  }
  if (path === "/mw/rewrite-ssr") {
    const url = request.nextUrl.clone();
    url.pathname = "/ssr";
    url.search = "?from=proxy";
    return NextResponse.rewrite(url);
  }
  if (path === "/mw/to-ja") {
    const url = request.nextUrl.clone();
    url.locale = "ja";
    url.pathname = "/ssr";
    return NextResponse.rewrite(url);
  }
  const response = NextResponse.next();
  response.headers.set("x-proxy-seen", `${locale}:${path}`);
  // The URL Next handed the proxy, as received.
  response.headers.set("x-proxy-url", new URL(request.url).pathname);
  return response;
}

export const config = {
  matcher: ["/protected/:path*", "/mw/:path*", "/ssr", "/api/whoami"],
};
