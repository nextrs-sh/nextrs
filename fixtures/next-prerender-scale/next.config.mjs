// RE-5B: a build with many prerendered (non-ISR) App Router routes, to
// measure how NextRS's memory grows with the size of prerendered output.
// NEXTRS_PRERENDER_ROUTES sets the number of /p/[id] routes; build each
// size into its own directory with NEXTRS_PRERENDER_DIST (.next-1000, ...).
// The fixture may share next-app's node_modules through a symlink, so
// Turbopack resolves from the fixtures directory.
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export default {
  distDir: process.env.NEXTRS_PRERENDER_DIST ?? ".next",
  turbopack: { root: resolve(here, "..") },
};
