// Runs in the tools container (Node.js 24), after benchmarks/series1's
// discover.mjs wrote the static and RSC specs to the same directory.
// Adds the workloads Series 1 never measured:
//
// * ssr: the flight search page (`/flights?from&to&depart&adults&trip`),
//   rendered per request in JavaScript (it reads the search parameters and
//   streams results from the retailing API); each response must contain
//   flight numbers ("VEL "), so a page that streams only its loading shell
//   or an error counts as a failure (body checks need identity encoding:
//   the load generator does not decode gzip, so every `*-gzip.json` spec
//   checks status only and has an `*-identity.json` twin with the body
//   checks, run as a correctness pass);
// * api: route handlers: flight search (`/api/flights`, calls the API),
//   airports, destinations, fare families (cached API reads) and
//   `robots.txt`;
// * mixed: static assets, RSC navigations, prerendered HTML, SSR and route
//   handlers interleaved (weights below), the closest to a browsing mix.
//
//   node /re/discover-dynamic.mjs http://s1-sf:8080 /work/specs [DAYS_AHEAD]
//
// Departure dates are today + DAYS_AHEAD (default 14) and the six days
// after it: the API's clock is real time, so a fixed date would leave the
// sale window.
import { readFileSync, writeFileSync } from "node:fs";

const [base, dir, aheadArg] = process.argv.slice(2);
const ahead = Number(aheadArg ?? 14);

const routes = (await (await fetch(`${base}/api/routes`)).json()).data;
const day = (offset) => {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() + ahead + offset);
  return d.toISOString().slice(0, 10);
};
const gzip = [["accept-encoding", "gzip"]];
// Deterministic choice: routes leaving a hub, by id; for each, the first of
// seven days with flights on sale (schedules are weekly), until eight.
const hubRoutes = routes
  .filter((r) => r.origin === "MNL" || r.origin === "CEB")
  .sort((a, b) => a.id.localeCompare(b.id));
const pairs = [];
for (const r of hubRoutes) {
  for (let offset = 0; offset < 7 && pairs.length < 8; offset++) {
    const body = await (await fetch(`${base}/api/flights?from=${r.origin}&to=${r.destination}&departureDate=${day(offset)}&adults=1`)).text();
    if (body.includes('"itineraries":[{')) {
      pairs.push([r.origin, r.destination, day(offset)]);
      break;
    }
  }
  if (pairs.length === 8) break;
}
if (pairs.length < 4) throw new Error(`only ${pairs.length} hub routes with flights`);
const ssr = pairs.map(([from, to, date], i) => ({
  path: `/flights?from=${from}&to=${to}&depart=${date}&adults=${1 + (i % 2)}&trip=one-way`,
  headers: gzip, expect_body: ["VEL "], kind: "ssr",
}));
const api = [
  ...pairs.slice(0, 4).map(([from, to, date]) => ({
    path: `/api/flights?from=${from}&to=${to}&departureDate=${date}&adults=1`,
    headers: gzip, expect_body: ['"itineraries"'], kind: "api-search",
  })),
  { path: "/api/airports", headers: gzip, expect_body: ['"code"'], kind: "api-read" },
  { path: "/api/destinations", headers: gzip, expect_body: ['"slug"'], kind: "api-read" },
  { path: "/api/fare-families", headers: gzip, expect_body: ['"code"'], kind: "api-read" },
  { path: "/robots.txt", headers: gzip, expect_body: ["User-Agent"], kind: "api-read" },
];

// Check each request once before writing the specs.
for (const r of [...ssr, ...api]) {
  const res = await fetch(base + r.path);
  const body = await res.text();
  const missing = (r.expect_body ?? []).filter((s) => !body.includes(s));
  if (res.status !== 200 || missing.length) throw new Error(`${r.path}: ${res.status}, missing ${missing}`);
}

const spec = (requests) => JSON.stringify({ requests }, null, 1);
const identity = (reqs) => reqs.map(({ headers, ...r }) => r);
const gzipped = (reqs) => reqs.map(({ expect_body, ...r }) => r);
writeFileSync(`${dir}/ssr-identity.json`, spec(identity(ssr)));
writeFileSync(`${dir}/api-identity.json`, spec(identity(api)));
writeFileSync(`${dir}/ssr-gzip.json`, spec(gzipped(ssr)));
writeFileSync(`${dir}/api-gzip.json`, spec(gzipped(api)));

// Mixed: per 20 requests, 8 static assets, 5 RSC, 2 prerendered HTML
// pages, 2 SSR searches, 3 route handlers (interleaved, deterministic).
const load = (name) => JSON.parse(readFileSync(`${dir}/${name}`, "utf8")).requests;
const statics = load("static-gzip.json");
const rsc = load("rsc-gzip.json");
const html = ["/", "/destinations", "/about", "/help"].map((path) => ({ path, headers: gzip, kind: "html" }));
const pattern = ["static", "rsc", "static", "api", "static", "rsc", "ssr", "static", "rsc", "html",
  "static", "api", "static", "rsc", "static", "ssr", "rsc", "static", "api", "html"];
const pools = { static: statics, rsc, html, ssr: gzipped(ssr), api: gzipped(api) };
const cursor = { static: 0, rsc: 0, html: 0, ssr: 0, api: 0 };
const mixed = [];
// Enough cycles that every request of every pool appears.
const cycles = Math.max(...Object.entries(pools).map(([k, v]) => Math.ceil(v.length / pattern.filter((p) => p === k).length)));
for (let c = 0; c < cycles; c++) {
  for (const kind of pattern) {
    const pool = pools[kind];
    mixed.push({ ...pool[cursor[kind]++ % pool.length], kind });
  }
}
writeFileSync(`${dir}/mixed-gzip.json`, spec(mixed));
console.log(JSON.stringify({ pairs: pairs.map((p) => p.join(" ")), ssr: ssr.length, api: api.length, mixed: mixed.length }));
