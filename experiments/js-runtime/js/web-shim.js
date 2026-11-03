// Minimal web globals for engines that have none (deno_core without
// deno_web, QuickJS). Pure JS so both engines run identical code.
// Not spec-complete: just enough for React's edge server builds.
import { ReadableStream, WritableStream, TransformStream } from "web-streams-polyfill";

const g = globalThis;
if (typeof g.ReadableStream === "undefined") {
  Object.assign(g, { ReadableStream, WritableStream, TransformStream });
}
if (typeof g.TextEncoder === "undefined") {
  g.TextEncoder = class TextEncoder {
    get encoding() {
      return "utf-8";
    }
    encode(input = "") {
      const bytes = [];
      for (const ch of String(input)) {
        let c = ch.codePointAt(0);
        if (c < 0x80) bytes.push(c);
        else if (c < 0x800) bytes.push(0xc0 | (c >> 6), 0x80 | (c & 63));
        else if (c < 0x10000) bytes.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
        else bytes.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      }
      return new Uint8Array(bytes);
    }
    encodeInto(input, dest) {
      const bytes = this.encode(input);
      dest.set(bytes.subarray(0, dest.length));
      return { read: input.length, written: Math.min(bytes.length, dest.length) };
    }
  };
}
if (typeof g.TextDecoder === "undefined") {
  g.TextDecoder = class TextDecoder {
    decode(bytes, _options) {
      if (!bytes) return "";
      const b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes.buffer ?? bytes);
      let out = "";
      for (let i = 0; i < b.length; ) {
        const c = b[i++];
        let cp = c;
        if (c >= 0xf0) cp = ((c & 7) << 18) | ((b[i++] & 63) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63);
        else if (c >= 0xe0) cp = ((c & 15) << 12) | ((b[i++] & 63) << 6) | (b[i++] & 63);
        else if (c >= 0xc0) cp = ((c & 31) << 6) | (b[i++] & 63);
        out += String.fromCodePoint(cp);
      }
      return out;
    }
  };
}
if (typeof g.AbortController === "undefined") {
  // React's Flight server creates one per request to propagate aborts.
  class AbortSignal {
    constructor() {
      this.aborted = false;
      this.reason = undefined;
      this._listeners = [];
    }
    addEventListener(type, listener) {
      if (type === "abort") this._listeners.push(listener);
    }
    removeEventListener(type, listener) {
      this._listeners = this._listeners.filter((l) => l !== listener);
    }
    throwIfAborted() {
      if (this.aborted) throw this.reason;
    }
  }
  g.AbortSignal = AbortSignal;
  g.AbortController = class AbortController {
    constructor() {
      this.signal = new AbortSignal();
    }
    abort(reason = new Error("This operation was aborted")) {
      if (this.signal.aborted) return;
      this.signal.aborted = true;
      this.signal.reason = reason;
      for (const listener of this.signal._listeners) listener({ type: "abort" });
    }
  };
}
