// Extends a built fixture's `.next/static/chunks` to the rc.2 churn soak's
// key space: 141 JavaScript and CSS files under .next/static whose gzip
// copies total about 3.59 MB (rc.2's churn project: its 32 MiB cache held
// 3,593,313 bytes for them; how that project was generated is not
// recorded). The added files are slices of the build's own chunks of equal
// gzip size, each with a unique leading comment, so they compress like real
// Next.js output.
//
//   node benchmarks/rc3/make-churn-project.mjs DIR [--files 141] [--gzip-bytes 3593313]
//
// DIR is a project whose `next build` already ran. NextRS serves every file
// under .next/static as immutable build output, so the compression cache
// sees these keys exactly as it sees Next's own chunks.
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";

const dir = process.argv[2];
const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? Number(process.argv[i + 1]) : fallback;
};
const wantFiles = arg("files", 141);
const wantGzip = arg("gzip-bytes", 3_593_313);
const staticDir = join(dir, ".next/static");
const chunksDir = join(staticDir, "chunks");
// The level of the server's gzip (flate2's default, as zlib's).
const gz = (buf) => gzipSync(buf, { level: 6 }).length;

const all = [];
const walk = (d) => {
  for (const f of readdirSync(d)) {
    const p = join(d, f);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(js|css)$/.test(f)) all.push({ p, size: statSync(p).size });
  }
};
walk(staticDir);
let files = all.length;
let identity = all.reduce((n, f) => n + f.size, 0);
let gzipped = all.reduce((n, f) => n + gz(readFileSync(f.p)), 0);
// The build's own JavaScript, as one text the added files take slices of
// (at a different offset each), so every file has the same gzip size and
// compresses like real chunks.
const source = all
  .filter((f) => f.p.startsWith(chunksDir) && f.p.endsWith(".js"))
  .sort((a, b) => a.p.localeCompare(b.p))
  .map((f) => readFileSync(f.p, "utf8"))
  .join("\n");
const added = wantFiles - files;
if (added <= 0) throw new Error(`already ${files} files`);
const step = 4096;
for (let i = 0; i < added; i++) {
  const target = (wantGzip - gzipped) / (added - i);
  let body = `/* nextrs churn chunk ${i} */\n`;
  let offset = Math.floor((i * source.length) / added);
  while (gz(Buffer.from(body)) < target) {
    let slice = source.slice(offset, offset + step);
    if (slice.length < step) slice += source.slice(0, step - slice.length);
    body += slice;
    offset = (offset + step) % source.length;
  }
  writeFileSync(join(chunksDir, `churn-${String(i).padStart(3, "0")}.js`), body);
  files += 1;
  identity += Buffer.byteLength(body);
  gzipped += gz(Buffer.from(body));
}
console.log(JSON.stringify({ static_files: files, identity_bytes: identity, gzip_bytes: gzipped, added }));
