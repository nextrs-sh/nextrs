// TextEncoder.encode checked against Node. The same file runs under Node
// (`node text-encoder.js`), which prints the reference results that
// tests/text_encoder.rs compares against (text-encoder.expected.json).
// The cases cover the boundaries of the short-ASCII fast path Phase 3A
// measured and dropped (src/web_patches.rs), so a future one is held to
// Node too: length 64/65, the first non-ASCII unit at every position,
// 0x7f/0x80, lone and paired surrogates, non-string arguments, and seeded
// random strings.
"use strict";

const hex = (u8) => Array.from(u8, (b) => b.toString(16).padStart(2, "0")).join("");
const digest = (s) => {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
};
const attempt = (f) => {
  try {
    return f();
  } catch (e) {
    return `throws ${e.name}`;
  }
};
let seed = 0x7e57;
const random = () => {
  seed |= 0;
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
const int = (n) => Math.floor(random() * n);

function describe(u8) {
  return {
    type: Object.prototype.toString.call(u8),
    proto: Object.getPrototypeOf(u8) === Uint8Array.prototype,
    length: u8.length,
    byteOffset: u8.byteOffset,
    bufferLength: u8.buffer.byteLength,
    bufferType: Object.prototype.toString.call(u8.buffer),
    hex: hex(u8),
  };
}

function run() {
  const encoder = new TextEncoder();
  const out = { cases: {}, random: [] };
  const add = (name, input) => (out.cases[name] = attempt(() => describe(encoder.encode(input))));

  add("empty", "");
  add("noArgument", undefined);
  out.cases.noArgumentCall = describe(encoder.encode());
  add("null", null);
  add("number", 123.5);
  add("object", { toString: () => "obj" });
  add("array", [1, 2]);
  out.cases.symbol = attempt(() => encoder.encode(Symbol("s")));
  out.cases.wrongThis = attempt(() => TextEncoder.prototype.encode.call({}, "a"));
  for (const n of [1, 2, 7, 8, 31, 32, 63, 64, 65, 66, 127, 128, 1000]) add(`ascii${n}`, "abcdefghij".repeat(110).slice(0, n));
  add("del7f", "\x7f".repeat(64));
  add("x80", "\x80");
  add("controls", "\x00\x01\t\n\r\x1b");
  for (const at of [0, 1, 31, 62, 63, 64, 65]) {
    for (const [label, ch] of [["latin1", "é"], ["bmp", "€"], ["pair", "😀"], ["loneHigh", "\ud800"], ["loneLow", "\udc00"]]) {
      add(`${label}At${at}of64`, "a".repeat(at) + ch + "b".repeat(Math.max(0, 63 - at)));
    }
  }
  add("highAtEnd", "abc\ud83d");
  add("lowFirst", "\ude00abc");
  add("reversedPair", "\ude00\ud83d");

  // Results are fresh, independent arrays.
  const a = encoder.encode("same");
  const b = encoder.encode("same");
  a[0] = 0;
  out.independent = a !== b && b[0] === 115 && a.buffer !== b.buffer;
  // .buffer of a short result is a real ArrayBuffer that can be copied.
  // (Transfer is not compared: deno_core's structuredClone does not detach
  // a transferred buffer's views, with or without NextRS's encoder.)
  const c = encoder.encode("short");
  out.structuredCloneCopy = attempt(() => hex(new Uint8Array(structuredClone(c.buffer))));
  // Buffer.from goes through the encoder.
  const buf = Buffer.from("hi there");
  out.bufferFrom = [Buffer.isBuffer(buf), buf.toString("hex"), buf.length];

  // Random strings: ASCII-heavy, mixed, around the 64-unit boundary.
  const pool = ["a", "Z", "0", " ", "<", ">", '"', "=", "\x7f", "é", "ß", "€", "中", "😀", "\ud800", "\udfff"];
  for (let i = 0; i < 3000; i++) {
    const length = int(4) === 0 ? 60 + int(10) : int(80);
    const asciiOnly = int(2) === 0;
    let s = "";
    for (let j = 0; j < length; j++) {
      s += asciiOnly ? String.fromCharCode(int(128)) : pool[int(pool.length)];
    }
    out.random.push(digest(hex(encoder.encode(s))));
  }
  out.random = digest(out.random.join(","));
  return out;
}

exports.run = run;
exports.handler = async (req, res) => {
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(run()));
};

if (require.main === module) {
  console.log(JSON.stringify(run(), null, 1));
}
