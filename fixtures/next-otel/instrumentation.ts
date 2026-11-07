import { OTLPHttpJsonTraceExporter, registerOTel } from "@vercel/otel";

// Counts registrations, per runtime instance (an isolate in NextRS, the
// process in next start): /api/otel-state reports it.
declare global {
  // eslint-disable-next-line no-var
  var __otelRegistrations: number | undefined;
}

export function register() {
  globalThis.__otelRegistrations = (globalThis.__otelRegistrations ?? 0) + 1;
  registerOTel({
    serviceName: "nextrs-otel-fixture",
    traceExporter: new OTLPHttpJsonTraceExporter({ url: `${process.env.OTEL_COLLECTOR}/v1/traces` }),
    // Outgoing fetches carry the trace context to the upstream.
    instrumentationConfig: { fetch: { propagateContextUrls: [/127\.0\.0\.1/] } },
  });
}
