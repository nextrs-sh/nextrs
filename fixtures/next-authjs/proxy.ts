// Auth.js's `auth` as the proxy: the `authorized` callback decides, and an
// anonymous visitor of a protected page is sent to the sign-in page.
export { auth as proxy } from "./auth";

export const config = {
  matcher: ["/((?!api/auth|api/health|_next/static|_next/image|favicon.ico).*)"],
};
