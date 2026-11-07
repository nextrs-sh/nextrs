import { trace } from "@opentelemetry/api";

export const dynamic = "force-dynamic";

// What this runtime instance registered.
export function GET() {
  const provider = trace.getTracerProvider() as { constructor?: { name?: string } };
  return Response.json({
    registrations: globalThis.__otelRegistrations ?? 0,
    provider: provider?.constructor?.name ?? null,
  });
}
