// Deterministic codec corpus for the Phase 2E image measurements
// (benchmarks/codec-matrix.sh): photos from the host's wallpapers, plus
// generated graphics, text, gradients, noise and alpha. Uses the sharp that
// fixtures/next-image installs.
//
//   node benchmarks/image-corpus.mjs OUT_DIR
import { createRequire } from "node:module";
import { existsSync, mkdirSync, statSync } from "node:fs";
import { join } from "node:path";

const require = createRequire(new URL("../fixtures/next-image/package.json", import.meta.url));
const sharp = require("sharp");
const out = process.argv[2];
mkdirSync(out, { recursive: true });
const wall = (name) => `/usr/share/backgrounds/fedora-workstation/${name}.webp`;
const make = async (name, fn) => {
  const path = join(out, name);
  if (!existsSync(path)) await fn(path);
  const m = await sharp(path).metadata();
  console.log(name, m.width, m.height, m.channels, m.isProgressive ? "progressive" : "", statSync(path).size);
};

function rng(seed) {
  let s = seed >>> 0;
  return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 2 ** 32);
}
const raw = (w, h, c, f) => {
  const buf = Buffer.alloc(w * h * c);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) f(x, y, buf, (y * w + x) * c);
  return sharp(buf, { raw: { width: w, height: h, channels: c } });
};

await make("photo-24mp.jpg", (p) => sharp(wall("montclair_light")).resize(6000, 4000, { fit: "cover" }).jpeg({ quality: 85 }).toFile(p));
await make("photo-24mp-progressive.jpg", (p) =>
  sharp(wall("montclair_light")).resize(6000, 4000, { fit: "cover" }).jpeg({ quality: 85, progressive: true }).toFile(p));
await make("photo-3840.jpg", (p) => sharp(wall("montclair_light")).resize(3840).jpeg({ quality: 90 }).toFile(p));
await make("photo-1200.jpg", (p) => sharp(wall("petals_light")).resize(1200).jpeg({ quality: 90 }).toFile(p));
await make("photo-1200-444.jpg", (p) => sharp(wall("petals_light")).resize(1200).jpeg({ quality: 90, chromaSubsampling: "4:4:4" }).toFile(p));
await make("grey.jpg", (p) => sharp(wall("glasscurtains_light")).resize(1600).greyscale().jpeg({ quality: 90 }).toFile(p));
await make("cmyk.jpg", (p) => sharp(wall("futurecity_light")).resize(1600).toColourspace("cmyk").jpeg({ quality: 90 }).toFile(p));
await make("photo-2400.webp", (p) => sharp(wall("flight_light")).resize(2400).webp({ quality: 90 }).toFile(p));
await make("photo-4000.png", (p) => sharp(wall("mermaid_light")).resize(4000).png().toFile(p));
await make("alpha-2000.png", (p) =>
  raw(2000, 1500, 4, (x, y, b, i) => {
    const dx = x - 1000, dy = y - 750, r = Math.sqrt(dx * dx + dy * dy);
    b[i] = (x * 255) / 2000; b[i + 1] = (y * 255) / 1500; b[i + 2] = (Math.sin(r / 20) + 1) * 127;
    b[i + 3] = r < 700 ? 255 : Math.max(0, 255 - (r - 700));
  }).png().toFile(p));
await make("gradient.png", (p) =>
  raw(1600, 1000, 3, (x, y, b, i) => {
    b[i] = (x / 1600) * 255; b[i + 1] = (y / 1000) * 255; b[i + 2] = 128 + 127 * Math.sin((x + y) / 400);
  }).png().toFile(p));
const noise = rng(7);
await make("noise.png", (p) => raw(1024, 1024, 3, (x, y, b, i) => { b[i] = noise() * 256; b[i + 1] = noise() * 256; b[i + 2] = noise() * 256; }).png().toFile(p));
// A flat-colour chart with text: few colours, sharp edges (palette-friendly).
const bars = [120, 340, 260, 410, 180, 300].map((v, k) =>
  `<rect x="${80 + k * 150}" y="${500 - v}" width="100" height="${v}" fill="${["#1f77b4", "#ff7f0e", "#2ca02c", "#d62728", "#9467bd", "#8c564b"][k]}"/>` +
  `<text x="${130 + k * 150}" y="540" font-size="22" text-anchor="middle" font-family="sans-serif">Q${k + 1}</text>`).join("");
const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1100" height="600"><rect width="1100" height="600" fill="#ffffff"/>` +
  `<text x="550" y="50" font-size="34" text-anchor="middle" font-family="sans-serif">Quarterly reads (thousands)</text>` +
  `<line x1="60" y1="500" x2="1040" y2="500" stroke="#333" stroke-width="3"/>${bars}</svg>`;
await make("graphic.png", (p) => sharp(Buffer.from(svg)).png().toFile(p));
await make("graphic-flat.png", (p) => sharp(Buffer.from(svg)).png({ palette: true, colours: 16, dither: 0 }).toFile(p));
await make("screenshot.png", (p) => sharp("/usr/share/doc/redhat-display-fonts/type-specimen@2x.png").png().toFile(p));
