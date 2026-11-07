import { context, trace } from "@opentelemetry/api";
import { headers } from "next/headers";

export const dynamic = "force-dynamic";
const tracer = trace.getTracer("fixture");

// A route handler with nested spans across awaits, a timer and an
// outgoing fetch; it reports the ids it saw so the suite can check the
// exported spans against them.
export async function GET(req: Request) {
  const label = new URL(req.url).searchParams.get("label") ?? "x";
  const root = trace.getActiveSpan();
  return tracer.startActiveSpan("work", async (work) => {
    work.setAttribute("fixture.label", label);
    await new Promise((resolve) => setTimeout(resolve, 2));
    const echo = await (await fetch(`${process.env.NEXTRS_UPSTREAM}/echo?label=${encodeURIComponent(label)}`)).json();
    const inner = await new Promise<{ spanId: string; parent: string | undefined }>((resolve) => {
      setTimeout(() => {
        tracer.startActiveSpan("work-inner", (span) => {
          const parent = trace.getSpan(context.active()) === span ? work.spanContext().spanId : undefined;
          span.end();
          resolve({ spanId: span.spanContext().spanId, parent });
        });
      }, 1);
    });
    work.end();
    return Response.json({
      label,
      traceId: work.spanContext().traceId,
      rootSpanId: root?.spanContext().spanId ?? null,
      workSpanId: work.spanContext().spanId,
      innerSpanId: inner.spanId,
      upstreamTraceparent: echo.headers.traceparent ?? null,
      proxyTrace: (await headers()).get("x-proxy-trace"),
    });
  });
}
