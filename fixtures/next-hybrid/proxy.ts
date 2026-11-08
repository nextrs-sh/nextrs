import { NextResponse, type NextRequest } from "next/server";

// One session check in front of both routers' /secure pages.
export function proxy(request: NextRequest) {
  if (request.cookies.get("session")?.value !== "synthetic-valid-session") {
    const login = request.nextUrl.clone();
    login.pathname = "/pages-page";
    login.search = "?login=required";
    return NextResponse.redirect(login);
  }
  return NextResponse.next();
}

export const config = { matcher: ["/secure/:path*"] };
