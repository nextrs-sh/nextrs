// NextRS bootstrap: Web Crypto (globalThis.crypto, CryptoKey, SubtleCrypto),
// which node:crypto's webcrypto/subtle re-export (02_node.js).
//
// Implemented (src/webcrypto/): digest (SHA-1/256/384/512); AES-GCM and
// AES-CBC; HMAC; HKDF deriveBits; ECDSA P-256/P-384, RSASSA-PKCS1-v1_5,
// RSA-PSS and Ed25519 sign/verify; key formats raw, spki, pkcs8 and jwk
// where the algorithm has them; generateKey for AES, HMAC, ECDSA and
// Ed25519. Key material stays in Rust: a CryptoKey holds a handle, released
// when the key is garbage collected. Everything else fails explicitly:
// algorithms NextRS lacks with NextRSUnsupportedError
// (ERR_NEXTRS_UNSUPPORTED), invalid use with the DOMException Web Crypto
// specifies. This file checks what Web Crypto checks against the request
// (names, usages, extractability, JWK members); Rust validates key
// material and checks the key's algorithm again for every operation.
"use strict";

((globalThis) => {
  const internal = globalThis.__nextrsInternal;
  const { ops, NextRSUnsupportedError } = internal;

  const domError = (name, message) => new DOMException(message, name);
  // Host errors are "Name: message"; TypeError stays a TypeError.
  function hostError(error) {
    const text = String(error?.message ?? error);
    const match = /^(\w+Error): ([\s\S]*)$/.exec(text);
    if (!match) return domError("OperationError", text);
    if (match[1] === "TypeError") return new TypeError(match[2]);
    return domError(match[1], match[2]);
  }
  const host = (fn) => {
    try {
      return fn();
    } catch (error) {
      throw hostError(error);
    }
  };

  // --- Names --------------------------------------------------------------
  const REGISTERED = ["AES-GCM", "AES-CBC", "HMAC", "HKDF", "ECDSA", "RSASSA-PKCS1-v1_5", "RSA-PSS", "Ed25519"];
  // Web Crypto algorithms NextRS knows but does not implement.
  const NOT_IMPLEMENTED = ["RSA-OAEP", "ECDH", "AES-CTR", "AES-KW", "PBKDF2", "X25519", "X448", "Ed448", "SHA-3"];
  const HASHES = ["SHA-1", "SHA-256", "SHA-384", "SHA-512"];
  const byUpper = (names) => Object.fromEntries(names.map((n) => [n.toUpperCase(), n]));
  const REGISTERED_BY_UPPER = byUpper(REGISTERED);
  const HASH_BY_UPPER = byUpper(HASHES);
  const NOT_IMPLEMENTED_BY_UPPER = byUpper(NOT_IMPLEMENTED);
  const SIGNATURE = ["ECDSA", "RSASSA-PKCS1-v1_5", "RSA-PSS", "Ed25519"];
  const SECRET = ["AES-GCM", "AES-CBC", "HMAC", "HKDF"];
  const RSA = ["RSASSA-PKCS1-v1_5", "RSA-PSS"];

  // The operations each algorithm has here.
  const OPERATIONS = {
    importKey: REGISTERED,
    exportKey: REGISTERED,
    generateKey: ["AES-GCM", "AES-CBC", "HMAC", "ECDSA", "Ed25519", ...RSA],
    sign: ["HMAC", ...SIGNATURE],
    verify: ["HMAC", ...SIGNATURE],
    encrypt: ["AES-GCM", "AES-CBC"],
    decrypt: ["AES-GCM", "AES-CBC"],
    deriveBits: ["HKDF"],
  };

  function algorithmName(algorithm, operation) {
    const name = typeof algorithm === "string" ? algorithm : algorithm?.name;
    if (typeof name !== "string") throw new TypeError(`crypto.subtle.${operation}: algorithm.name is required`);
    return name;
  }

  // The registered name (matched ignoring ASCII case), or the error Web
  // Crypto (or NextRS, for what it lacks) gives.
  function normalizeName(algorithm, operation) {
    const given = algorithmName(algorithm, operation);
    const upper = given.toUpperCase();
    const name = REGISTERED_BY_UPPER[upper];
    if (name && OPERATIONS[operation].includes(name)) return name;
    if (name || NOT_IMPLEMENTED_BY_UPPER[upper] || HASH_BY_UPPER[upper]) {
      throw new NextRSUnsupportedError("crypto.subtle", operation, `algorithm ${given}`);
    }
    throw domError("NotSupportedError", `Unrecognized algorithm name '${given}'`);
  }

  function normalizeHash(hash, operation) {
    if (hash === undefined) throw new TypeError(`crypto.subtle.${operation}: the algorithm needs a hash`);
    const given = typeof hash === "string" ? hash : hash?.name;
    const name = HASH_BY_UPPER[String(given).toUpperCase()];
    if (!name) throw domError("NotSupportedError", `Unrecognized hash '${given}'`);
    return name;
  }

  function bufferSource(data, what) {
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    if (data instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && data instanceof SharedArrayBuffer)) return new Uint8Array(data);
    throw new TypeError(`${what} must be an ArrayBuffer or ArrayBufferView`);
  }
  // Web Crypto copies its inputs before it starts.
  const copy = (data, what) => bufferSource(data, what).slice();
  const arrayBuffer = (out) => out.buffer.slice(out.byteOffset, out.byteOffset + out.byteLength);

  // --- CryptoKey ------------------------------------------------------------
  const kHandle = Symbol("key handle");
  const kKeyData = Symbol("key data");
  const releaseKey = new FinalizationRegistry((id) => ops.op_nextrs_key_release(id));
  const KEY_USAGES = ["encrypt", "decrypt", "sign", "verify", "deriveKey", "deriveBits", "wrapKey", "unwrapKey"];

  class CryptoKey {
    constructor() {
      throw new TypeError("Illegal constructor");
    }
    get type() { return this[kKeyData].type; }
    get extractable() { return this[kKeyData].extractable; }
    get algorithm() { return this[kKeyData].algorithm; }
    get usages() { return this[kKeyData].usages; }
    get [Symbol.toStringTag]() { return "CryptoKey"; }
  }

  // The usages each key type of an algorithm may have.
  function allowedUsages(name, type) {
    if (name === "AES-GCM" || name === "AES-CBC") return ["encrypt", "decrypt", "wrapKey", "unwrapKey"];
    if (name === "HMAC") return ["sign", "verify"];
    if (name === "HKDF") return ["deriveKey", "deriveBits"];
    return type === "public" ? ["verify"] : ["sign"];
  }

  function parseUsages(usages, operation) {
    const list = [...(usages ?? [])].map(String);
    for (const usage of list) {
      if (!KEY_USAGES.includes(usage)) throw new TypeError(`crypto.subtle.${operation}: invalid key usage '${usage}'`);
    }
    return list;
  }

  function checkUsages(usages, name, type) {
    const allowed = allowedUsages(name, type);
    for (const usage of usages) {
      if (!allowed.includes(usage)) throw domError("SyntaxError", `'${usage}' is not a valid usage for this ${name} key`);
    }
    if (usages.length === 0 && type !== "public") throw domError("SyntaxError", `a ${type} key needs at least one usage`);
  }

  // CryptoKey.algorithm for a key Rust created (`info`).
  function keyAlgorithm(name, spec, info) {
    switch (name) {
      case "AES-GCM":
      case "AES-CBC":
        return { name, length: info.length };
      case "HMAC":
        return { name, length: info.length, hash: Object.freeze({ name: spec.hash }) };
      case "ECDSA":
        return { name, namedCurve: spec.namedCurve };
      case "RSASSA-PKCS1-v1_5":
      case "RSA-PSS":
        return { name, modulusLength: info.modulusLength, publicExponent: Uint8Array.from(info.publicExponent), hash: Object.freeze({ name: spec.hash }) };
      default:
        return { name };
    }
  }

  function makeKey(name, spec, info, extractable, usages) {
    const key = Object.create(CryptoKey.prototype);
    key[kHandle] = info.handle;
    key[kKeyData] = {
      type: info.type,
      extractable: Boolean(extractable),
      algorithm: Object.freeze(keyAlgorithm(name, spec, info)),
      usages: Object.freeze(KEY_USAGES.filter((usage) => usages.includes(usage))),
    };
    releaseKey.register(key, info.handle);
    return key;
  }

  function useKey(key, name, usage, operation) {
    if (!(key instanceof CryptoKey) || key[kHandle] === undefined) throw new TypeError(`crypto.subtle.${operation}: key is not a CryptoKey`);
    if (key.algorithm.name !== name) throw domError("InvalidAccessError", "the key's algorithm does not match the operation");
    if (!key.usages.includes(usage)) throw domError("InvalidAccessError", `the key does not allow '${usage}'`);
    return key[kHandle];
  }

  // The algorithm description Rust needs for a key.
  function keySpec(name, algorithm, operation) {
    const spec = { name };
    if (name === "HMAC" || RSA.includes(name)) spec.hash = normalizeHash(algorithm.hash, operation);
    if (name === "ECDSA") {
      if (typeof algorithm.namedCurve !== "string") throw new TypeError(`crypto.subtle.${operation}: ECDSA needs a namedCurve`);
      spec.namedCurve = algorithm.namedCurve;
    }
    return spec;
  }

  // --- JWK ---------------------------------------------------------------------
  const HASH_BITS = { "SHA-1": "1", "SHA-256": "256", "SHA-384": "384", "SHA-512": "512" };
  // The `alg` a JWK may name for a key, and the one export sets (null: none).
  function jwkAlg(name, spec, lengthBits) {
    switch (name) {
      case "HMAC":
        return { accept: [`HS${HASH_BITS[spec.hash]}`], export: `HS${HASH_BITS[spec.hash]}` };
      case "AES-GCM":
        return { accept: [`A${lengthBits}GCM`], export: `A${lengthBits}GCM` };
      case "AES-CBC":
        return { accept: [`A${lengthBits}CBC`], export: `A${lengthBits}CBC` };
      case "RSASSA-PKCS1-v1_5":
        return { accept: [`RS${HASH_BITS[spec.hash]}`], export: `RS${HASH_BITS[spec.hash]}` };
      case "RSA-PSS":
        return { accept: [`PS${HASH_BITS[spec.hash]}`], export: `PS${HASH_BITS[spec.hash]}` };
      case "ECDSA":
        return { accept: [{ "P-256": "ES256", "P-384": "ES384", "P-521": "ES512" }[spec.namedCurve]], export: null };
      case "Ed25519":
        return { accept: ["Ed25519", "EdDSA"], export: "Ed25519" };
      default:
        return { accept: [], export: null };
    }
  }
  const JWK_KTY = { HMAC: "oct", "AES-GCM": "oct", "AES-CBC": "oct", ECDSA: "EC", "RSASSA-PKCS1-v1_5": "RSA", "RSA-PSS": "RSA", Ed25519: "OKP" };
  const JWK_MEMBERS = ["kty", "crv", "x", "y", "d", "n", "e", "p", "q", "dp", "dq", "qi", "k"];

  function checkJwk(jwk, name, spec, usages, extractable) {
    if (jwk === null || typeof jwk !== "object") throw new TypeError("crypto.subtle.importKey: a 'jwk' key must be an object");
    if (jwk.kty !== JWK_KTY[name]) throw domError("DataError", `the JWK's kty must be '${JWK_KTY[name]}'`);
    const wantedUse = SECRET.includes(name) && name !== "HMAC" ? "enc" : "sig";
    if (jwk.use !== undefined && usages.length > 0 && jwk.use !== wantedUse) throw domError("DataError", `the JWK's use must be '${wantedUse}'`);
    if (jwk.key_ops !== undefined) {
      if (!Array.isArray(jwk.key_ops) || new Set(jwk.key_ops).size !== jwk.key_ops.length) throw domError("DataError", "invalid JWK key_ops");
      for (const usage of usages) {
        if (!jwk.key_ops.includes(usage)) throw domError("DataError", `the JWK's key_ops do not allow '${usage}'`);
      }
    }
    if (jwk.ext === false && extractable) throw domError("DataError", "the JWK is not extractable");
    if (jwk.alg !== undefined) {
      const length = name.startsWith("AES") && typeof jwk.k === "string" ? Math.floor((jwk.k.length * 6) / 8) * 8 : undefined;
      if (!jwkAlg(name, spec, length).accept.includes(jwk.alg)) throw domError("DataError", `the JWK's alg '${jwk.alg}' does not fit the algorithm`);
    }
    const members = {};
    for (const member of JWK_MEMBERS) {
      if (jwk[member] === undefined) continue;
      if (typeof jwk[member] !== "string") throw domError("DataError", `the JWK's '${member}' must be a string`);
      members[member] = jwk[member];
    }
    if (jwk.oth !== undefined) members.oth = jwk.oth;
    return members;
  }

  // --- AES ---------------------------------------------------------------------------
  function aes(encrypt, algorithm, key, data) {
    const operation = encrypt ? "encrypt" : "decrypt";
    const name = normalizeName(algorithm, operation);
    if (algorithm.iv === undefined) throw new TypeError(`crypto.subtle.${operation}: ${name} needs an iv`);
    const iv = copy(algorithm.iv, "iv");
    if (name === "AES-CBC") {
      const handle = useKey(key, name, operation, operation);
      if (iv.byteLength !== 16) throw domError("OperationError", "the AES-CBC iv must be 16 bytes");
      return arrayBuffer(host(() => ops.op_nextrs_aes_cbc(handle, encrypt, iv, copy(data, "data"))));
    }
    const aad = algorithm.additionalData === undefined ? new Uint8Array(0) : copy(algorithm.additionalData, "additionalData");
    if (algorithm.tagLength !== undefined && algorithm.tagLength !== 128) {
      if ([32, 64, 96, 104, 112, 120].includes(algorithm.tagLength)) {
        throw new NextRSUnsupportedError("crypto.subtle", operation, `AES-GCM tagLength ${algorithm.tagLength} (NextRS supports 128)`);
      }
      throw domError("OperationError", "invalid AES-GCM tagLength");
    }
    const handle = useKey(key, name, operation, operation);
    return arrayBuffer(host(() => ops.op_nextrs_aes_gcm(handle, encrypt, iv, aad, copy(data, "data"))));
  }

  // --- Signatures ---------------------------------------------------------------------
  function signSpec(name, algorithm, operation) {
    const spec = { name };
    if (name === "ECDSA") spec.hash = normalizeHash(algorithm.hash, operation);
    if (name === "RSA-PSS") {
      if (algorithm.saltLength === undefined) throw new TypeError(`crypto.subtle.${operation}: RSA-PSS needs a saltLength`);
      const salt = Number(algorithm.saltLength);
      if (!Number.isInteger(salt) || salt < 0 || salt > 0xffffffff) throw new TypeError(`crypto.subtle.${operation}: invalid saltLength`);
      spec.saltLength = salt;
    }
    return spec;
  }

  class SubtleCrypto {
    constructor() {
      throw new TypeError("Illegal constructor");
    }

    async digest(algorithm, data) {
      const given = algorithmName(algorithm, "digest");
      const name = HASH_BY_UPPER[given.toUpperCase()];
      if (!name) {
        if (REGISTERED_BY_UPPER[given.toUpperCase()]) throw domError("NotSupportedError", `${given} is not a digest`);
        throw new NextRSUnsupportedError("crypto.subtle", "digest", `algorithm ${given}`);
      }
      const bytes = copy(data, "data");
      return arrayBuffer(ops.op_nextrs_hash(name.replace("-", "").toLowerCase(), bytes));
    }

    async importKey(format, keyData, algorithm, extractable, keyUsages) {
      if (!["raw", "spki", "pkcs8", "jwk"].includes(format)) throw new TypeError(`crypto.subtle.importKey: invalid format '${format}'`);
      const name = normalizeName(algorithm, "importKey");
      const spec = keySpec(name, algorithm, "importKey");
      const usages = parseUsages(keyUsages, "importKey");
      let type;
      if (SECRET.includes(name)) type = "secret";
      else if (format === "jwk") type = keyData?.d === undefined ? "public" : "private";
      else type = format === "pkcs8" ? "private" : "public";
      if (name === "HKDF") {
        if (format !== "raw") throw domError("NotSupportedError", "HKDF keys import from 'raw' only");
        if (extractable) throw domError("SyntaxError", "HKDF keys cannot be extractable");
      }
      if (SECRET.includes(name) && (format === "spki" || format === "pkcs8")) {
        throw domError("NotSupportedError", `${name} keys do not have the '${format}' format`);
      }
      if (format === "raw" && (RSA.includes(name))) throw domError("NotSupportedError", "RSA keys do not have the 'raw' format");
      checkUsages(usages, name, type);
      let info;
      if (format === "jwk") {
        const members = checkJwk(keyData, name, spec, usages, extractable);
        info = host(() => ops.op_nextrs_key_import_jwk(spec, members));
      } else {
        const bytes = copy(keyData, "keyData");
        if (name === "HMAC" && algorithm.length !== undefined && algorithm.length !== bytes.byteLength * 8) {
          throw new NextRSUnsupportedError("crypto.subtle", "importKey", "an HMAC length other than the key data's length");
        }
        info = host(() => ops.op_nextrs_key_import(format, spec, bytes));
      }
      if (name === "HMAC" && format === "jwk" && algorithm.length !== undefined && algorithm.length !== info.length) {
        ops.op_nextrs_key_release(info.handle);
        throw new NextRSUnsupportedError("crypto.subtle", "importKey", "an HMAC length other than the key data's length");
      }
      return makeKey(name, spec, info, extractable, usages);
    }

    async generateKey(algorithm, extractable, keyUsages) {
      const name = normalizeName(algorithm, "generateKey");
      const spec = keySpec(name, algorithm, "generateKey");
      const usages = parseUsages(keyUsages, "generateKey");
      if (RSA.includes(name)) throw new NextRSUnsupportedError("crypto.subtle", "generateKey", `${name} keys (NextRS imports RSA keys; it does not generate them)`);
      if (name === "AES-GCM" || name === "AES-CBC") {
        if (![128, 192, 256].includes(algorithm.length)) throw domError("OperationError", "AES key length must be 128, 192 or 256 bits");
        spec.length = algorithm.length;
      }
      if (name === "HMAC") {
        const requested = algorithm.length ?? 0;
        if (!Number.isInteger(requested) || requested < 0) throw new TypeError("crypto.subtle.generateKey: invalid HMAC length");
        if (algorithm.length === 0) throw domError("OperationError", "HMAC key length must not be 0");
        spec.length = requested;
      }
      if (SECRET.includes(name)) {
        checkUsages(usages, name, "secret");
        const [info] = host(() => ops.op_nextrs_key_generate(spec));
        return makeKey(name, spec, info, extractable, usages);
      }
      const publicUsages = usages.filter((u) => allowedUsages(name, "public").includes(u));
      const privateUsages = usages.filter((u) => allowedUsages(name, "private").includes(u));
      for (const usage of usages) {
        if (!publicUsages.includes(usage) && !privateUsages.includes(usage)) throw domError("SyntaxError", `'${usage}' is not a valid usage for a ${name} key pair`);
      }
      if (privateUsages.length === 0) throw domError("SyntaxError", "the private key needs at least one usage");
      const [publicInfo, privateInfo] = host(() => ops.op_nextrs_key_generate(spec));
      return {
        publicKey: makeKey(name, spec, publicInfo, true, publicUsages),
        privateKey: makeKey(name, spec, privateInfo, extractable, privateUsages),
      };
    }

    async exportKey(format, key) {
      if (!["raw", "spki", "pkcs8", "jwk"].includes(format)) throw new TypeError(`crypto.subtle.exportKey: invalid format '${format}'`);
      if (!(key instanceof CryptoKey) || key[kHandle] === undefined) throw new TypeError("crypto.subtle.exportKey: key is not a CryptoKey");
      const name = key.algorithm.name;
      // Web Crypto checks that the algorithm exports before extractability.
      if (name === "HKDF") throw domError("NotSupportedError", "HKDF keys cannot be exported");
      if (!key.extractable) throw domError("InvalidAccessError", "the key is not extractable");
      if (format !== "jwk") return arrayBuffer(host(() => ops.op_nextrs_key_export(key[kHandle], format)));
      const jwk = host(() => ops.op_nextrs_key_export_jwk(key[kHandle]));
      const spec = { hash: key.algorithm.hash?.name, namedCurve: key.algorithm.namedCurve };
      const alg = jwkAlg(name, spec, key.algorithm.length).export;
      const out = { ...jwk };
      if (alg) out.alg = alg;
      out.key_ops = [...key.usages];
      out.ext = key.extractable;
      return out;
    }

    async encrypt(algorithm, key, data) {
      return aes(true, algorithm, key, data);
    }

    async decrypt(algorithm, key, data) {
      return aes(false, algorithm, key, data);
    }

    async sign(algorithm, key, data) {
      const name = normalizeName(algorithm, "sign");
      const spec = signSpec(name, algorithm, "sign");
      const handle = useKey(key, name, "sign", "sign");
      const bytes = copy(data, "data");
      return arrayBuffer(host(() => ops.op_nextrs_sign(handle, spec, bytes)));
    }

    async verify(algorithm, key, signature, data) {
      const name = normalizeName(algorithm, "verify");
      const spec = signSpec(name, algorithm, "verify");
      const handle = useKey(key, name, "verify", "verify");
      const sig = copy(signature, "signature");
      const bytes = copy(data, "data");
      return host(() => ops.op_nextrs_verify(handle, spec, sig, bytes));
    }

    async deriveBits(algorithm, baseKey, length) {
      const name = normalizeName(algorithm, "deriveBits");
      const hash = normalizeHash(algorithm.hash, "deriveBits");
      if (algorithm.salt === undefined || algorithm.info === undefined) throw new TypeError("crypto.subtle.deriveBits: HKDF needs a salt and an info");
      const salt = copy(algorithm.salt, "salt");
      const info = copy(algorithm.info, "info");
      const handle = useKey(baseKey, name, "deriveBits", "deriveBits");
      if (length === null || length === undefined) throw domError("OperationError", "HKDF needs a length");
      const bits = Number(length);
      if (!Number.isInteger(bits) || bits < 0 || bits > 0xffffffff) throw new TypeError("crypto.subtle.deriveBits: invalid length");
      return arrayBuffer(host(() => ops.op_nextrs_hkdf(handle, hash, salt, info, bits)));
    }

    get [Symbol.toStringTag]() {
      return "SubtleCrypto";
    }
  }
  for (const name of ["deriveKey", "wrapKey", "unwrapKey"]) {
    Object.defineProperty(SubtleCrypto.prototype, name, {
      value: async function () {
        throw new NextRSUnsupportedError("crypto.subtle", name);
      },
      writable: true,
      configurable: true,
    });
  }
  const subtle = Object.create(SubtleCrypto.prototype);

  // --- Crypto -------------------------------------------------------------------------
  function getRandomValues(array) {
    if (!ArrayBuffer.isView(array) || array instanceof Float32Array || array instanceof Float64Array || array instanceof DataView) {
      throw new DOMException("The data argument must be an integer-type TypedArray", "TypeMismatchError");
    }
    if (array.byteLength > 65536) throw new DOMException("The ArrayBufferView's byte length exceeds 65536", "QuotaExceededError");
    ops.op_nextrs_random_fill(new Uint8Array(array.buffer, array.byteOffset, array.byteLength));
    return array;
  }

  function randomUUID() {
    const bytes = new Uint8Array(16);
    ops.op_nextrs_random_fill(bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  }

  class Crypto {
    constructor() {
      throw new TypeError("Illegal constructor");
    }
    get subtle() { return subtle; }
    getRandomValues(array) { return getRandomValues(array); }
    randomUUID() { return randomUUID(); }
    get [Symbol.toStringTag]() { return "Crypto"; }
  }
  const webcrypto = Object.create(Crypto.prototype);
  Object.defineProperty(globalThis, "crypto", { value: webcrypto, writable: true, configurable: true });
  for (const [name, value] of Object.entries({ Crypto, CryptoKey, SubtleCrypto })) {
    Object.defineProperty(globalThis, name, { value, writable: true, enumerable: false, configurable: true });
  }

  internal.webcrypto = { webcrypto, subtle, getRandomValues, randomUUID };
})(globalThis);
