// Base64 conversions of Buffer, checked against Node. The same file runs
// under Node (`node buffer-base64.js`), which prints the reference results
// the Rust test compares against (buffer-base64.expected.json).
//
// Covers decoding (Buffer.from, byteLength, write) of canonical, unpadded,
// whitespace-laden, URL-safe, invalid, mid-padded and truncated input,
// non-Latin-1 strings and non-zero trailing bits; encoding (toString with
// ranges, base64url); and seeded random byte arrays, compared through a
// digest of each case.
"use strict";

const hex = (b) => {
  const h = Buffer.from(b.buffer, b.byteOffset, b.length).toString("hex");
  return h.length > 256 ? `#${digest(h)}:${h.length}` : h;
};
const digest = (s) => {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
};
const short = (s) => (s.length > 120 ? `#${digest(s)}:${s.length}` : s);
const attempt = (f) => {
  try {
    return f();
  } catch (e) {
    return `throws ${e.name}`;
  }
};

// Deterministic PRNG (mulberry32).
let seed = 0x5eed64;
const random = () => {
  seed |= 0;
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
const int = (n) => Math.floor(random() * n);
const randomBytes = (max) => {
  const b = new Uint8Array(int(max + 1));
  for (let i = 0; i < b.length; i++) b[i] = int(256);
  return b;
};

function decodeCase(s) {
  const out = {
    from: attempt(() => hex(Buffer.from(s, "base64"))),
    fromUpper: attempt(() => hex(Buffer.from(s, "BASE64"))),
    byteLength: attempt(() => Buffer.byteLength(s, "base64")),
    url: attempt(() => hex(Buffer.from(s, "base64url"))),
  };
  const target = Buffer.alloc(12, 0xaa);
  out.write = attempt(() => [target.write(s, 2, 7, "base64"), hex(target)]);
  return out;
}

function encodeCase(bytes) {
  const b = Buffer.from(bytes);
  const n = b.length;
  return {
    b64: short(b.toString("base64")),
    url: short(b.toString("base64url")),
    range: short(b.toString("base64", 1, Math.max(1, n - 1))),
    rangeOut: b.toString("base64", n + 3, n + 9),
    roundTrip: hex(Buffer.from(b.toString("base64"), "base64")) === hex(b),
  };
}

const EDGE = [
  "",
  "QQ==",
  "QQ",
  "QQ=",
  "QUI=",
  "QUI",
  "QUJD",
  "QUJDRA==",
  "QUJDR",
  "QR==",
  "QUJ=",
  "Q===",
  "====",
  "=",
  "QQ==QQ==",
  "QU JD",
  "QUJD\nRA==",
  " QUJD ",
  "\tQUJD\r\n",
  "QUJD!RA==",
  "QU*JD",
  "-_-_",
  "+/+/",
  "a-b_",
  "YWJj ZGVm",
  "Ł",
  "QUJDŁ",
  "QUJD\u0000",
  "QUJDÿ",
  "😀",
  "AAAA".repeat(1000),
  "////".repeat(257),
  "SGVsbG8sIHdvcmxkIQ==",
  "SGVsbG8sIHdvcmxkIQ",
  "SGVsbG8s IHdvcmxkIQ==",
];

function run() {
  const result = { edgeDecode: {}, edgeEncode: {}, randomDecode: [], randomEncode: [], dirty: [] };
  for (const s of EDGE) result.edgeDecode[JSON.stringify(s).slice(0, 80)] = decodeCase(s);
  for (const bytes of [[], [0], [0, 1], [0, 1, 2], [255, 254, 253, 252], Array.from({ length: 70000 }, (_, i) => i & 255)]) {
    result.edgeEncode[bytes.length] = encodeCase(Uint8Array.from(bytes));
  }
  for (let i = 0; i < 1000; i++) {
    const bytes = randomBytes(i < 900 ? 64 : 4096);
    let s = Buffer.from(bytes).toString("base64");
    if (int(3) === 0) s = s.replace(/=+$/, "");
    result.randomDecode.push(digest(JSON.stringify(decodeCase(s))));
    result.randomEncode.push(digest(JSON.stringify(encodeCase(bytes))));
  }
  // Canonical strings with one random alteration each.
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=-_ \n\t!.Ł";
  for (let i = 0; i < 1000; i++) {
    let s = Buffer.from(randomBytes(32)).toString("base64");
    const at = int(s.length + 1);
    const op = int(3);
    const c = alphabet[int(alphabet.length)];
    if (op === 0) s = s.slice(0, at) + c + s.slice(at);
    else if (op === 1) s = s.slice(0, at) + s.slice(at + 1);
    else s = s.slice(0, at) + c + s.slice(at + 1);
    result.dirty.push(digest(JSON.stringify(decodeCase(s))));
  }
  return result;
}

exports.run = run;
exports.handler = async (req, res) => {
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(run()));
};

if (require.main === module) {
  console.log(JSON.stringify(run(), null, 1));
}
