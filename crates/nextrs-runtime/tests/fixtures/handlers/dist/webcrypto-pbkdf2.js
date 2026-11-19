// Web Crypto PBKDF2 deriveBits across hashes, iteration counts and lengths,
// and the errors Web Crypto specifies. The same code runs under Node.js to
// record webcrypto-pbkdf2.expected.json (see tests/webapis.rs).
const run = async () => {
  const subtle = globalThis.crypto.subtle;
  const enc = new TextEncoder();
  const hex = (b) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, "0")).join("");
  const out = {};
  for (const hash of ["SHA-1", "SHA-256", "SHA-384", "SHA-512"]) {
    for (const [password, salt, iterations, bits] of [["password", "salt", 1, 160], ["password", "salt", 4096, 256], ["", "salt", 2, 128], ["pässwörd", "", 3, 512]]) {
      const key = await subtle.importKey("raw", enc.encode(password), { name: "PBKDF2" }, false, ["deriveBits"]);
      out[`${hash}:${password}:${salt}:${iterations}:${bits}`] = hex(await subtle.deriveBits({ name: "PBKDF2", hash, salt: enc.encode(salt), iterations }, key, bits));
    }
  }
  const key = await subtle.importKey("raw", enc.encode("k"), "pbkdf2", false, ["deriveBits"]);
  out.keyAlgorithm = key.algorithm.name;
  out.keyUsages = key.usages;
  out.keyExtractable = key.extractable;
  const fail = async (f) => { try { await f(); return "ok"; } catch (e) { return e.name; } };
  out.extractableImport = await fail(() => subtle.importKey("raw", enc.encode("k"), "PBKDF2", true, ["deriveBits"]));
  out.jwkImport = await fail(() => subtle.importKey("jwk", { kty: "oct", k: "aw" }, "PBKDF2", false, ["deriveBits"]));
  out.signUsage = await fail(() => subtle.importKey("raw", enc.encode("k"), "PBKDF2", false, ["sign"]));
  out.zeroIterations = await fail(() => subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt: enc.encode("s"), iterations: 0 }, key, 256));
  out.oddLength = await fail(() => subtle.deriveBits({ name: "PBKDF2", hash: "SHA-256", salt: enc.encode("s"), iterations: 1 }, key, 12));
  out.export = await fail(() => subtle.exportKey("raw", key));
  return out;
};

if (typeof exports === "object" && typeof require === "function" && require.main === module) {
  run().then((r) => console.log(JSON.stringify(r, null, 2)));
}
exports.handler = async (_req, res) => {
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(await run()));
};
