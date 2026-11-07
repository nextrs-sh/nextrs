// NextRS bootstrap, part 3: the Node.js built-ins Next.js route handlers
// use, and a CommonJS loader.
//
// Sources, in order of preference:
//   * vendored npm implementations (node-polyfills.js): buffer, events,
//     readable-stream (Node's own streams, published by the Node.js
//     streams WG), string_decoder, path-browserify (Node's path.posix);
//   * Web APIs from deno_web, re-exported under their Node module names;
//   * NextRS-owned code below, kept to what the tested routes need.
//
// Rules: unsupported functionality throws NextRSUnsupportedError (or a
// Node-style error such as EROFS for writes); missing members of
// implemented modules are reported as diagnostics. Nothing fabricates a
// successful result.
"use strict";

((globalThis) => {
  const internal = globalThis.__nextrsInternal;
  const { core, ops, boot, unsupported, NextRSUnsupportedError } = internal;
  const vendor = globalThis.__nextrsVendor;
  delete globalThis.__nextrsVendor;

  const { Buffer } = vendor.buffer;
  const EventEmitter = vendor.events;
  const path = vendor.path;
  const stream = vendor.stream;

  // process becomes an EventEmitter, as in Node.
  Object.setPrototypeOf(process, EventEmitter.prototype);
  EventEmitter.call(process);
  Object.defineProperty(globalThis, "Buffer", { value: Buffer, writable: true, configurable: true });

  // --- Buffer: base64url (Node >= 15) is missing from buffer@6 ---------
  {
    const toBase64 = (s) => s.replace(/-/g, "+").replace(/_/g, "/");
    const originalFrom = Buffer.from;
    Buffer.from = function from(value, encodingOrOffset, length) {
      if (typeof value === "string" && encodingOrOffset === "base64url") {
        return originalFrom.call(Buffer, toBase64(value), "base64");
      }
      return originalFrom.call(Buffer, value, encodingOrOffset, length);
    };
    const originalToString = Buffer.prototype.toString;
    Buffer.prototype.toString = function toString(encoding, start, end) {
      if (encoding === "base64url") {
        return originalToString
          .call(this, "base64", start, end)
          .replace(/\+/g, "-")
          .replace(/\//g, "_")
          .replace(/=+$/, "");
      }
      return originalToString.call(this, encoding, start, end);
    };
    const originalByteLength = Buffer.byteLength;
    Buffer.byteLength = function byteLength(value, encoding) {
      if (typeof value === "string" && encoding === "base64url") {
        return originalByteLength.call(Buffer, toBase64(value), "base64");
      }
      return originalByteLength.call(Buffer, value, encoding);
    };
    const originalIsEncoding = Buffer.isEncoding;
    Buffer.isEncoding = (encoding) => encoding === "base64url" || originalIsEncoding(encoding);

    // Node's per-encoding slice methods (undocumented, but busboy, which
    // Next.js uses to parse multipart Server Action bodies, calls them).
    // Node defaults start to 0 and end to the length, and clamps them.
    for (const encoding of ["latin1", "ascii", "utf8", "hex", "base64", "ucs2", "utf16le"]) {
      const name = `${encoding}Slice`;
      if (typeof Buffer.prototype[name] === "function") continue;
      Object.defineProperty(Buffer.prototype, name, {
        value: function slice(start = 0, end = this.length) {
          return originalToString.call(this, encoding, start, end);
        },
        writable: true,
        configurable: true,
      });
    }

    // Node accepts any Uint8Array as the search value; buffer@6 only Buffers.
    for (const name of ["indexOf", "lastIndexOf", "includes"]) {
      const original = Buffer.prototype[name];
      Buffer.prototype[name] = function search(value, byteOffset, encoding) {
        if (value instanceof Uint8Array && !Buffer.isBuffer(value)) {
          value = Buffer.from(value.buffer, value.byteOffset, value.byteLength);
        }
        return original.call(this, value, byteOffset, encoding);
      };
    }
  }

  const fatalUtf8 = new TextDecoder("utf-8", { fatal: true });
  function isUtf8(input) {
    try {
      fatalUtf8.decode(input);
      return true;
    } catch {
      return false;
    }
  }
  const isAscii = (input) => new Uint8Array(input.buffer ?? input, input.byteOffset ?? 0, input.byteLength).every((b) => b < 0x80);

  // --- diagnostics for missing members --------------------------------
  const reported = new Set();
  const IGNORED_MEMBERS = new Set(["__esModule", "default", "then", "toJSON", "constructor", "inspect"]);

  const traced = new Set();
  function trace(member) {
    if (boot.traceNodeApi && !traced.has(member)) {
      traced.add(member);
      ops.op_nextrs_diagnostic("node-api-access", member);
    }
  }

  function strict(name, target) {
    return new Proxy(target, {
      get(object, key, receiver) {
        if (typeof key !== "symbol") trace(`node:${name}.${key}`);
        if (typeof key === "symbol" || key in object) return Reflect.get(object, key, receiver);
        const member = `node:${name}.${key}`;
        if (!IGNORED_MEMBERS.has(key) && !reported.has(member)) {
          reported.add(member);
          ops.op_nextrs_diagnostic("missing-member", `${member} is not implemented (read as undefined)`);
        }
        return undefined;
      },
    });
  }

  // Pass-through modules are only wrapped while tracing, so normal runs
  // see the vendored objects themselves.
  function observed(name, target) {
    if (!boot.traceNodeApi) return target;
    return new Proxy(target, {
      get(object, key, receiver) {
        if (typeof key !== "symbol") trace(`node:${name}.${key}`);
        return Reflect.get(object, key, receiver);
      },
    });
  }

  // Modules NextRS does not implement at all: requiring them works (Next
  // requires some eagerly), every use throws.
  function unsupportedModule(name) {
    const fail = (member) =>
      function unsupportedMember() {
        return unsupported(`node:${name}`, member);
      };
    return new Proxy(Object.create(null), {
      get(_target, key) {
        if (typeof key === "symbol" || IGNORED_MEMBERS.has(key)) return undefined;
        return fail(key);
      },
      has() {
        return false;
      },
    });
  }

  // --- errors -----------------------------------------------------------
  const ERRNO = { ENOENT: -2, EACCES: -13, EISDIR: -21, ENOTDIR: -20, EROFS: -30, EIO: -5 };

  function nodeError(message, extra) {
    const error = new Error(message);
    Object.assign(error, extra);
    Error.captureStackTrace?.(error, nodeError);
    return error;
  }

  // Op errors arrive as "CODE: reason, syscall 'path'".
  function translateFsError(error, syscall, file) {
    const message = String(error?.message ?? error);
    const code = /^([A-Z]+):/.exec(message)?.[1];
    if (!code || !(code in ERRNO)) return error;
    return nodeError(message, { code, errno: ERRNO[code], syscall, path: file });
  }

  function readOnly(syscall, file) {
    const error = nodeError(`EROFS: read-only file system (NextRS grants no write access), ${syscall} '${file}'`, {
      code: "EROFS",
      errno: ERRNO.EROFS,
      syscall,
      path: file,
    });
    ops.op_nextrs_diagnostic("fs-write-denied", error.message);
    return error;
  }

  // --- fs (read-only) -------------------------------------------------
  function toPath(file) {
    if (file instanceof URL) {
      if (file.protocol !== "file:") throw new TypeError("The URL must be of scheme file");
      return decodeURIComponent(file.pathname);
    }
    if (file instanceof Uint8Array) return new TextDecoder().decode(file);
    if (typeof file !== "string") {
      const error = new TypeError(`The "path" argument must be of type string. Received ${typeof file}`);
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    return path.resolve(boot.projectDir, file);
  }

  function readBytes(file) {
    try {
      return ops.op_nextrs_fs_read(file);
    } catch (error) {
      throw translateFsError(error, "open", file);
    }
  }

  function statOrNull(file) {
    try {
      return ops.op_nextrs_fs_stat(file);
    } catch (error) {
      throw translateFsError(error, "stat", file);
    }
  }

  class Stats {
    constructor(info) {
      this.size = info.size;
      this.mode = info.mode;
      this.mtimeMs = info.mtimeMs;
      this.atimeMs = info.mtimeMs;
      this.ctimeMs = info.mtimeMs;
      this.birthtimeMs = info.mtimeMs;
      this.mtime = new Date(info.mtimeMs);
      this.atime = this.mtime;
      this.ctime = this.mtime;
      this.birthtime = this.mtime;
      this.nlink = 1;
      this.blksize = 4096;
      this.blocks = Math.ceil(info.size / 512);
      Object.defineProperty(this, "kind", { value: info.kind });
    }
    isFile() { return this.kind === "file"; }
    isDirectory() { return this.kind === "dir"; }
    // The sandbox resolves symlinks before answering.
    isSymbolicLink() { return false; }
    isFIFO() { return false; }
    isSocket() { return false; }
    isBlockDevice() { return false; }
    isCharacterDevice() { return false; }
  }

  class Dirent {
    constructor(name, parentPath, isDir) {
      this.name = name;
      this.parentPath = parentPath;
      this.path = parentPath;
      Object.defineProperty(this, "dir", { value: isDir });
    }
    isFile() { return !this.dir; }
    isDirectory() { return this.dir; }
    isSymbolicLink() { return false; }
    isFIFO() { return false; }
    isSocket() { return false; }
    isBlockDevice() { return false; }
    isCharacterDevice() { return false; }
  }

  const encodingOf = (options) => (typeof options === "string" ? options : options?.encoding ?? null);

  const fsSync = {
    readFileSync(file, options) {
      if (typeof file === "number") return unsupported("node:fs", "readFileSync", "file descriptors");
      const bytes = Buffer.from(readBytes(toPath(file)));
      const encoding = encodingOf(options);
      return encoding ? bytes.toString(encoding) : bytes;
    },
    existsSync(file) {
      try {
        return statOrNull(toPath(file)) !== null;
      } catch {
        return false;
      }
    },
    statSync(file, options) {
      const resolved = toPath(file);
      const info = statOrNull(resolved);
      if (info === null) {
        if (options?.throwIfNoEntry === false) return undefined;
        throw nodeError(`ENOENT: no such file or directory, stat '${resolved}'`, {
          code: "ENOENT",
          errno: ERRNO.ENOENT,
          syscall: "stat",
          path: resolved,
        });
      }
      return new Stats(info);
    },
    readdirSync(dir, options) {
      const resolved = toPath(dir);
      let entries;
      try {
        entries = ops.op_nextrs_fs_readdir(resolved);
      } catch (error) {
        throw translateFsError(error, "scandir", resolved);
      }
      if (options?.withFileTypes) return entries.map(([name, isDir]) => new Dirent(name, resolved, isDir));
      return entries.map(([name]) => name);
    },
    realpathSync(file) {
      const resolved = toPath(file);
      try {
        return ops.op_nextrs_fs_realpath(resolved);
      } catch (error) {
        throw translateFsError(error, "realpath", resolved);
      }
    },
    accessSync(file, mode = 0) {
      const resolved = toPath(file);
      if (mode & 2) throw readOnly("access", resolved);
      if (fsSync.statSync(resolved, { throwIfNoEntry: true }) === undefined) return;
    },
  };
  fsSync.lstatSync = fsSync.statSync;
  fsSync.realpathSync.native = fsSync.realpathSync;

  const WRITE_OPERATIONS = [
    "writeFile", "appendFile", "mkdir", "mkdtemp", "rm", "rmdir", "unlink", "rename", "copyFile",
    "cp", "symlink", "link", "chmod", "chown", "utimes", "truncate", "open",
  ];

  const fs = { ...fsSync, constants: { F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1 } };
  for (const name of WRITE_OPERATIONS) {
    fs[`${name}Sync`] = (file) => {
      throw readOnly(name, String(file));
    };
    fs[name] = (file, ...rest) => {
      const callback = rest.findLast((arg) => typeof arg === "function");
      const error = readOnly(name, String(file));
      if (callback) process.nextTick(callback, error);
      else throw error;
    };
  }
  fs.createWriteStream = (file) => {
    throw readOnly("open", String(file));
  };
  fs.createReadStream = (file) => unsupported("node:fs", "createReadStream", String(file));
  fs.watch = () => unsupported("node:fs", "watch");
  fs.watchFile = () => unsupported("node:fs", "watchFile");

  // Callback forms of the read operations.
  for (const name of ["readFile", "stat", "lstat", "readdir", "realpath", "access"]) {
    const sync = fsSync[`${name}Sync`];
    fs[name] = (...args) => {
      const callback = args.pop();
      if (typeof callback !== "function") throw new TypeError("The \"cb\" argument must be of type function");
      let result;
      let failure = null;
      try {
        result = sync(...args);
      } catch (error) {
        failure = error;
      }
      process.nextTick(() => (failure ? callback(failure) : callback(null, result)));
    };
  }
  fs.exists = (file, callback) => process.nextTick(callback, fsSync.existsSync(file));

  const fsPromises = { constants: fs.constants };
  for (const name of ["readFile", "stat", "lstat", "readdir", "realpath", "access"]) {
    const sync = fsSync[`${name}Sync`];
    fsPromises[name] = async (...args) => sync(...args);
  }
  for (const name of WRITE_OPERATIONS) {
    fsPromises[name] = async (file) => {
      throw readOnly(name, String(file));
    };
  }
  fs.promises = fsPromises;

  // --- crypto -----------------------------------------------------------
  const HASHES = { md5: 64, sha1: 64, sha256: 64, sha384: 128, sha512: 128 };

  function normalizeAlgorithm(algorithm) {
    const name = String(algorithm).toLowerCase().replace(/^rsa-/, "").replace("-", "");
    if (!(name in HASHES)) return unsupported("node:crypto", "createHash", `digest algorithm '${algorithm}'`);
    return name;
  }

  const toBytes = (data, encoding) =>
    typeof data === "string" ? Buffer.from(data, encoding || "utf8") : Buffer.from(data.buffer ?? data, data.byteOffset ?? 0, data.byteLength);

  const digestBytes = (algorithm, bytes) => Buffer.from(ops.op_nextrs_hash(algorithm, bytes));

  class Hash {
    #algorithm;
    #chunks = [];
    #done = false;

    constructor(algorithm) {
      this.#algorithm = normalizeAlgorithm(algorithm);
    }

    update(data, encoding) {
      if (this.#done) throw nodeError("Digest already called", { code: "ERR_CRYPTO_HASH_FINALIZED" });
      this.#chunks.push(toBytes(data, encoding));
      return this;
    }

    digest(encoding) {
      if (this.#done) throw nodeError("Digest already called", { code: "ERR_CRYPTO_HASH_FINALIZED" });
      this.#done = true;
      const out = digestBytes(this.#algorithm, Buffer.concat(this.#chunks));
      return encoding && encoding !== "buffer" ? out.toString(encoding) : out;
    }

    copy() {
      const copy = new Hash(this.#algorithm);
      for (const chunk of this.#chunks) copy.update(chunk);
      return copy;
    }
  }

  // RFC 2104 over the Rust digests.
  class Hmac {
    #algorithm;
    #key;
    #chunks = [];

    constructor(algorithm, key) {
      this.#algorithm = normalizeAlgorithm(algorithm);
      const blockSize = HASHES[this.#algorithm];
      let keyBytes = toBytes(typeof key === "string" ? key : key?.export?.() ?? key);
      if (keyBytes.length > blockSize) keyBytes = digestBytes(this.#algorithm, keyBytes);
      this.#key = Buffer.alloc(blockSize);
      keyBytes.copy(this.#key);
    }

    update(data, encoding) {
      this.#chunks.push(toBytes(data, encoding));
      return this;
    }

    digest(encoding) {
      const pad = (byte) => Buffer.from(this.#key.map((b) => b ^ byte));
      const inner = digestBytes(this.#algorithm, Buffer.concat([pad(0x36), ...this.#chunks]));
      const out = digestBytes(this.#algorithm, Buffer.concat([pad(0x5c), inner]));
      return encoding && encoding !== "buffer" ? out.toString(encoding) : out;
    }
  }

  // Web Crypto is 02_webcrypto.js; node:crypto re-exports it.
  const { webcrypto, subtle, getRandomValues, randomUUID } = internal.webcrypto;

  function randomBytes(size, callback) {
    const bytes = Buffer.alloc(size);
    ops.op_nextrs_random_fill(bytes);
    if (typeof callback === "function") {
      process.nextTick(callback, null, bytes);
      return undefined;
    }
    return bytes;
  }

  const crypto = {
    createHash: (algorithm) => new Hash(algorithm),
    createHmac: (algorithm, key) => new Hmac(algorithm, key),
    randomBytes,
    randomUUID,
    getRandomValues,
    randomFillSync(buffer, offset = 0, size) {
      const view = new Uint8Array(buffer.buffer ?? buffer, (buffer.byteOffset ?? 0) + offset, size ?? buffer.byteLength - offset);
      ops.op_nextrs_random_fill(view);
      return buffer;
    },
    randomInt(min, max, callback) {
      if (max === undefined || typeof max === "function") {
        callback = max;
        max = min;
        min = 0;
      }
      const range = max - min;
      if (!Number.isSafeInteger(range) || range <= 0 || range > 2 ** 48) throw new RangeError("Invalid randomInt range");
      const limit = 2 ** 48 - (2 ** 48 % range);
      let value;
      do {
        value = randomBytes(6).readUIntBE(0, 6);
      } while (value >= limit);
      const result = min + (value % range);
      if (typeof callback === "function") return process.nextTick(callback, null, result);
      return result;
    },
    timingSafeEqual(a, b) {
      if (a.byteLength !== b.byteLength) throw new RangeError("Input buffers must have the same byte length");
      const x = new Uint8Array(a.buffer ?? a, a.byteOffset ?? 0, a.byteLength);
      const y = new Uint8Array(b.buffer ?? b, b.byteOffset ?? 0, b.byteLength);
      let diff = 0;
      for (let i = 0; i < x.length; i++) diff |= x[i] ^ y[i];
      return diff === 0;
    },
    getHashes: () => Object.keys(HASHES),
    webcrypto,
    subtle,
    constants: {},
  };
  // HKDF (RFC 5869), as node:crypto's hkdf and hkdfSync: libraries pick
  // it over Web Crypto when it exists (Auth.js's @panva/hkdf does).
  const NODE_HKDF_HASHES = { sha1: "SHA-1", sha256: "SHA-256", sha384: "SHA-384", sha512: "SHA-512" };
  const HKDF_LENGTHS = { sha1: 20, sha256: 32, sha384: 48, sha512: 64 };
  function hkdfBytes(value, name) {
    if (typeof value === "string") return Buffer.from(value, "utf8");
    if (ArrayBuffer.isView(value)) return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (value && typeof value === "object" && value.type === "secret") return unsupported("node:crypto", "hkdf", "a KeyObject as the key");
    const error = new TypeError(`The "${name}" argument must be of type string or an instance of ArrayBuffer, Buffer, TypedArray or DataView.`);
    error.code = "ERR_INVALID_ARG_TYPE";
    throw error;
  }
  function hkdfSync(digest, ikm, salt, info, keylen) {
    const hash = String(digest).toLowerCase();
    if (!NODE_HKDF_HASHES[hash]) {
      const error = new TypeError(`Invalid digest: ${digest}`);
      error.code = "ERR_CRYPTO_INVALID_DIGEST";
      throw error;
    }
    const key = hkdfBytes(ikm, "ikm");
    const saltBytes = hkdfBytes(salt, "salt");
    const infoBytes = hkdfBytes(info, "info");
    if (infoBytes.byteLength > 1024) {
      const error = new RangeError('The value of "info" is out of range. It must be <= 1024.');
      error.code = "ERR_OUT_OF_RANGE";
      throw error;
    }
    if (!Number.isInteger(keylen) || keylen < 0 || keylen > 255 * HKDF_LENGTHS[hash]) {
      const error = new RangeError(`The value of "keylen" is out of range.`);
      error.code = keylen > 255 * HKDF_LENGTHS[hash] ? "ERR_CRYPTO_INVALID_KEYLEN" : "ERR_OUT_OF_RANGE";
      throw error;
    }
    // Node (OpenSSL) refuses to derive nothing.
    if (keylen === 0) throw new Error("Deriving bits failed");
    const out = ops.op_nextrs_hkdf_bytes(NODE_HKDF_HASHES[hash], key, saltBytes, infoBytes, keylen);
    return out.buffer.slice(out.byteOffset, out.byteOffset + out.byteLength);
  }
  crypto.hkdfSync = hkdfSync;
  crypto.hkdf = function hkdf(digest, ikm, salt, info, keylen, callback) {
    if (typeof callback !== "function") {
      const error = new TypeError('The "callback" argument must be of type function');
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }
    // Argument errors throw synchronously, as in Node; the result is async.
    const out = hkdfSync(digest, ikm, salt, info, keylen);
    process.nextTick(callback, null, out);
  };
  for (const name of ["createCipheriv", "createDecipheriv", "createSign", "createVerify", "createECDH", "createDiffieHellman", "generateKeyPair", "generateKeyPairSync", "createPublicKey", "createPrivateKey", "createSecretKey", "pbkdf2", "pbkdf2Sync", "scrypt", "scryptSync", "publicEncrypt", "privateDecrypt", "sign", "verify"]) {
    crypto[name] = () => unsupported("node:crypto", name);
  }

  // --- util -------------------------------------------------------------
  const kCustomPromisify = Symbol.for("nodejs.util.promisify.custom");

  function promisify(original) {
    if (typeof original !== "function") throw new TypeError('The "original" argument must be of type function');
    if (original[kCustomPromisify]) return original[kCustomPromisify];
    function fn(...args) {
      return new Promise((resolve, reject) => {
        original.call(this, ...args, (error, ...values) => {
          if (error) reject(error);
          else resolve(values.length > 1 ? values : values[0]);
        });
      });
    }
    Object.setPrototypeOf(fn, Object.getPrototypeOf(original));
    Object.defineProperty(fn, kCustomPromisify, { value: fn });
    return Object.defineProperties(fn, Object.getOwnPropertyDescriptors(original));
  }
  promisify.custom = kCustomPromisify;

  const types = {
    isPromise: core.isPromise,
    isRegExp: core.isRegExp,
    isDate: core.isDate,
    isNativeError: core.isNativeError,
    isAsyncFunction: core.isAsyncFunction,
    isGeneratorFunction: core.isGeneratorFunction,
    isGeneratorObject: core.isGeneratorObject,
    isMap: core.isMap,
    isSet: core.isSet,
    isWeakMap: core.isWeakMap,
    isWeakSet: core.isWeakSet,
    isMapIterator: core.isMapIterator,
    isSetIterator: core.isSetIterator,
    isProxy: core.isProxy,
    isArrayBuffer: core.isArrayBuffer,
    isAnyArrayBuffer: core.isAnyArrayBuffer,
    isSharedArrayBuffer: core.isSharedArrayBuffer,
    isArrayBufferView: core.isArrayBufferView,
    isTypedArray: core.isTypedArray,
    isDataView: core.isDataView,
    isBoxedPrimitive: core.isBoxedPrimitive,
    isModuleNamespaceObject: core.isModuleNamespaceObject,
    isUint8Array: (value) => value instanceof Uint8Array,
    isUint8ClampedArray: (value) => value instanceof Uint8ClampedArray,
    isUint16Array: (value) => value instanceof Uint16Array,
    isUint32Array: (value) => value instanceof Uint32Array,
    isInt8Array: (value) => value instanceof Int8Array,
    isInt16Array: (value) => value instanceof Int16Array,
    isInt32Array: (value) => value instanceof Int32Array,
    isFloat32Array: (value) => value instanceof Float32Array,
    isFloat64Array: (value) => value instanceof Float64Array,
    isBigInt64Array: (value) => value instanceof BigInt64Array,
    isBigUint64Array: (value) => value instanceof BigUint64Array,
  };

  const debugEnabled = new Set(String(boot.env.NODE_DEBUG ?? "").toUpperCase().split(/[\s,]+/).filter(Boolean));

  const util = {
    promisify,
    callbackify(original) {
      return function callbackified(...args) {
        const callback = args.pop();
        original.apply(this, args).then(
          (value) => process.nextTick(callback, null, value),
          (error) => process.nextTick(callback, error),
        );
      };
    },
    inherits(ctor, superCtor) {
      Object.defineProperty(ctor, "super_", { value: superCtor, writable: true, configurable: true });
      Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
    },
    deprecate(fn, message, code) {
      let warned = false;
      return function deprecated(...args) {
        if (!warned) {
          warned = true;
          process.emitWarning(message, "DeprecationWarning", code);
        }
        return new.target ? Reflect.construct(fn, args, new.target) : fn.apply(this, args);
      };
    },
    // Logs only for sections listed in NODE_DEBUG, as in Node.
    debuglog(section) {
      const enabled = debugEnabled.has(String(section).toUpperCase());
      const log = enabled
        ? (...args) => core.print(`${String(section).toUpperCase()} ${process.pid}: ${internal.format(...args)}\n`, true)
        : () => {};
      log.enabled = enabled;
      return log;
    },
    inspect: internal.inspect,
    format: internal.format,
    types,
    isArray: Array.isArray,
    isDeepStrictEqual: () => unsupported("node:util", "isDeepStrictEqual"),
    TextEncoder,
    TextDecoder,
    stripVTControlCharacters: (text) => String(text).replace(/\u001b\[[0-9;]*[A-Za-z]/g, ""),
  };
  util.inspect.custom = Symbol.for("nodejs.util.inspect.custom");
  util.debug = util.debuglog;

  // --- events -----------------------------------------------------------
  const events = EventEmitter;
  events.setMaxListeners ??= function setMaxListeners(n, ...targets) {
    // EventTargets (AbortSignal) have no leak warning to configure here.
    for (const target of targets) if (target instanceof EventEmitter) target.setMaxListeners(n);
    if (targets.length === 0) EventEmitter.defaultMaxListeners = n;
  };
  events.addAbortListener ??= function addAbortListener(signal, listener) {
    if (signal.aborted) {
      queueMicrotask(() => listener());
    } else {
      signal.addEventListener("abort", listener, { once: true });
    }
    return { [Symbol.dispose]: () => signal.removeEventListener("abort", listener) };
  };
  events.getEventListeners ??= (target, name) =>
    target instanceof EventEmitter ? target.listeners(name) : unsupported("node:events", "getEventListeners", "EventTarget");

  // --- stream -----------------------------------------------------------
  // readable-stream's browser build declares these but lacks the adapters
  // they call. Same semantics as Node's lib/internal/webstreams/adapters.js.
  const { Readable, Writable } = stream;
  Readable.toWeb = function toWeb(readable, options = {}) {
    const objectMode = readable.readableObjectMode;
    return new ReadableStream(
      {
        start(controller) {
          readable.on("data", (chunk) => {
            controller.enqueue(!objectMode && Buffer.isBuffer(chunk) ? new Uint8Array(chunk) : chunk);
            if (controller.desiredSize <= 0) readable.pause();
          });
          readable.once("end", () => controller.close());
          readable.once("error", (error) => controller.error(error));
          readable.pause();
        },
        pull() {
          readable.resume();
        },
        cancel(reason) {
          readable.destroy(reason);
        },
      },
      options.strategy ?? (objectMode ? undefined : new CountQueuingStrategy({ highWaterMark: readable.readableHighWaterMark ?? 1 })),
    );
  };
  Readable.fromWeb = function fromWeb(readableStream, options = {}) {
    const reader = readableStream.getReader();
    let closed = false;
    return new Readable({
      objectMode: options.objectMode,
      highWaterMark: options.highWaterMark,
      encoding: options.encoding,
      signal: options.signal,
      read() {
        reader.read().then(
          ({ done, value }) => {
            if (done) {
              closed = true;
              this.push(null);
            } else {
              this.push(value);
            }
          },
          (error) => this.destroy(error),
        );
      },
      destroy(error, callback) {
        if (closed) return callback(error);
        reader.cancel(error).then(
          () => callback(error),
          (cancelError) => callback(cancelError),
        );
      },
    });
  };
  Writable.toWeb = () => unsupported("node:stream", "Writable.toWeb");
  Writable.fromWeb = () => unsupported("node:stream", "Writable.fromWeb");
  const streamWeb = {
    ReadableStream,
    ReadableStreamDefaultReader,
    ReadableStreamBYOBReader,
    ReadableByteStreamController,
    ReadableStreamDefaultController,
    WritableStream,
    WritableStreamDefaultWriter,
    TransformStream,
    ByteLengthQueuingStrategy,
    CountQueuingStrategy,
    TextEncoderStream,
    TextDecoderStream,
  };

  // --- timers -----------------------------------------------------------
  function abortError(signal) {
    return new DOMException(signal?.reason?.message ?? "The operation was aborted", "AbortError");
  }

  const timersPromises = {
    setTimeout(delay, value, options = {}) {
      return new Promise((resolve, reject) => {
        const signal = options.signal;
        if (signal?.aborted) return reject(abortError(signal));
        const timer = setTimeout(() => {
          signal?.removeEventListener("abort", onAbort);
          resolve(value);
        }, delay);
        if (options.ref === false) timer.unref();
        function onAbort() {
          clearTimeout(timer);
          reject(abortError(signal));
        }
        signal?.addEventListener("abort", onAbort, { once: true });
      });
    },
    setImmediate(value, options = {}) {
      return new Promise((resolve, reject) => {
        const signal = options.signal;
        if (signal?.aborted) return reject(abortError(signal));
        const immediate = setImmediate(() => {
          signal?.removeEventListener("abort", onAbort);
          resolve(value);
        });
        if (options.ref === false) immediate.unref();
        function onAbort() {
          clearImmediate(immediate);
          reject(abortError(signal));
        }
        signal?.addEventListener("abort", onAbort, { once: true });
      });
    },
    setInterval: () => unsupported("node:timers/promises", "setInterval"),
    scheduler: {
      wait: (delay, options) => timersPromises.setTimeout(delay, undefined, options),
      yield: () => timersPromises.setImmediate(),
    },
  };

  Object.defineProperty(setTimeout, kCustomPromisify, { value: timersPromises.setTimeout });
  Object.defineProperty(setImmediate, kCustomPromisify, { value: timersPromises.setImmediate });

  const timers = {
    setTimeout,
    clearTimeout,
    setInterval,
    clearInterval,
    setImmediate,
    clearImmediate,
    promises: timersPromises,
  };

  // --- url --------------------------------------------------------------
  const nodeUrl = {
    URL,
    URLSearchParams,
    fileURLToPath(value) {
      const parsed = typeof value === "string" ? new URL(value) : value;
      if (parsed.protocol !== "file:") {
        throw nodeError("The URL must be of scheme file", { code: "ERR_INVALID_URL_SCHEME" });
      }
      return decodeURIComponent(parsed.pathname);
    },
    pathToFileURL(file) {
      const url = new URL("file://");
      url.pathname = path.resolve(boot.projectDir, file);
      return url;
    },
    format(value) {
      if (value instanceof URL) return value.href;
      return unsupported("node:url", "format", "legacy URL objects");
    },
    parse: () => unsupported("node:url", "parse", "the legacy URL parser"),
    resolve: () => unsupported("node:url", "resolve", "the legacy URL parser"),
  };

  // --- async_hooks --------------------------------------------------------
  const asyncHooks = {
    AsyncLocalStorage: internal.AsyncLocalStorage,
    AsyncResource: internal.AsyncResource,
    createHook: () => unsupported("node:async_hooks", "createHook"),
    executionAsyncId: () => unsupported("node:async_hooks", "executionAsyncId"),
    triggerAsyncId: () => unsupported("node:async_hooks", "triggerAsyncId"),
    executionAsyncResource: () => unsupported("node:async_hooks", "executionAsyncResource"),
  };

  // --- vm ---------------------------------------------------------------
  const contexts = new WeakSet();
  function runInNewContext(code, sandbox = {}, options) {
    const filename = (typeof options === "string" ? options : options?.filename) ?? "evalmachine.<anonymous>";
    const [result, exception] = ops.op_nextrs_vm_run_in_new_context(String(code), filename, sandbox);
    if (exception !== undefined) throw exception;
    return result;
  }
  const vm = {
    runInNewContext,
    createContext(sandbox = {}) {
      contexts.add(sandbox);
      return sandbox;
    },
    runInContext(code, context, options) {
      if (!contexts.has(context)) throw new TypeError('The "contextifiedObject" argument must be an vm.Context');
      return runInNewContext(code, context, options);
    },
    isContext: (value) => contexts.has(value),
    compileFunction(code, params = [], options = {}) {
      const [fn, error] = core.compileFunction(String(code), fileUrl(options.filename ?? "evalmachine.<anonymous>"), undefined, params);
      if (error) throw error.thrown;
      return fn;
    },
    runInThisContext: () => unsupported("node:vm", "runInThisContext"),
    Script: class Script {
      constructor() {
        unsupported("node:vm", "Script");
      }
    },
  };

  // --- worker_threads / inspector: the facts of a main-thread runtime ----
  const workerThreads = {
    isMainThread: true,
    parentPort: null,
    workerData: null,
    threadId: 0,
    resourceLimits: {},
    MessageChannel,
    MessagePort,
    Worker: class Worker {
      constructor() {
        unsupported("node:worker_threads", "Worker");
      }
    },
    receiveMessageOnPort: () => unsupported("node:worker_threads", "receiveMessageOnPort"),
  };
  const inspector = {
    // No inspector is active in this runtime.
    url: () => undefined,
    open: () => unsupported("node:inspector", "open"),
    close: () => unsupported("node:inspector", "close"),
    waitForDebugger: () => unsupported("node:inspector", "waitForDebugger"),
    Session: class Session {
      constructor() {
        unsupported("node:inspector", "Session");
      }
    },
    console,
  };

  // --- module (CommonJS loader) -------------------------------------------
  const decoder = new TextDecoder();
  const fileUrl = (file) => `file://${encodeURI(file)}`;
  const stripBOM = (text) => (text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);
  const readText = (file) => stripBOM(decoder.decode(readBytes(file)));

  // The build output does not change while the runtime runs.
  const statCache = new Map();
  function kind(file) {
    if (!statCache.has(file)) {
      let info = null;
      try {
        info = ops.op_nextrs_fs_stat(file);
      } catch {
        // Outside the read roots: resolution treats it as absent.
      }
      statCache.set(file, info?.kind ?? null);
    }
    return statCache.get(file);
  }

  const packageJsonCache = new Map();
  function readPackageJson(dir) {
    const file = path.join(dir, "package.json");
    if (!packageJsonCache.has(file)) {
      packageJsonCache.set(file, kind(file) === "file" ? JSON.parse(readText(file)) : null);
    }
    return packageJsonCache.get(file);
  }

  const EXTENSIONS = [".js", ".json", ".node", ".cjs"];
  const CONDITIONS = ["require", "node", "node-addons", "default"];

  function tryFile(file) {
    return kind(file) === "file" ? file : null;
  }

  function tryExtensions(base) {
    for (const extension of EXTENSIONS) {
      const file = tryFile(base + extension);
      if (file) return file;
    }
    return null;
  }

  function tryDirectory(dir) {
    if (kind(dir) !== "dir") return null;
    const pkg = readPackageJson(dir);
    if (pkg?.main) {
      const main = path.resolve(dir, pkg.main);
      const found = tryFile(main) ?? tryExtensions(main) ?? tryExtensions(path.join(main, "index"));
      if (found) return found;
    }
    return tryExtensions(path.join(dir, "index"));
  }

  function resolvePath(base) {
    return tryFile(base) ?? tryExtensions(base) ?? tryDirectory(base);
  }

  function resolveConditional(target) {
    if (typeof target === "string" || target === null) return target;
    if (Array.isArray(target)) {
      for (const item of target) {
        const resolved = resolveConditional(item);
        if (resolved) return resolved;
      }
      return null;
    }
    for (const [condition, value] of Object.entries(target)) {
      if (CONDITIONS.includes(condition)) {
        const resolved = resolveConditional(value);
        if (resolved !== undefined && resolved !== null) return resolved;
      }
    }
    return null;
  }

  // package.json "exports" for require(): exact subpaths and "*" patterns.
  function resolveExports(pkgDir, subpath, exports, request) {
    const map =
      typeof exports === "string" || Array.isArray(exports) || !Object.keys(exports).some((k) => k.startsWith("."))
        ? { ".": exports }
        : exports;
    let target = null;
    if (Object.hasOwn(map, subpath)) {
      target = resolveConditional(map[subpath]);
    } else {
      let best = null;
      for (const key of Object.keys(map)) {
        const star = key.indexOf("*");
        if (star === -1) continue;
        const prefix = key.slice(0, star);
        const suffix = key.slice(star + 1);
        if (subpath.startsWith(prefix) && subpath.endsWith(suffix) && subpath.length >= key.length - 1) {
          if (!best || prefix.length > best.prefix.length) best = { key, prefix, suffix };
        }
      }
      if (best) {
        const match = subpath.slice(best.prefix.length, subpath.length - best.suffix.length);
        const resolved = resolveConditional(map[best.key]);
        target = resolved && resolved.replaceAll("*", match);
      }
    }
    if (!target) {
      throw nodeError(`Package subpath '${subpath}' is not defined by "exports" in ${pkgDir}/package.json`, {
        code: "ERR_PACKAGE_PATH_NOT_EXPORTED",
        request,
      });
    }
    return path.resolve(pkgDir, target);
  }

  function splitPackage(request) {
    const parts = request.split("/");
    const nameLength = request.startsWith("@") ? 2 : 1;
    return { name: parts.slice(0, nameLength).join("/"), subpath: `.${parts.length > nameLength ? "/" + parts.slice(nameLength).join("/") : ""}` };
  }

  function nodeModulePaths(from) {
    const out = [];
    for (let dir = path.resolve(from); ; dir = path.dirname(dir)) {
      if (path.basename(dir) !== "node_modules") out.push(path.join(dir, "node_modules"));
      if (dir === "/") break;
    }
    return out;
  }

  const BUILTINS = new Map();
  const builtinCache = new Map();
  const isBuiltin = (request) => BUILTINS.has(String(request).replace(/^node:/, ""));

  function loadBuiltin(request) {
    const name = request.replace(/^node:/, "");
    trace(`require:${name}`);
    if (!builtinCache.has(name)) builtinCache.set(name, BUILTINS.get(name)());
    return builtinCache.get(name);
  }

  class Module {
    constructor(id = "", parent) {
      this.id = id;
      this.path = path.dirname(id);
      this.exports = {};
      this.filename = null;
      this.loaded = false;
      this.children = [];
      this.paths = [];
      this.parent = parent;
      if (parent?.children) parent.children.push(this);
    }

    require(id) {
      if (typeof id !== "string" || id === "") {
        throw nodeError(`The argument 'id' must be a non-empty string. Received ${String(id)}`, { code: "ERR_INVALID_ARG_VALUE" });
      }
      return Module._load(id, this, false);
    }

    load(filename) {
      this.filename = filename;
      this.paths = Module._nodeModulePaths(path.dirname(filename));
      const extension = path.extname(filename);
      const loader = Module._extensions[extension] ?? Module._extensions[".js"];
      loader(this, filename);
      this.loaded = true;
    }

    _compile(content, filename) {
      const [fn, error] = core.compileFunction(content, fileUrl(filename), undefined, [
        "exports",
        "require",
        "module",
        "__filename",
        "__dirname",
      ]);
      if (error) throw error.thrown;
      return fn.call(this.exports, this.exports, makeRequire(this), this, filename, path.dirname(filename));
    }

    static _resolveFilename(request, parent, _isMain, _options) {
      if (isBuiltin(request)) return request;
      const fromDir = parent?.filename ? path.dirname(parent.filename) : boot.projectDir;
      let resolved = null;
      if (request.startsWith("./") || request.startsWith("../") || request === "." || request === ".." || request.startsWith("/")) {
        resolved = resolvePath(path.resolve(fromDir, request));
      } else {
        const { name, subpath } = splitPackage(request);
        for (const dir of parent?.paths?.length ? parent.paths : nodeModulePaths(fromDir)) {
          const pkgDir = path.join(dir, name);
          if (kind(pkgDir) !== "dir") continue;
          const pkg = readPackageJson(pkgDir);
          if (pkg?.exports !== undefined && pkg.exports !== null) {
            resolved = tryFile(resolveExports(pkgDir, subpath, pkg.exports, request));
          } else {
            resolved = resolvePath(path.join(pkgDir, subpath));
          }
          if (resolved) break;
        }
      }
      if (!resolved) {
        throw nodeError(`Cannot find module '${request}'\nRequire stack:\n- ${parent?.filename ?? boot.projectDir}`, {
          code: "MODULE_NOT_FOUND",
          requireStack: parent?.filename ? [parent.filename] : [],
        });
      }
      return resolved;
    }

    static _load(request, parent, isMain) {
      if (request.startsWith("node:") && !isBuiltin(request)) {
        throw nodeError(`No such built-in module: ${request}`, { code: "ERR_UNKNOWN_BUILTIN_MODULE" });
      }
      const filename = Module._resolveFilename(request, parent, isMain);
      if (isBuiltin(filename)) return loadBuiltin(filename);
      const cached = Module._cache[filename];
      if (cached) return cached.exports;
      const module = new Module(filename, parent);
      Module._cache[filename] = module;
      try {
        module.load(filename);
      } catch (error) {
        delete Module._cache[filename];
        throw error;
      }
      return module.exports;
    }

    static createRequire(filename) {
      const file = typeof filename === "string" && filename.startsWith("file:") ? nodeUrl.fileURLToPath(filename) : String(filename);
      const module = new Module(file, null);
      module.filename = file;
      module.paths = Module._nodeModulePaths(path.dirname(file));
      return makeRequire(module);
    }

    static _nodeModulePaths(from) {
      return nodeModulePaths(from);
    }

    static isBuiltin(request) {
      return isBuiltin(request);
    }

    // Node answers undefined unless source maps are enabled, and this
    // runtime does not enable them.
    static findSourceMap() {
      return undefined;
    }
  }
  Module._cache = Object.create(null);
  Module._pathCache = Object.create(null);
  Module._extensions = Object.assign(Object.create(null), {
    ".js": (module, filename) => module._compile(readText(filename), filename),
    ".cjs": (module, filename) => module._compile(readText(filename), filename),
    ".json": (module, filename) => {
      module.exports = JSON.parse(readText(filename));
    },
    ".node": (_module, filename) => unsupported("require", "native addon", filename),
    ".mjs": (_module, filename) => unsupported("require", "ES module", filename),
  });
  Module.Module = Module;
  Module.wrap = (script) => `(function (exports, require, module, __filename, __dirname) { ${script}\n});`;

  function makeRequire(module) {
    const require = (id) => module.require(id);
    require.resolve = (request, options) => Module._resolveFilename(request, module, false, options);
    require.resolve.paths = (request) => (isBuiltin(request) ? null : module.paths);
    require.cache = Module._cache;
    require.extensions = Module._extensions;
    require.main = undefined;
    return require;
  }

  // --- registry -----------------------------------------------------------
  const implemented = {
    async_hooks: () => strict("async_hooks", asyncHooks),
    buffer: () =>
      strict("buffer", Object.assign(Object.create(null), vendor.buffer, { Buffer, atob, btoa, Blob, File, isUtf8, isAscii })),
    crypto: () => strict("crypto", crypto),
    events: () => observed("events", events),
    fs: () => strict("fs", fs),
    "fs/promises": () => strict("fs/promises", fsPromises),
    inspector: () => strict("inspector", inspector),
    module: () => observed("module", Module),
    path: () => observed("path", path),
    "path/posix": () => observed("path/posix", path),
    perf_hooks: () => strict("perf_hooks", { performance, PerformanceEntry, PerformanceMark, PerformanceMeasure }),
    process: () => process,
    stream: () => observed("stream", stream),
    "stream/promises": () => observed("stream/promises", stream.promises),
    "stream/web": () => strict("stream/web", streamWeb),
    string_decoder: () => observed("string_decoder", vendor.string_decoder),
    timers: () => strict("timers", timers),
    "timers/promises": () => strict("timers/promises", timersPromises),
    url: () => strict("url", nodeUrl),
    util: () => strict("util", util),
    "util/types": () => strict("util/types", types),
    vm: () => strict("vm", vm),
    worker_threads: () => strict("worker_threads", workerThreads),
  };
  for (const [name, factory] of Object.entries(implemented)) BUILTINS.set(name, factory);
  for (const name of [
    "assert", "assert/strict", "child_process", "cluster", "console", "constants", "dgram",
    "diagnostics_channel", "dns", "dns/promises", "domain", "http", "http2", "https", "net",
    "punycode", "querystring", "readline", "repl", "sqlite", "sys", "test", "tls", "trace_events",
    "v8", "wasi", "zlib",
  ]) {
    BUILTINS.set(name, () => unsupportedModule(name));
  }
  // node:os: only what describes the runtime as process already does (the
  // values NextRS reports for process.platform and process.arch). Host
  // details (hostname, user, CPUs, memory, network interfaces, temp and
  // home directories) are not the application's to read: calling them
  // throws NextRSUnsupportedError, except homedir, which libraries probe
  // for (`os.homedir || fallback`, OpenTelemetry's instrumentation loader
  // does): it reads as undefined, with a diagnostic, so they fall back.
  const os = {
    EOL: "\n",
    platform: () => process.platform,
    arch: () => process.arch,
    type: () => (process.platform === "linux" ? "Linux" : process.platform === "darwin" ? "Darwin" : "Windows_NT"),
    endianness: () => "LE",
    constants: Object.freeze({ signals: Object.freeze({}), errno: Object.freeze({}) }),
  };
  // A module NextRS implements in part: `object`'s members, `absent` ones
  // read as undefined (with a diagnostic) for libraries that probe for
  // them, every other member throws NextRSUnsupportedError when called.
  function partialModule(name, object, absent = new Set()) {
    const missing = unsupportedModule(name);
    return new Proxy(object, {
      get(target, key, receiver) {
        if (typeof key === "symbol" || key in target) return Reflect.get(target, key, receiver);
        if (IGNORED_MEMBERS.has(key)) return undefined;
        if (absent.has(key)) {
          const member = `node:${name}.${key}`;
          if (!reported.has(member)) {
            reported.add(member);
            ops.op_nextrs_diagnostic("missing-member", `${member} is not implemented (read as undefined)`);
          }
          return undefined;
        }
        return missing[key];
      },
      has(target, key) {
        return key in target;
      },
    });
  }
  BUILTINS.set("os", () => partialModule("os", os, new Set(["homedir"])));
  // node:tty: no stream of the runtime is a terminal (process.stdout and
  // stderr say isTTY: false); logging libraries ask before using colors.
  BUILTINS.set("tty", () => partialModule("tty", { isatty: () => false }));
  Module.builtinModules = [...BUILTINS.keys()];

  internal.Module = Module;
  internal.Buffer = Buffer;
  internal.stream = stream;
  internal.path = path;
})(globalThis);
