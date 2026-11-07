import { trace } from "@opentelemetry/api";
import { headers } from "next/headers";

export const dynamic = "force-dynamic";

// A server-rendered page inside Next's render span, with its own span.
export default async function Traced() {
  const proxyTrace = (await headers()).get("x-proxy-trace");
  const traceId = await trace.getTracer("fixture").startActiveSpan("render-work", async (span) => {
    await new Promise((resolve) => setTimeout(resolve, 5));
    span.end();
    return span.spanContext().traceId;
  });
  return (
    <main>
      <h1 id="title">Traced page</h1>
      <p id="info">{JSON.stringify({ traceId, proxyTrace })}</p>
    </main>
  );
}
