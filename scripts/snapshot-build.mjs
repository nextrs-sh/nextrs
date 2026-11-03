// Copies the metadata NextRS reads from a `.next` directory into a test
// snapshot, without server JS bundles, trace files or caches.
//
//   node scripts/snapshot-build.mjs <dist-dir> <out-dir>
//
// Build-time secrets (preview-mode keys, Server Actions encryption key) are
// replaced with placeholders.
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

const [distDir, outDir] = process.argv.slice(2).map((p) => path.resolve(p));
if (!distDir || !outDir) throw new Error("usage: snapshot-build.mjs <dist-dir> <out-dir>");

const FILES = [
  "BUILD_ID",
  "routes-manifest.json",
  "prerender-manifest.json",
  "app-path-routes-manifest.json",
  "build-manifest.json",
  "required-server-files.json",
  "diagnostics/framework.json",
  "server/app-paths-manifest.json",
  "server/pages-manifest.json",
  "server/middleware-manifest.json",
  "server/functions-config-manifest.json",
  "server/server-reference-manifest.json",
];
const DIRS = ["static"];

rmSync(outDir, { recursive: true, force: true });
const copy = (rel) => {
  const from = path.join(distDir, rel);
  if (!existsSync(from)) return;
  mkdirSync(path.dirname(path.join(outDir, rel)), { recursive: true });
  cpSync(from, path.join(outDir, rel), { recursive: true });
};
FILES.forEach(copy);
DIRS.forEach(copy);
// With a deployment adapter, prerenders live in server/route-cache. Without
// one, `next start` also writes runtime cache entries there, so only copy it
// for adapter builds to keep default snapshots identical to a fresh build.
if (existsSync(path.join(distDir, "nextrs/build-output.json"))) copy("server/route-cache");
// Prerendered responses in the default layout: server/app/**/*.{html,rsc,meta}
cpSync(path.join(distDir, "server/app"), path.join(outDir, "server/app"), {
  recursive: true,
  filter: (src) => !/\.(js|map|json)$/.test(src),
});
// Also keep pages/*.html (default 404/500 shells).
cpSync(path.join(distDir, "server/pages"), path.join(outDir, "server/pages"), { recursive: true });

const rewriteJson = (rel, edit) => {
  const file = path.join(outDir, rel);
  if (!existsSync(file)) return;
  const json = JSON.parse(readFileSync(file, "utf8"));
  edit(json);
  writeFileSync(file, JSON.stringify(json, null, 2));
};
rewriteJson("prerender-manifest.json", (m) => {
  m.preview = { previewModeId: "redacted", previewModeSigningKey: "redacted", previewModeEncryptionKey: "redacted" };
});
rewriteJson("server/server-reference-manifest.json", (m) => {
  m.encryptionKey = "redacted";
});
rewriteJson("required-server-files.json", (m) => {
  m.files = [];
});
const adapterFile = path.join(distDir, "nextrs/build-output.json");
if (existsSync(adapterFile)) {
  copy("nextrs/build-output.json");
  rewriteJson("nextrs/build-output.json", (o) => {
    for (const list of Object.values(o.outputs)) {
      for (const output of Array.isArray(list) ? list : list ? [list] : []) {
        // Traced node_modules lists are large and irrelevant to the reader.
        if (output.assets) output.assets = {};
        if (output.assetsHashes) output.assetsHashes = {};
        if (output.config?.bypassToken) output.config.bypassToken = "redacted";
      }
    }
  });
}
console.log(`snapshot written to ${outDir}`);
