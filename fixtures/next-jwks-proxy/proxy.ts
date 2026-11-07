import { NextResponse, type NextRequest } from "next/server";
import { cookies } from "next/headers";
import { SESSION_COOKIE, decrypt } from "./app/lib/session";
import { rejection, verifyAccessToken } from "./app/lib/tokens";

// Two kinds of identity:
// * API paths take a bearer access token from the OpenID provider,
//   verified against its JWKS; the verified subject is passed on.
// * Pages take the application's ES256 session cookie; anonymous
//   visitors of protected pages go to the login page.
const protectedPages = ["/dashboard", "/account", "/reports", "/actions/protected"];
// API paths that authenticate on their own (the session cookie).
const sessionApi = ["/api/app-token"];

export default async function proxy(req: NextRequest) {
  const path = req.nextUrl.pathname;
  // Fault injection for the soak: busy CPU past the stall limit (the
  // executor is terminated and replaced).
  if (path === "/proxy/spin") {
    const end = Date.now() + Math.min(Number(req.nextUrl.searchParams.get("ms") ?? 100), 30_000);
    while (Date.now() < end) {
      // spin
    }
    return NextResponse.json({ spun: true });
  }
  // A client's own identity headers never pass.
  const headers = new Headers(req.headers);
  headers.delete("x-auth-sub");
  headers.delete("x-auth-tenant");

  if (path.startsWith("/api/") && !sessionApi.some((p) => path === p || path.startsWith(`${p}/`))) {
    const tenant = /^\/api\/t\/([^/]+)\//.exec(path)?.[1] ?? "main";
    const result = await verifyAccessToken(req.headers.get("authorization"), tenant);
    if (!result.ok) return rejection(result);
    headers.set("x-auth-sub", String(result.claims.sub));
    headers.set("x-auth-tenant", result.tenant);
    return NextResponse.next({ request: { headers } });
  }

  const isProtected = protectedPages.some((route) => path === route || path.startsWith(`${route}/`));
  const session = await decrypt((await cookies()).get(SESSION_COOKIE)?.value);
  if (isProtected && !session) {
    const login = new URL("/login", req.nextUrl);
    login.searchParams.set("next", `${path}${req.nextUrl.search}`);
    return NextResponse.redirect(login);
  }
  if (path === "/login" && session) return NextResponse.redirect(new URL("/dashboard", req.nextUrl));
  if (session) headers.set("x-auth-sub", session.userId);
  return NextResponse.next({ request: { headers } });
}

export const config = {
  matcher: ["/((?!_next/static|_next/image|favicon.ico|api/health).*)"],
};
