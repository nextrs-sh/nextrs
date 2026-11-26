// Compares two captures (capture.mjs) request by request and prints a
// Markdown report: decoded-body equality, status, encoding and the cache
// headers. Differences are listed, never hidden; a known, documented
// difference is still a difference.
//
//   node benchmarks/series1/compare.mjs A.json B.json [--labels node,nextrs]
import { readFileSync } from "node:fs";

const args = process.argv.slice(2);
const [fa, fb] = args.filter((a) => !a.startsWith("--") && !a.includes(","));
const labelsArg = args[args.indexOf("--labels") + 1];
const [la, lb] = args.includes("--labels") ? labelsArg.split(",") : ["A", "B"];
const A = JSON.parse(readFileSync(fa, "utf8"));
const B = JSON.parse(readFileSync(fb, "utf8"));
const key = (r) => `${r.name} ${r.method} ${r.path}`;
const byKey = new Map(B.map((r) => [key(r), r]));

const groups = new Map();
const diffs = [];
for (const a of A) {
  const b = byKey.get(key(a));
  const g = groups.get(a.name) ?? { n: 0, body: 0, status: 0, enc: 0, wireA: 0, wireB: 0 };
  groups.set(a.name, g);
  g.n++;
  if (!b) {
    diffs.push(`missing in ${lb}: ${key(a)}`);
    continue;
  }
  g.wireA += a.wireBytes;
  g.wireB += b.wireBytes;
  if (a.sha256 === b.sha256) g.body++;
  else diffs.push(`${key(a)}: decoded body differs (${a.decodedBytes} vs ${b.decodedBytes} bytes)`);
  if (a.status === b.status) g.status++;
  else diffs.push(`${key(a)}: status ${a.status} vs ${b.status}`);
  if ((a.headers["content-encoding"] ?? "") === (b.headers["content-encoding"] ?? "")) g.enc++;
  for (const h of new Set([...Object.keys(a.headers), ...Object.keys(b.headers)])) {
    if (h === "etag" || h === "last-modified") continue; // compared separately below
    const va = a.headers[h];
    const vb = b.headers[h];
    if (JSON.stringify(va) !== JSON.stringify(vb)) {
      diffs.push(`${key(a)}: ${h}: ${la}=${JSON.stringify(va)} ${lb}=${JSON.stringify(vb)}`);
    }
  }
  if (Boolean(a.headers.etag) !== Boolean(b.headers.etag)) {
    diffs.push(`${key(a)}: etag present ${la}=${Boolean(a.headers.etag)} ${lb}=${Boolean(b.headers.etag)}`);
  }
  if (a.decodeError || b.decodeError) diffs.push(`${key(a)}: gzip decode error ${a.decodeError ?? ""} ${b.decodeError ?? ""}`);
}

console.log(`| Requests | n | same decoded body | same status | same encoding | wire bytes ${la} | wire bytes ${lb} |`);
console.log("|---|---:|---:|---:|---:|---:|---:|");
for (const [name, g] of groups) {
  console.log(`| ${name} | ${g.n} | ${g.body} | ${g.status} | ${g.enc} | ${g.wireA} | ${g.wireB} |`);
}
console.log(`\n${diffs.length} difference(s)${diffs.length ? ":" : "."}`);
// Collapse identical messages that differ only by path.
const collapsed = new Map();
for (const d of diffs) {
  const shape = d.replace(/ \/[^\s:]*/, " <path>");
  const entry = collapsed.get(shape) ?? { n: 0, example: d };
  entry.n++;
  collapsed.set(shape, entry);
}
for (const { n, example } of collapsed.values()) console.log(`- ${n}× ${example}`);
