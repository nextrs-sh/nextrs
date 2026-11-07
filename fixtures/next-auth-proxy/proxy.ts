import { NextResponse, type NextRequest } from "next/server";
import { cookies } from "next/headers";
import { SESSION_COOKIE, decrypt } from "./app/lib/session";

// The Next.js authentication guide's optimistic check: read the session
// cookie, verify it with jose, redirect anonymous visitors of protected
// paths to the login page (keeping where they were going), and send
// signed-in visitors of /login to the dashboard.
const protectedRoutes = ["/dashboard", "/account", "/reports", "/actions/protected", "/api/private"];

export default async function proxy(req: NextRequest) {
  const path = req.nextUrl.pathname;
  const isProtected = protectedRoutes.some((route) => path === route || path.startsWith(`${route}/`));
  const session = await decrypt((await cookies()).get(SESSION_COOKIE)?.value);

  if (isProtected && !session) {
    if (path.startsWith("/api/")) return NextResponse.json({ error: "unauthorized" }, { status: 401 });
    const login = new URL("/login", req.nextUrl);
    login.searchParams.set("next", `${path}${req.nextUrl.search}`);
    return NextResponse.redirect(login);
  }
  if (path === "/login" && session) return NextResponse.redirect(new URL("/dashboard", req.nextUrl));

  // The verified identity, for routes that want it; a client's own value
  // of the header is dropped.
  const headers = new Headers(req.headers);
  headers.delete("x-auth-user");
  if (session) headers.set("x-auth-user", session.userId);
  return NextResponse.next({ request: { headers } });
}

export const config = {
  matcher: ["/((?!_next/static|_next/image|favicon.ico|api/health).*)"],
};
