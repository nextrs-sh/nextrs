// The k6 scenario benchmarks/phase2g_bench.mjs runs for one measurement:
// ITERATIONS requests shared by VUS virtual users against one URL, each
// response's status checked against EXPECT. The summary goes to SUMMARY.
import http from "k6/http";
import { check } from "k6";

const headers = JSON.parse(__ENV.HEADERS || "{}");
const expect = Number(__ENV.EXPECT || 200);
export const options = {
  scenarios: { bench: { executor: "shared-iterations", vus: Number(__ENV.VUS), iterations: Number(__ENV.ITERATIONS), maxDuration: "10m" } },
  summaryTrendStats: ["med", "p(95)", "p(99)"],
  // Redirects are measured, not followed.
  maxRedirects: 0,
  discardResponseBodies: true,
};

export default function () {
  const res = http.request(__ENV.METHOD || "GET", __ENV.URL, __ENV.BODY || null, { headers: { "accept-encoding": "identity", ...headers } });
  check(res, { status: (r) => r.status === expect });
}

export function handleSummary(data) {
  return { [__ENV.SUMMARY]: JSON.stringify(data) };
}
