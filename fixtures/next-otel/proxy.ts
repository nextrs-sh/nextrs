import { NextResponse, type NextRequest } from "next/server";
import { trace } from "@opentelemetry/api";

// A proxy that tags the active span (Next's middleware span) and passes
// the trace id on as a request header.
export default function proxy(req: NextRequest) {
  const span = trace.getActiveSpan();
  span?.setAttribute("fixture.proxy", req.nextUrl.pathname);
  const headers = new Headers(req.headers);
  headers.set("x-proxy-trace", span?.spanContext().traceId ?? "none");
  return NextResponse.next({ request: { headers } });
}

export const config = { matcher: ["/api/work", "/page-traced"] };
