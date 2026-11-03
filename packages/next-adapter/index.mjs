// NextRS deployment adapter (Phase 0).
//
// Enable without touching the application:
//   NEXT_ADAPTER_PATH=/abs/path/to/packages/next-adapter/index.mjs next build
//
// Writes <distDir>/nextrs/build-output.json: the adapter API's view of the
// build, with absolute paths made relative to the project directory so the
// Rust runtime can consume it from any location.
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const SCHEMA_VERSION = 1;

// Only config keys that affect request routing or asset serving. The full
// config may contain `env` values and other secrets.
const CONFIG_KEYS = [
  "basePath",
  "assetPrefix",
  "trailingSlash",
  "i18n",
  "output",
  "skipTrailingSlashRedirect",
  "skipMiddlewareUrlNormalize",
  "poweredByHeader",
  "compress",
  "generateEtags",
  "cacheComponents",
];

// Adapter outputs carry absolute filesystem paths in `filePath`, the values
// of `assets`/`wasmAssets`, and `fallback.filePath`. URL pathnames also start
// with "/", so only these known fields are rewritten.
function relativizeOutput(projectDir, output) {
  const rel = (file) => (file ? path.relative(projectDir, file) : file);
  const mapValues = (record) =>
    record && Object.fromEntries(Object.entries(record).map(([key, file]) => [key, rel(file)]));
  const result = { ...output, filePath: rel(output.filePath) };
  if (output.assets) result.assets = mapValues(output.assets);
  if (output.wasmAssets) result.wasmAssets = mapValues(output.wasmAssets);
  if (output.fallback) result.fallback = { ...output.fallback, filePath: rel(output.fallback.filePath) };
  return result;
}

function relativizeOutputs(projectDir, outputs) {
  return Object.fromEntries(
    Object.entries(outputs).map(([kind, value]) => [
      kind,
      Array.isArray(value)
        ? value.map((output) => relativizeOutput(projectDir, output))
        : value && relativizeOutput(projectDir, value),
    ]),
  );
}

/** @type {import('next').NextAdapter} */
const adapter = {
  name: "nextrs",
  async onBuildComplete(ctx) {
    const config = Object.fromEntries(
      CONFIG_KEYS.filter((key) => key in ctx.config).map((key) => [key, ctx.config[key]]),
    );
    const output = {
      schemaVersion: SCHEMA_VERSION,
      adapter: "nextrs",
      nextVersion: ctx.nextVersion,
      buildId: ctx.buildId,
      distDir: path.relative(ctx.projectDir, ctx.distDir),
      config,
      routing: ctx.routing,
      outputs: relativizeOutputs(ctx.projectDir, ctx.outputs),
    };
    const outDir = path.join(ctx.distDir, "nextrs");
    await mkdir(outDir, { recursive: true });
    await writeFile(path.join(outDir, "build-output.json"), JSON.stringify(output, null, 2));
  },
};

export default adapter;
