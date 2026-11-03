// Semantic comparison of dynamic responses, for runtimes whose output is
// equivalent to `next start` but not byte-identical (different stream flush
// boundaries split the inline Flight data into different <script> chunks,
// and Flight row ids follow resolution order).
//
//   node tests/compat/semantic.mjs --baseline http://127.0.0.1:3000 \
//        --candidate http://127.0.0.1:3200 [--json out.json]
//
// For each response it compares:
//   * status and content-type;
//   * the HTML with every inline `self.__next_f.push(...)` script removed
//     (exact string equality: this is the server-rendered DOM);
//   * the Flight data: rows parsed from the concatenated payloads, every
//     reference resolved from the root row, and the resulting tree compared
//     as JSON. Import ("I") and hint ("H*") rows are compared as sets.
// Only per-request random keys (22-character ids that differ between two
// `next start` runs) are masked. Nothing else is normalized away.
//
// This parses Flight for testing only; NextRS never parses it at runtime.
import http from "node:http";
import { writeFileSync } from "node:fs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const baseline = new URL(args.baseline ?? "http://127.0.0.1:3000");
const candidate = new URL(args.candidate ?? "http://127.0.0.1:3001");

const cases = [
  { path: "/dynamic" },
  { path: "/books/42" },
  { path: "/books/%E2%9C%93" },
  { path: "/api/health" },
  { path: "/request-info", headers: { "x-nextrs-probe": "hello", cookie: "theme=dark" } },
  { path: "/request-info" },
  { path: "/books/42?_rsc", headers: { rsc: "1" }, flightOnly: true },
];

function request(base, { path, headers = {} }) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: base.hostname, port: base.port, path, headers, agent: false }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks).toString() }));
    });
    req.on("error", reject);
    req.end();
  });
}

const PUSH = /<script>self\.__next_f\.push\((\[[\s\S]*?\])\)<\/script>/g;

function splitHtml(html) {
  let flight = "";
  for (const match of html.matchAll(PUSH)) {
    const [kind, data] = JSON.parse(match[1]);
    if (kind === 1) flight += data;
  }
  return { dom: html.replace(PUSH, ""), flight };
}

// Flight rows: `<hex id>:<payload>\n`, where the payload is JSON, a tagged
// JSON value (`I[...]`, `HL[...]`, ...), or a length-prefixed text row
// (`T<hex length>,<text>`).
function parseRows(text) {
  const rows = new Map();
  let i = 0;
  while (i < text.length) {
    const colon = text.indexOf(":", i);
    if (colon === -1) break;
    const id = text.slice(i, colon);
    let j = colon + 1;
    let tag = "";
    while (j < text.length && /[A-Z]/.test(text[j])) tag += text[j++];
    if (tag === "T") {
      const comma = text.indexOf(",", j);
      const length = parseInt(text.slice(j, comma), 16);
      const bytes = Buffer.from(text.slice(comma + 1), "utf8").subarray(0, length).toString("utf8");
      append(rows, id, { tag, value: bytes });
      i = comma + 1 + bytes.length;
      continue;
    }
    const end = text.indexOf("\n", j);
    const raw = text.slice(j, end === -1 ? text.length : end);
    let value = raw;
    try {
      value = raw === "" ? null : JSON.parse(raw);
    } catch {
      // Keep unparseable payloads verbatim; they still have to be equal.
    }
    append(rows, id, { tag, value });
    i = end === -1 ? text.length : end + 1;
  }
  return rows;
}

// Stream rows reuse their id (`6:X` ... `6:C`); keep every value in order.
function append(rows, id, row) {
  if (!rows.has(id)) rows.set(id, []);
  rows.get(id).push(row);
}

const RANDOM_KEY = /^[A-Za-z0-9_-]{22}$/;

function canonical(rows) {
  const resolving = new Set();
  const resolveOne = (row) => {
    if (row.tag === "I") return { import: row.value };
    const out = walk(row.value);
    return row.tag ? { [row.tag]: out } : out;
  };
  const resolveRef = (id, kind = "") => {
    if (resolving.has(id)) return `<cycle ${id}>`;
    const list = rows.get(id);
    if (!list) return `<missing ${id}>`;
    resolving.add(id);
    const values = list.map(resolveOne);
    resolving.delete(id);
    const out = values.length === 1 ? values[0] : values;
    return kind ? { [`$${kind}`]: out } : out;
  };
  const walk = (value) => {
    if (typeof value === "string") {
      // Row references: plain, lazy (L), promise (@), Map (Q), Set (W),
      // FormData (K), Blob (B), server reference (F). Not $n (BigInt), $D
      // (Date) or $S (symbol), whose payload is a literal.
      const ref = /^\$([L@QWKBF]?)([0-9a-f]+)$/.exec(value);
      if (ref) return resolveRef(ref[2], ref[1] === "L" || ref[1] === "@" ? "" : ref[1]);
      return RANDOM_KEY.test(value) ? "<per-request-key>" : value;
    }
    if (Array.isArray(value)) return value.map(walk);
    if (value && typeof value === "object") {
      return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, walk(v)]));
    }
    return value;
  };
  const sideTables = {};
  for (const row of [...rows.values()].flat()) {
    if (row.tag === "I" || row.tag.startsWith("H")) {
      (sideTables[row.tag] ??= new Set()).add(JSON.stringify(row.value));
    }
  }
  return {
    tree: JSON.stringify(resolveRef("0")),
    side: Object.fromEntries(Object.entries(sideTables).map(([k, v]) => [k, [...v].sort()])),
  };
}

function compareFlight(a, b) {
  const ca = canonical(parseRows(a));
  const cb = canonical(parseRows(b));
  const problems = [];
  if (ca.tree !== cb.tree) {
    const at = [...ca.tree].findIndex((ch, k) => ch !== cb.tree[k]);
    problems.push(`flight tree differs at ${at}: ${ca.tree.slice(Math.max(0, at - 60), at + 60)} | ${cb.tree.slice(Math.max(0, at - 60), at + 60)}`);
  }
  if (JSON.stringify(ca.side) !== JSON.stringify(cb.side)) problems.push("flight import/hint rows differ");
  return problems;
}

const results = [];
for (const testCase of cases) {
  const [a, b] = await Promise.all([request(baseline, testCase), request(candidate, testCase)]);
  const problems = [];
  if (a.status !== b.status) problems.push(`status ${a.status} vs ${b.status}`);
  if (a.headers["content-type"] !== b.headers["content-type"]) {
    problems.push(`content-type ${a.headers["content-type"]} vs ${b.headers["content-type"]}`);
  }
  const html = String(a.headers["content-type"]).startsWith("text/html");
  if (testCase.flightOnly) {
    problems.push(...compareFlight(a.body, b.body));
  } else if (html) {
    const sa = splitHtml(a.body);
    const sb = splitHtml(b.body);
    if (sa.dom !== sb.dom) problems.push("server-rendered HTML (without Flight scripts) differs");
    problems.push(...compareFlight(sa.flight, sb.flight));
  } else if (a.body !== b.body) {
    problems.push("body differs");
  }
  results.push({
    request: testCase.path,
    verdict: problems.length ? "DIFF" : a.body === b.body ? "IDENTICAL" : "EQUIVALENT",
    bytes: [a.body.length, b.body.length],
    problems,
  });
}

for (const r of results) {
  console.log(`${r.verdict.padEnd(11)} ${r.request.padEnd(24)} bytes=${r.bytes.join("/")}`);
  for (const p of r.problems) console.log(`            ${p}`);
}
if (args.json) writeFileSync(args.json, JSON.stringify({ baseline: baseline.href, candidate: candidate.href, results }, null, 2));
process.exitCode = results.some((r) => r.verdict === "DIFF") ? 1 : 0;
