// A minimal OTLP/HTTP (JSON) trace collector for the OpenTelemetry
// fixture: it keeps the spans it receives in memory and answers queries,
// and can be told to fail (an unavailable backend). Not a telemetry backend.
//
//   node tests/compat/otel-collector.mjs --port 5610
//
//   POST /v1/traces          OTLP ExportTraceServiceRequest (JSON); 503 while failing
//   GET  /spans?traceId=T    [{traceId, spanId, parentSpanId, name, kind, service, scope, attributes}]
//   POST /admin/fail?on=1|0  answer exports with 503 (on) or accept them
//   GET  /stats              {exports, refused, spans, services}
import http from "node:http";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const port = Number(args.port ?? 5610);
const byTrace = new Map();
const stats = { exports: 0, refused: 0, spans: 0 };
let failing = false;

const value = (v) => v?.stringValue ?? (v?.intValue !== undefined ? Number(v.intValue) : v?.boolValue ?? v?.doubleValue ?? null);
const attributes = (list = []) => Object.fromEntries(list.map((a) => [a.key, value(a.value)]));

function ingest(body) {
  for (const resourceSpans of body.resourceSpans ?? []) {
    const resource = attributes(resourceSpans.resource?.attributes);
    for (const scopeSpans of resourceSpans.scopeSpans ?? []) {
      for (const span of scopeSpans.spans ?? []) {
        const record = {
          traceId: span.traceId,
          spanId: span.spanId,
          parentSpanId: span.parentSpanId || null,
          name: span.name,
          kind: span.kind,
          service: resource["service.name"] ?? null,
          scope: scopeSpans.scope?.name ?? null,
          attributes: attributes(span.attributes),
          status: span.status?.code ?? 0,
        };
        if (!byTrace.has(record.traceId)) byTrace.set(record.traceId, []);
        byTrace.get(record.traceId).push(record);
        stats.spans++;
      }
    }
  }
}

const send = (res, status, body) => {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
};

http
  .createServer(async (req, res) => {
    const url = new URL(req.url, "http://collector");
    if (url.pathname === "/v1/traces" && req.method === "POST") {
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      if (failing) {
        stats.refused++;
        return send(res, 503, { error: "unavailable" });
      }
      stats.exports++;
      try {
        ingest(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      } catch {
        return send(res, 400, { error: "not OTLP JSON" });
      }
      return send(res, 200, {});
    }
    if (url.pathname === "/spans") return send(res, 200, byTrace.get(url.searchParams.get("traceId") ?? "") ?? []);
    if (url.pathname === "/admin/fail" && req.method === "POST") {
      failing = url.searchParams.get("on") === "1";
      return send(res, 200, { failing });
    }
    if (url.pathname === "/stats") return send(res, 200, { ...stats, failing });
    return send(res, 404, {});
  })
  .listen(port, "127.0.0.1", () => console.log(`otel collector on ${port}`));
