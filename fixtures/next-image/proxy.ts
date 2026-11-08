import { NextResponse, type NextRequest } from "next/server";

// Only the soak's proxied pages: every other request (images included)
// never reaches the proxy.
export function proxy(request: NextRequest) {
  const headers = new Headers(request.headers);
  headers.set("x-soak-proxied", request.headers.get("x-soak-token") ?? "none");
  return NextResponse.next({ request: { headers } });
}

export const config = { matcher: "/soak/proxied/:path*" };
