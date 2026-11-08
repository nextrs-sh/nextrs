// Writes the fixture's images (deterministic: same bytes on every run with
// the same sharp) to public/photos, app/assets (static imports) and the
// route handler's data module. Each has properties the suites assert:
// dimensions, transparency, orientation, colour profile, animation.
import { mkdirSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const photos = join(root, "public/photos");
const assets = join(root, "app/assets");
mkdirSync(photos, { recursive: true });
mkdirSync(assets, { recursive: true });

// xorshift32: reproducible noise.
function rng(seed) {
  let x = seed >>> 0 || 1;
  return () => {
    x ^= x << 13; x >>>= 0; x ^= x >>> 17; x ^= x << 5; x >>>= 0;
    return x / 0x100000000;
  };
}

// A photo-like picture: smooth gradients, discs and fine noise, with four
// quadrant colours the suites sample (top-left red, top-right green,
// bottom-left blue, bottom-right white).
function picture(width, height, { alpha = false, seed = 7 } = {}) {
  const channels = alpha ? 4 : 3;
  const data = Buffer.alloc(width * height * channels);
  const rand = rng(seed);
  const quad = [[220, 40, 40], [40, 200, 60], [40, 60, 220], [235, 235, 235]];
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const q = (y < height / 2 ? 0 : 2) + (x < width / 2 ? 0 : 1);
      const i = (y * width + x) * channels;
      const n = (rand() - 0.5) * 24;
      const shade = Math.sin(x / 37) * 12 + Math.cos(y / 53) * 12;
      for (let c = 0; c < 3; c++) data[i + c] = Math.max(0, Math.min(255, quad[q][c] + shade + n));
      if (alpha) {
        const dx = x - width / 2, dy = y - height / 2;
        const r = Math.sqrt(dx * dx + dy * dy) / (Math.min(width, height) / 2);
        // Opaque centre, fully transparent corners.
        data[i + 3] = r < 0.6 ? 255 : r > 0.95 ? 0 : Math.round(255 * (0.95 - r) / 0.35);
      }
    }
  }
  return sharp(data, { raw: { width, height, channels } });
}

const out = (name) => join(photos, name);
async function make(name, build) {
  const path = out(name);
  if (existsSync(path) && !process.env.FORCE) return;
  await build(path);
}

await make("photo.jpg", (p) => picture(1600, 1200).jpeg({ quality: 90 }).toFile(p));
await make("odd.jpg", (p) => picture(1001, 667, { seed: 3 }).jpeg({ quality: 90 }).toFile(p));
await make("small.png", (p) => picture(48, 32, { seed: 5 }).png().toFile(p));
await make("alpha.png", (p) => picture(800, 600, { alpha: true, seed: 9 }).png().toFile(p));
await make("photo.webp", (p) => picture(1200, 800, { seed: 11 }).webp({ quality: 90 }).toFile(p));
await make("alpha.webp", (p) => picture(640, 480, { alpha: true, seed: 12 }).webp({ quality: 90 }).toFile(p));
await make("photo.avif", (p) => picture(800, 600, { seed: 13 }).avif({ quality: 60 }).toFile(p));
await make("still.gif", (p) => picture(300, 200, { seed: 15 }).gif().toFile(p));
// sharp's default TIFF is JPEG-compressed YCbCr; the LZW one is RGB.
await make("photo.tiff", (p) => picture(640, 480, { seed: 16 }).tiff().toFile(p));
await make("lzw.tiff", (p) => picture(640, 480, { seed: 18 }).tiff({ compression: "lzw" }).toFile(p));
// Stored 600x400 with EXIF orientation 6: displayed (and optimized) as
// 400x600, red quadrant at the top right.
await make("rotated.jpg", (p) => picture(600, 400, { seed: 17 }).withMetadata({ orientation: 6 }).jpeg({ quality: 90 }).toFile(p));
// Pixels in Display P3 with the profile embedded: output must be sRGB.
await make("p3.jpg", (p) => picture(640, 480, { seed: 19 }).withIccProfile("p3").jpeg({ quality: 92 }).toFile(p));
await make("animated.gif", async (p) => {
  const a = await picture(120, 80, { seed: 21 }).png().toBuffer();
  const b = await picture(120, 80, { seed: 22 }).negate().png().toBuffer();
  await sharp([a, b], { join: { animated: true } }).gif({ delay: [200, 200] }).toFile(p);
});
await make("animated.webp", async (p) => {
  const a = await picture(120, 80, { seed: 23 }).png().toBuffer();
  const b = await picture(120, 80, { seed: 24 }).negate().png().toBuffer();
  await sharp([a, b], { join: { animated: true } }).webp({ delay: [200, 200] }).toFile(p);
});
// A large photo (24 MP) and a decompression bomb: 16,500 x 16,500 of one
// colour, above next start's imgOptMaxInputPixels (268,402,689), a few
// hundred KB compressed.
await make("large.jpg", (p) => picture(6000, 4000, { seed: 25 }).jpeg({ quality: 85 }).toFile(p));
// The same size, progressive: its decoder keeps every DCT coefficient
// (memory measurements, Phase 2E).
await make("large-progressive.jpg", (p) => picture(6000, 4000, { seed: 26 }).jpeg({ quality: 85, progressive: true }).toFile(p));
await make("bomb.png", (p) =>
  sharp({ create: { width: 16500, height: 16500, channels: 3, background: "#808080" }, limitInputPixels: false })
    .toColourspace("b-w").png({ compressionLevel: 9 }).toFile(p));
await make("icon.svg", (p) =>
  writeFileSync(p, '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="#2a6"/></svg>'));
// JPEG magic number, then garbage: sniffed as JPEG, cannot be decoded.
await make("corrupt.jpg", (p) => writeFileSync(p, Buffer.concat([Buffer.from([0xff, 0xd8, 0xff, 0xe0]), Buffer.alloc(2048, 0x41)])));
await make("truncated.png", async (p) => {
  const full = await picture(400, 300, { seed: 27 }).png().toBuffer();
  writeFileSync(p, full.subarray(0, Math.floor(full.length / 2)));
});
await make("not-an-image.png", (p) => writeFileSync(p, "this is text, not a picture\n"));
await make("document.pdf", (p) => writeFileSync(p, "%PDF-1.4\n%nextrs fixture\n"));

// Static imports (next build reads their size and blur placeholder).
const still = join(assets, "still.jpg");
if (!existsSync(still) || process.env.FORCE) await picture(1200, 900, { seed: 31 }).jpeg({ quality: 90 }).toFile(still);
const wide = join(assets, "wide.png");
if (!existsSync(wide) || process.env.FORCE) await picture(1920, 640, { seed: 33 }).png().toFile(wide);

// A route handler that answers with image bytes (a dynamic source).
const data = join(root, "app/api/image/[name]/data.ts");
mkdirSync(dirname(data), { recursive: true });
const dyn = await picture(320, 240, { seed: 35 }).png().toBuffer();
writeFileSync(data, `// Generated by scripts/make-images.mjs.\nexport const png = ${JSON.stringify(dyn.toString("base64"))};\n`);
console.log("fixture images ready");
