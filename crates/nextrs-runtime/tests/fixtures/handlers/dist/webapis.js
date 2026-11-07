// Web Crypto (AES-GCM, HMAC) and FormData checks. The same file runs under Node
// (`node webapis.js`), which prints the reference results the Rust test
// compares against (webapis.expected.json); keys under `nextrsOnly` are
// NextRS's documented refusals, which Node does not share.
"use strict";

const hex = (buf) => Buffer.from(buf instanceof ArrayBuffer ? new Uint8Array(buf) : buf).toString("hex");
const unhex = (s) => Uint8Array.from(Buffer.from(s, "hex"));
const outcome = async (fn) => {
  try {
    const value = await fn();
    return { ok: value === undefined ? null : value };
  } catch (error) {
    return { error: error?.code === "ERR_NEXTRS_UNSUPPORTED" ? "ERR_NEXTRS_UNSUPPORTED" : `${error?.name}` };
  }
};

async function crypto_() {
  const subtle = globalThis.crypto.subtle;
  const r = {};
  // McGrew & Viega GCM test case 16 (AES-256, 96-bit IV, AAD).
  const kat = {
    key: "feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308",
    iv: "cafebabefacedbaddecaf888",
    pt: "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
    aad: "feedfacedeadbeeffeedfacedeadbeefabaddad2",
  };
  const katKey = await subtle.importKey("raw", unhex(kat.key), "AES-GCM", false, ["encrypt", "decrypt"]);
  const katCt = await subtle.encrypt({ name: "AES-GCM", iv: unhex(kat.iv), additionalData: unhex(kat.aad) }, katKey, unhex(kat.pt));
  r.katCiphertext = hex(katCt);
  r.katDecrypt = hex(await subtle.decrypt({ name: "AES-GCM", iv: unhex(kat.iv), additionalData: unhex(kat.aad) }, katKey, katCt));
  r.katWrongAad = await outcome(() => subtle.decrypt({ name: "AES-GCM", iv: unhex(kat.iv) }, katKey, katCt));

  // What Next.js does (encryption.ts): base64 key, 16-byte IV, AES-GCM.
  const rawKey = Buffer.alloc(32, 7).toString("base64");
  const key = await subtle.importKey("raw", Uint8Array.from(atob(rawKey), (c) => c.charCodeAt(0)), "AES-GCM", true, ["encrypt", "decrypt"]);
  r.keyType = key.type;
  r.keyAlgorithm = { ...key.algorithm };
  r.keyUsages = [...key.usages];
  r.keyExtractable = key.extractable;
  r.keyTag = Object.prototype.toString.call(key);
  r.keyInstance = key instanceof CryptoKey;
  const iv = new Uint8Array(16).fill(9);
  const actionId = "402c5c9cb77c704c278b15a8793d4cef4cfbb64da0";
  const payload = new TextEncoder().encode(`${actionId}["$@1",["closure",3]]`);
  const ct = await subtle.encrypt({ name: "AES-GCM", iv }, key, payload);
  r.nextCiphertext = hex(ct);
  r.nextDecrypt = new TextDecoder().decode(await subtle.decrypt({ name: "AES-GCM", iv }, key, ct));
  const tampered = new Uint8Array(ct);
  tampered[5] ^= 1;
  r.tamperedCiphertext = await outcome(() => subtle.decrypt({ name: "AES-GCM", iv }, key, tampered));
  const tamperedTag = new Uint8Array(ct);
  tamperedTag[tamperedTag.length - 1] ^= 1;
  r.tamperedTag = await outcome(() => subtle.decrypt({ name: "AES-GCM", iv }, key, tamperedTag));
  r.truncated = await outcome(() => subtle.decrypt({ name: "AES-GCM", iv }, key, new Uint8Array(ct).slice(0, 10)));
  const otherIv = new Uint8Array(16).fill(8);
  r.wrongIv = await outcome(() => subtle.decrypt({ name: "AES-GCM", iv: otherIv }, key, ct));
  const otherKey = await subtle.importKey("raw", new Uint8Array(32).fill(6), { name: "AES-GCM" }, false, ["decrypt"]);
  r.wrongKey = await outcome(() => subtle.decrypt({ name: "AES-GCM", iv }, otherKey, ct));
  r.exported = hex(await subtle.exportKey("raw", key));
  r.exportNonExtractable = await outcome(() => subtle.exportKey("raw", otherKey));
  r.encryptWithoutUsage = await outcome(() => subtle.encrypt({ name: "AES-GCM", iv }, otherKey, payload));
  r.badKeyLength = await outcome(() => subtle.importKey("raw", new Uint8Array(20), "AES-GCM", false, ["encrypt"]));
  r.badUsage = await outcome(() => subtle.importKey("raw", new Uint8Array(32), "AES-GCM", false, ["sign"]));
  r.noUsages = await outcome(() => subtle.importKey("raw", new Uint8Array(32), "AES-GCM", false, []));
  r.missingIv = await outcome(() => subtle.encrypt({ name: "AES-GCM" }, key, payload));
  r.badTagLength = await outcome(() => subtle.encrypt({ name: "AES-GCM", iv, tagLength: 100 }, key, payload));
  r.notAKey = await outcome(() => subtle.encrypt({ name: "AES-GCM", iv }, {}, payload));
  const generated = await subtle.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]);
  r.generatedLength = (await subtle.exportKey("raw", generated)).byteLength;
  r.generatedRoundTrip = new TextDecoder().decode(
    await subtle.decrypt({ name: "AES-GCM", iv }, generated, await subtle.encrypt({ name: "AES-GCM", iv }, generated, payload)),
  );
  r.illegalConstructor = await outcome(() => new CryptoKey());

  // HMAC (jose's HS256/384/512): RFC 4231 case 2, key shape, usages, and
  // verification failures.
  const hmacKey = await subtle.importKey("raw", new TextEncoder().encode("Jefe"), { name: "HMAC", hash: "SHA-256" }, true, ["sign", "verify"]);
  const hmacData = new TextEncoder().encode("what do ya want for nothing?");
  r.hmacAlgorithm = [hmacKey.algorithm.name, hmacKey.algorithm.hash.name, hmacKey.algorithm.length, hmacKey.type, [...hmacKey.usages]];
  const mac = await subtle.sign("HMAC", hmacKey, hmacData);
  r.hmacSign = hex(mac);
  r.hmacVerify = await subtle.verify({ name: "HMAC" }, hmacKey, mac, hmacData);
  const badMac = new Uint8Array(mac);
  badMac[0] ^= 1;
  r.hmacVerifyTampered = await subtle.verify("HMAC", hmacKey, badMac, hmacData);
  r.hmacVerifyTruncated = await subtle.verify("HMAC", hmacKey, new Uint8Array(mac).slice(0, 31), hmacData);
  r.hmacVerifyOtherData = await subtle.verify("HMAC", hmacKey, mac, new TextEncoder().encode("other"));
  const hmac512 = await subtle.importKey("raw", new TextEncoder().encode("Jefe"), { name: "hmac", hash: { name: "sha-512" } }, false, ["verify"]);
  r.hmac512Algorithm = [hmac512.algorithm.name, hmac512.algorithm.hash.name];
  r.hmac512SignWithoutUsage = await outcome(() => subtle.sign("HMAC", hmac512, hmacData));
  r.hmacExported = hex(await subtle.exportKey("raw", hmacKey));
  r.hmacNoHash = await outcome(() => subtle.importKey("raw", new Uint8Array(32), { name: "HMAC" }, false, ["sign"]));
  r.hmacBadHash = await outcome(() => subtle.importKey("raw", new Uint8Array(32), { name: "HMAC", hash: "MD5" }, false, ["sign"]));
  r.hmacBadUsage = await outcome(() => subtle.importKey("raw", new Uint8Array(32), { name: "HMAC", hash: "SHA-256" }, false, ["encrypt"]));
  r.hmacEmptyKey = await outcome(() => subtle.importKey("raw", new Uint8Array(0), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]));
  r.hmacWithAesKey = await outcome(() => subtle.sign("HMAC", key, hmacData));
  r.aesWithHmacKey = await outcome(() => subtle.encrypt({ name: "AES-GCM", iv }, hmacKey, payload));
  const generatedHmac = await subtle.generateKey({ name: "HMAC", hash: "SHA-384" }, true, ["sign", "verify"]);
  r.generatedHmac = [generatedHmac.algorithm.length, (await subtle.exportKey("raw", generatedHmac)).byteLength];
  r.generatedHmacRoundTrip = await subtle.verify("HMAC", generatedHmac, await subtle.sign("HMAC", generatedHmac, hmacData), hmacData);
  const nextrsOnly = {
    iv8: await outcome(() => subtle.encrypt({ name: "AES-GCM", iv: new Uint8Array(8) }, key, payload)),
    tag96: await outcome(() => subtle.encrypt({ name: "AES-GCM", iv, tagLength: 96 }, key, payload)),
  };
  return { r, nextrsOnly };
}

async function formData_() {
  const r = {};
  const form = new FormData();
  form.append("a", "1");
  form.append("a", "2");
  form.append("b", new Blob(["blob-bytes"], { type: "text/x-test" }));
  form.append("c", new Blob(["named"]), "named.txt");
  form.append("d", new File(["file"], "orig.bin", { type: "application/x-orig" }));
  form.append("lone", "\ud800x");
  r.getA = form.get("a");
  r.getAllA = form.getAll("a");
  r.missing = form.get("zzz");
  r.has = [form.has("a"), form.has("zzz")];
  r.blobName = [form.get("b").name, form.get("b").type, form.get("b") instanceof File];
  r.namedName = form.get("c").name;
  r.fileName = [form.get("d").name, form.get("d").type];
  r.lone = form.get("lone");
  form.set("a", "3");
  r.afterSet = [...form.keys()];
  r.afterSetA = form.getAll("a");
  form.delete("lone");
  r.afterDelete = [...form.keys()];
  r.tag = Object.prototype.toString.call(form);
  r.threeArgString = await outcome(() => form.append("x", "y", "z.txt"));
  const seen = [];
  form.forEach((value, name) => seen.push(name));
  r.forEach = seen;

  // Serialization through a body, and parsing back.
  const out = new FormData();
  out.append('q"uote\nname', "line1\nline2\rline3");
  out.append("unicode ✓", "café 📚");
  out.append("file", new File([Uint8Array.from([0, 1, 2, 255, 13, 10, 45, 45])], 'f"\r.bin', { type: "" }));
  out.append("empty", "");
  const request = new Request("http://n/", { method: "POST", body: out });
  const contentType = request.headers.get("content-type");
  r.contentType = contentType.replace(/boundary=.*/, "boundary=…");
  const raw = await request.text();
  const boundary = contentType.split("boundary=")[1];
  r.serialized = raw.split(boundary).join("BOUNDARY");
  const back = await new Response(raw, { headers: { "content-type": contentType } }).formData();
  r.parsed = [];
  for (const [name, value] of back) {
    r.parsed.push(typeof value === "string" ? [name, value] : [name, { name: value.name, type: value.type, bytes: hex(new Uint8Array(await value.arrayBuffer())) }]);
  }
  const noType = [
    "--B", 'Content-Disposition: form-data; name="f"; filename="a.txt"', "", "x",
    "--B", 'Content-Disposition: form-data; name="g"; filename=""', "Content-Type: application/octet-stream", "", "",
    "--B--", "",
  ].join("\r\n");
  const parsedNoType = await new Response(noType, { headers: { "content-type": "multipart/form-data; boundary=B" } }).formData();
  r.defaultFileType = [parsedNoType.get("f").type, parsedNoType.get("g").name, parsedNoType.get("g").size];
  const url = await new Response("a=1&a=2&b=%F0%9F%93%9A+x&c", { headers: { "content-type": "application/x-www-form-urlencoded;charset=UTF-8" } }).formData();
  r.urlencoded = [...url];
  r.truncated = await outcome(() =>
    new Response('--B\r\nContent-Disposition: form-data; name="a"\r\n\r\n1', { headers: { "content-type": "multipart/form-data; boundary=B" } }).formData(),
  );
  r.noBoundary = await outcome(() => new Response("--B--\r\n", { headers: { "content-type": "multipart/form-data" } }).formData());
  r.wrongType = await outcome(() => new Response("x", { headers: { "content-type": "text/plain" } }).formData());
  return r;
}

function buffers_() {
  const buf = Buffer.from([0x63, 0x61, 0x66, 0xc3, 0xa9, 0xff]);
  return {
    latin1: buf.latin1Slice(0, buf.length),
    latin1Default: buf.latin1Slice(),
    utf8: buf.utf8Slice(0, 5),
    ascii: buf.asciiSlice(1, 3),
    hex: buf.hexSlice(2, 4),
    base64: buf.base64Slice(0, 3),
  };
}

async function run() {
  const crypto = await crypto_();
  return { crypto: crypto.r, formData: await formData_(), buffers: buffers_(), nextrsOnly: crypto.nextrsOnly };
}

exports.run = run;
exports.handler = async (req, res) => {
  const out = await run();
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(out));
};

if (require.main === module) {
  run().then((out) => {
    delete out.nextrsOnly;
    console.log(JSON.stringify(out, null, 2));
  });
}
