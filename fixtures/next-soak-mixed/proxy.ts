import { NextResponse, type NextRequest } from "next/server";

const SESSION = "synthetic-valid-session";

// The App Router soak's proxied pages (the token forwarded as a request
// header) and a session check in front of the Pages Router's /p/secure.
export function proxy(request: NextRequest) {
  if (request.nextUrl.pathname.startsWith("/p/secure")) {
    if (request.cookies.get("session")?.value !== SESSION) {
      const login = request.nextUrl.clone();
      login.pathname = "/p/ssg";
      login.search = "?login=required";
      return NextResponse.redirect(login);
    }
    return NextResponse.next();
  }
  const headers = new Headers(request.headers);
  headers.set("x-soak-proxied", request.headers.get("x-soak-token") ?? "none");
  return NextResponse.next({ request: { headers } });
}

export const config = { matcher: ["/soak/proxied/:path*", "/p/secure/:path*"] };
