#!/usr/bin/env node
// Writes the third-party notices of the nextrs binary, for the release
// archive:
//
//   cargo metadata --format-version 1 --filter-platform x86_64-unknown-linux-gnu --locked |
//     node scripts/third-party-licenses.mjs --table THIRD-PARTY.md --texts THIRD-PARTY-LICENSES.txt
//
// The crates are the normal (not build or dev) dependencies of nextrs-cli
// for the target, from Cargo.lock. For each crate THIRD-PARTY-LICENSES.txt
// holds the license and notice files of its published source (LICENSE*,
// LICENCE*, COPYING*, NOTICE*, COPYRIGHT*; for *-sys crates also those of
// the C sources they bundle). A crate that ships no license file gets the
// SPDX text of the license it declares (release/licenses/spdx/), with its
// authors as the copyright holders. Identical texts are printed once.
//
// The V8 engine and the C/C++ libraries the `v8` crate compiles into its
// static library are not crates: their license files, fetched at the
// revisions rusty_v8 v150.4.0 pins, are in release/licenses/native/
// (release/licenses/SOURCES.tsv says where each came from).
//
// Fails if a crate has neither a license file nor a declared license with
// an SPDX text available, so a dependency update cannot drop a notice.
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, a, i, all) => (a.startsWith("--") ? [...pairs, [a.slice(2), all[i + 1]]] : pairs), []),
);
if (!args.table || !args.texts) {
  console.error("usage: cargo metadata ... | third-party-licenses.mjs --table FILE --texts FILE");
  process.exit(2);
}
const metadata = JSON.parse(readFileSync(0, "utf8"));
const NOTICE = /^(licen[cs]e|copying|notice|copyright|unlicense)([-._].*)?$/i;
const SPDX_DIR = join(ROOT, "release/licenses/spdx");
const NATIVE_DIR = join(ROOT, "release/licenses/native");

// The binary's crate graph.
const byId = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
const ids = new Set();
const root = metadata.packages.find((p) => p.name === "nextrs-cli").id;
const walk = (id) => {
  if (ids.has(id)) return;
  ids.add(id);
  for (const d of byId.get(id).deps) if (d.dep_kinds.some((k) => k.kind === null)) walk(d.pkg);
};
walk(root);
const crates = metadata.packages
  .filter((p) => ids.has(p.id) && !p.name.startsWith("nextrs-"))
  .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version, undefined, { numeric: true }));

function noticeFiles(dir, depth) {
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isFile() && NOTICE.test(e.name) && !/\.(rs|toml|json|html?|py|sh|c|h)$/i.test(e.name)) out.push(p);
    else if (e.isDirectory() && depth > 0 && !/^(tests?|examples?|benches|\.git|target|fuzz|doc|docs)$/i.test(e.name)) out.push(...noticeFiles(p, depth - 1));
  }
  return out.sort();
}

// The SPDX identifier to print for a crate without license files: the
// first alternative of an OR expression; every operand of an AND.
function spdxFor(expression) {
  const clean = expression.replace(/\//g, " OR ").replace(/[()]/g, " ");
  const first = clean.split(/\s+OR\s+/)[0];
  return first.split(/\s+AND\s+/).map((s) => s.trim()).filter(Boolean);
}

const printed = new Map(); // sha256 -> label of the first copy
const sections = [];
const rows = [];
let missing = [];
for (const c of crates) {
  const dir = dirname(c.manifest_path);
  const files = noticeFiles(dir, c.name.endsWith("-sys") ? 4 : 0);
  const label = `${c.name} ${c.version}`;
  const body = [];
  let source;
  if (files.length) {
    source = files.map((f) => relative(dir, f)).join(", ");
    for (const f of files) {
      const text = readFileSync(f, "utf8").replace(/\r\n/g, "\n").trimEnd();
      const sha = createHash("sha256").update(text).digest("hex");
      const name = relative(dir, f);
      if (printed.has(sha)) body.push(`--- ${name}: the same text as ${printed.get(sha)}, above.`);
      else {
        printed.set(sha, `${label} ${name}`);
        body.push(`--- ${name}\n\n${text}`);
      }
    }
  } else {
    const license = c.license ?? "";
    const holders = c.authors?.length ? c.authors.join(", ") : `the ${c.name} authors`;
    const ids = spdxFor(license);
    const texts = ids.map((id) => [id, join(SPDX_DIR, `${id}.txt`)]);
    const absent = texts.filter(([, p]) => !existsSync(p)).map(([id]) => id);
    if (!license || absent.length) {
      missing.push(`${label} (${license || "no license declared"}${absent.length ? `; no SPDX text for ${absent.join(", ")}` : ""})`);
      continue;
    }
    source = `no license file in the crate; SPDX text of ${ids.join(" AND ")}`;
    body.push(`The published crate has no license file. It declares "${license}"; the SPDX text of ${ids.join(" and ")} follows, and its copyright holders are: ${holders}.`);
    for (const [id, p] of texts) {
      const text = readFileSync(p, "utf8").trimEnd();
      const sha = createHash("sha256").update(text).digest("hex");
      if (printed.has(sha)) body.push(`--- ${id}: the same text as ${printed.get(sha)}, above.`);
      else {
        printed.set(sha, `the SPDX ${id} text`);
        body.push(`--- SPDX ${id}\n\n${text}`);
      }
    }
  }
  if (/\bMPL-2\.0\b/.test(c.license ?? "")) {
    body.push(`Source Code Form (MPL-2.0 section 3.2): https://crates.io/crates/${c.name}/${c.version} (the published crate)${c.repository ? `, ${c.repository}` : ""}.`);
  }
  rows.push(`| ${c.name} | ${c.version} | ${c.license ?? "unspecified"} | ${source} |`);
  sections.push(`==============================================================================\n${label} (${c.license ?? "unspecified"})\nhttps://crates.io/crates/${c.name}/${c.version}\n\n${body.join("\n\n")}\n`);
}
if (missing.length) {
  console.error(`no license text for:\n  ${missing.join("\n  ")}`);
  process.exit(1);
}

// V8 and its bundled C/C++ libraries.
const sources = readFileSync(join(ROOT, "release/licenses/SOURCES.tsv"), "utf8")
  .trim()
  .split("\n")
  .map((l) => l.split("\t"))
  .filter(([c]) => c !== "spdx");
const native = [];
const nativeRows = [];
for (const component of [...new Set(sources.map(([c]) => c))]) {
  const parts = [];
  for (const [, repo, rev, file] of sources.filter(([c]) => c === component)) {
    const text = readFileSync(join(NATIVE_DIR, component, file), "utf8").replace(/\r\n/g, "\n").trimEnd();
    parts.push(`--- ${file} (${repo} at ${rev})\n\n${text}`);
  }
  const [, repo, rev] = sources.find(([c]) => c === component);
  nativeRows.push(`| ${component} | ${repo} | \`${rev.slice(0, 12)}\` |`);
  native.push(`==============================================================================\n${component} (compiled into the v8 crate's static library)\n\n${parts.join("\n\n")}\n`);
}

const header = `Third-party notices for the nextrs binary

The nextrs binary statically links the Rust crates listed below (the
normal dependencies of nextrs-cli for x86_64-unknown-linux-gnu, from
Cargo.lock) and, through the v8 crate (rusty_v8 150.4.0), the V8 JavaScript
engine and the C/C++ libraries listed at the end. The JavaScript of deno_web
0.291.0 and deno_webidl 0.260.0 is compiled into the binary as text; it is
covered by those crates' entries (MIT, Copyright 2018-2026 the Deno
authors). At run time it links only the system C library (glibc: libc,
libm, ld-linux) and libgcc_s.

`;
writeFileSync(args.texts, `${header}${sections.join("\n")}\n${native.join("\n")}`);
writeFileSync(
  args.table,
  `# Third-party components

The nextrs binary statically links the Rust crates below (normal dependencies of nextrs-cli for x86_64-unknown-linux-gnu, from Cargo.lock) and, through the \`v8\` crate, Google V8 and the C/C++ libraries in the second table. The license and notice texts are in THIRD-PARTY-LICENSES.txt, next to this file.

| Crate | Version | License | License text |
|---|---|---|---|
${rows.join("\n")}

## Compiled into the v8 crate

| Component | Source | Revision |
|---|---|---|
${nativeRows.join("\n")}
`,
);
console.error(`${crates.length} crates, ${nativeRows.length} native components, ${printed.size} distinct license texts`);
