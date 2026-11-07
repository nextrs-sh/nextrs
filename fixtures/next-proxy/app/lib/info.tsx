import { cookies, headers } from "next/headers";

// What the route saw of the request after the proxy: the headers and
// cookies it may have changed. Rendering it makes the route dynamic.
export async function requestInfo() {
  const h = await headers();
  const c = await cookies();
  return {
    proxyRequest: h.get("x-proxy-request"),
    proxyPathname: h.get("x-proxy-pathname"),
    removeMe: h.get("x-remove-me"),
    user: h.get("x-user"),
    middlewareHeaders: [...h.keys()].filter((k) => k.startsWith("x-middleware")).sort(),
    // Every request header name except the ones clients and servers vary.
    headerNames: [...h.keys()].filter((k) => !["host", "connection", "user-agent", "accept", "accept-encoding", "accept-language", "sec-fetch-mode", "x-forwarded-for", "x-forwarded-host", "x-forwarded-port", "x-forwarded-proto"].includes(k)).sort(),
    proxyVisit: c.get("proxy-visit")?.value ?? null,
    proxyRequestCookie: c.get("proxy-request-cookie")?.value ?? null,
    proxyCustom: c.get("proxy-custom")?.value ?? null,
    cookieNames: c.getAll().map((x) => x.name).sort(),
  };
}

export async function Info() {
  return <pre id="info">{JSON.stringify(await requestInfo())}</pre>;
}
