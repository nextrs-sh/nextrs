// UTF-8 conversions of Buffer, checked against Node. The same file runs
// under Node (`node buffer-utf8.js`), which prints the reference results the
// Rust test compares against (buffer-utf8.expected.json).
//
// Covers encoding (Buffer.from, byteLength, write with offsets and length
// limits, fill, alloc) and decoding (toString with ranges, utf8Slice,
// StringDecoder across chunk boundaries) on edge cases (lone and reversed
// surrogates, invalid and overlong sequences, code points beyond U+10FFFF,
// truncated sequences, BOMs) and on seeded random strings and byte arrays,
// whose results are compared through a digest of each case.
"use strict";

const { StringDecoder } = require("string_decoder");

// Hex of the bytes; long outputs as a digest of the hex.
const hex = (b) => {
  const h = Buffer.from(b.buffer, b.byteOffset, b.length).toString("hex");
  return h.length > 256 ? `#${digest(h)}:${h.length}` : h;
};
// FNV-1a over a string's UTF-16 code units: compact and deterministic.
const digest = (s) => {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
};
const units = (s) => {
  const u = Array.from(s, (c) => c.codePointAt(0).toString(16)).join(" ");
  return u.length > 256 ? `#${digest(u)}:${u.length}` : u;
};

// Seeded PRNG (LCG): the same corpus in Node and in NextRS.
let seed = 0x5eed;
const rand = (n) => {
  seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
  return seed % n;
};

const STRINGS = [
  "", "a", "hello, world", "café", "naïve façade", "€", "日本語のテキスト", "😀", "a😀b", "𝔘𝔫𝔦𝔠𝔬𝔡𝔢",
  "\ud800", "\udfff", "a\ud800", "\ud800a", "\udc00\ud800", "\ud800𐀀", "\ud83d", "x😀\ud83d",
  "\u0000", "\u007f\u0080߿ࠀ￿", "﻿BOM", "�", "<div class=\"x\">€ & ©</div>",
  "  ", "é", "a".repeat(5000), "é".repeat(3000), "😀".repeat(1500),
];

const BYTES = [
  [], [0x61], [0xc3, 0xa9], [0xe2, 0x82, 0xac], [0xf0, 0x9f, 0x98, 0x80],
  [0xc0, 0x80], [0xc1, 0xbf], [0xe0, 0x80, 0x80], [0xe0, 0x9f, 0xbf], [0xed, 0xa0, 0x80], [0xed, 0xbf, 0xbf],
  [0xf0, 0x80, 0x80, 0x80], [0xf4, 0x90, 0x80, 0x80], [0xf5, 0x80, 0x80, 0x80], [0xf8, 0x88, 0x80, 0x80, 0x80],
  [0xe2, 0x82], [0xe2], [0xf0, 0x9f, 0x98], [0x80], [0xbf, 0x80], [0xfe], [0xff, 0xfe],
  [0xef, 0xbb, 0xbf, 0x61], [0x61, 0xe2, 0x82, 0x62], [0xc3, 0x28], [0xe2, 0x28, 0xa1], [0xf0, 0x28, 0x8c, 0xbc],
  [0xf0, 0x90, 0x28, 0xbc], [0xf0, 0x9f, 0x98, 0x80, 0xf0, 0x9f], [0x00, 0x7f, 0xc2, 0x80, 0xdf, 0xbf],
];

function randomString(maxLen) {
  const len = rand(maxLen + 1);
  let s = "";
  for (let i = 0; i < len; i++) {
    const kind = rand(10);
    if (kind < 4) s += String.fromCharCode(32 + rand(95));
    else if (kind < 6) s += String.fromCharCode(0x80 + rand(0x780));
    else if (kind < 8) s += String.fromCharCode(0x800 + rand(0xd000));
    else if (kind < 9) s += String.fromCodePoint(0x10000 + rand(0x100000));
    else s += String.fromCharCode(0xd800 + rand(0x800)); // lone surrogate
  }
  return s;
}

function randomBytes(maxLen) {
  const len = rand(maxLen + 1);
  const b = [];
  for (let i = 0; i < len; i++) {
    const kind = rand(6);
    if (kind < 2) b.push(rand(0x80));
    else if (kind < 4) b.push(0x80 + rand(0x40));
    else b.push(0xc0 + rand(0x40));
  }
  return b;
}

function encodeCase(s) {
  const out = {};
  out.from = hex(Buffer.from(s));
  out.fromUtf8 = hex(Buffer.from(s, "utf8"));
  out.fromUtf_8 = hex(Buffer.from(s, "utf-8"));
  out.fromUTF8 = hex(Buffer.from(s, "UTF8"));
  out.byteLength = Buffer.byteLength(s);
  out.byteLengthUtf8 = Buffer.byteLength(s, "utf8");
  // write: a buffer smaller than the encoding, and offsets/limits that cut
  // multibyte characters (Node writes only whole characters).
  const n = Buffer.byteLength(s);
  for (const [size, offset, length] of [[n, 0, undefined], [Math.max(0, n - 1), 0, undefined], [n + 4, 2, undefined], [n + 4, 1, Math.max(0, n - 2)], [8, 3, 4]]) {
    const b = Buffer.alloc(size, 0x2e);
    let written;
    try {
      written = length === undefined ? b.write(s, offset) : b.write(s, offset, length);
    } catch (e) {
      written = `error:${e.name}`;
    }
    out[`write_${size}_${offset}_${length}`] = [written, hex(b)];
  }
  const b2 = Buffer.alloc(12, 0x2e);
  out.writeEncodingArg = [b2.write(s, 1, "utf8"), hex(b2)];
  if (s.length) {
    out.fill = hex(Buffer.alloc(11).fill(s));
    out.alloc = hex(Buffer.alloc(7, s));
    out.allocUtf8 = hex(Buffer.alloc(7, s, "utf8"));
  }
  out.roundTrip = units(Buffer.from(s).toString());
  return out;
}

function decodeCase(bytes) {
  const b = Buffer.from(bytes);
  const out = {};
  out.toString = units(b.toString());
  out.toStringUtf8 = units(b.toString("utf8"));
  out.toStringUtf_8 = units(b.toString("utf-8"));
  out.utf8Slice = units(b.utf8Slice(0, b.length));
  const ranges = [[1], [0, b.length - 1], [1, b.length - 1], [2, 3], [-1], [0, 100], [5, 2]];
  for (const r of ranges) out[`range_${r.join("_")}`] = units(b.toString("utf8", ...r));
  // StringDecoder: every split point.
  const splits = [];
  for (let i = 0; i <= b.length; i++) {
    const d = new StringDecoder("utf8");
    splits.push(units(d.write(b.subarray(0, i)) + d.write(b.subarray(i)) + d.end()));
  }
  out.decoderSplits = splits;
  // Three chunks, end(buf), and a string passed through.
  const third = Math.floor(b.length / 3);
  const d3 = new StringDecoder("utf8");
  out.decoderThree = units(d3.write(b.subarray(0, third)) + d3.write(b.subarray(third, 2 * third)) + d3.write(b.subarray(2 * third)) + d3.end());
  const de = new StringDecoder("utf8");
  out.decoderEndBuf = units(de.write(b.subarray(0, third)) + de.end(b.subarray(third)));
  out.decoderReuse = units(de.write(b) + de.end());
  out.decoderString = new StringDecoder("utf8").write("plain");
  out.subarrayToString = units(b.subarray(1).toString());
  return out;
}

function run() {
  const encode = STRINGS.map((s) => ({ input: units(s.length > 40 ? s.slice(0, 40) : s), ...encodeCase(s) }));
  const decode = BYTES.map((bytes) => ({ input: bytes.map((x) => x.toString(16)).join(" "), ...decodeCase(bytes) }));
  seed = 0x5eed;
  const randomEncode = [];
  for (let i = 0; i < 1000; i++) randomEncode.push(digest(JSON.stringify(encodeCase(randomString(48)))));
  const randomDecode = [];
  for (let i = 0; i < 1000; i++) randomDecode.push(digest(JSON.stringify(decodeCase(randomBytes(24)))));
  // Large conversions (the SSR path): content, not only lengths.
  const big = "<p>Ünïcödé — 日本語 😀</p>".repeat(20000);
  const bigBuf = Buffer.from(big);
  const large = { byteLength: bigBuf.length, digestHex: digest(bigBuf.toString("hex")), roundTrip: bigBuf.toString() === big, sliced: digest(bigBuf.toString("utf8", 7, 200007)) };
  return { encode, decode, randomEncode: digest(randomEncode.join()), randomEncodeCases: randomEncode, randomDecode: digest(randomDecode.join()), randomDecodeCases: randomDecode, large };
}

// Diagnosis: the full output of one random decode case (by index), to see
// which field a digest mismatch comes from.
function randomDecodeCase(index) {
  seed = 0x5eed;
  for (let i = 0; i < 1000; i++) randomEncode_skip(48);
  let bytes;
  for (let i = 0; i <= index; i++) bytes = randomBytes(24);
  return { bytes: bytes.map((x) => x.toString(16)).join(" "), ...decodeCase(bytes) };
}
// Advances the PRNG exactly as one random encode case does.
function randomEncode_skip(maxLen) {
  encodeCase(randomString(maxLen));
}

exports.run = run;
exports.handler = async (req, res) => {
  res.setHeader("content-type", "application/json");
  const one = req.headers["x-decode-case"];
  res.end(JSON.stringify(one === undefined ? run() : randomDecodeCase(Number(one))));
};

if (require.main === module) {
  const one = process.argv[2];
  console.log(JSON.stringify(one === undefined ? run() : randomDecodeCase(Number(one)), null, 1));
}
