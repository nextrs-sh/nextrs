// Asymmetric Web Crypto, HKDF and AES-CBC checks. The same file runs under
// Node (`node webcrypto-asym.js`), which prints the reference results the
// Rust test compares against (webcrypto-asym.expected.json); keys under
// `nextrsOnly` are NextRS's documented refusals, which Node does not share.
//
// KEYS are test keys generated once with Node's Web Crypto for this file
// (no secret protects anything with them); SIGS are Node's signatures
// over MSG with them (ECDSA and RSA-PSS signatures are randomized, so
// NextRS must verify Node's). Deterministic outputs (exports,
// RSASSA-PKCS1-v1_5 and Ed25519 signatures, HKDF, AES-CBC) must be
// byte-identical.
"use strict";

const KEYS = {
  "p256": {
    "private": {
      "kty": "EC",
      "x": "GSm_NcCNNxH9NBlL11k5rhKk_JSecUCSRLMRC_d37G8",
      "y": "OpgHWAVucen7jLsSjInA6SEbcjy9R3jIjyoS-SZKkSg",
      "crv": "P-256",
      "d": "_9qldIqrNbNsa0rlZEA95w4nwPpd_SuaNakPkrHycec"
    }
  },
  "p384": {
    "private": {
      "kty": "EC",
      "x": "R9WKQDW67bq3DXMb4z765Qrac_BYv78yR9zgYZSSIqqgmCALwZSzTDtCmA4kqdpl",
      "y": "igeB_e8LlK4s0-x2_mdV8XGNtlUxGR78bPbdFMfl3qdJNzWiaZYhCnOxgbpIcktI",
      "crv": "P-384",
      "d": "dToozTs1Bsn5IYooaldDbI3eEN_3VMbaIw532vxvXWMbVVX4popQEIslcr6stWLK"
    }
  },
  "rsa2048": {
    "private": {
      "kty": "RSA",
      "n": "onUpaPLCmKhMNG0UygoX3D8G2R2_HQr5JnrtgFxV8w8OW01wPsipo-jMq0dQOTG6zopsryr7wUDLyMbc1tnvJnfonZsyg21bR0Zo2A8F57cYFnNJ-93JbeZ7bnr-JbuG2jmprr0M2HQ6D5HTgMdaPuDoxJR_5XD3oO11tQeOFcZV5GMvnk_qsDshzFnYa2G0jQDaAfJmSr22KmjwDJnD9gA2nj80pI2VVuyVmmf-FQ92xc1VhR-ErTJhHkNA66wbdQOOrn_1XGWN_BAaBh-LXzXIpQVFAGV6AmlATZ9f7lFiu3mKOWb40dR3FBF9fwqe0dX7isRqj3HDs1nnX6TRKQ",
      "e": "AQAB",
      "d": "AXGYm109mPqKk38Hq677ciKLlp4yzq4vCNDtWjzCf8fkY6_UaEkYvC3rXwyhxsxYFxH4Wssp6cMAEfSgeEv_WdXAIbcZc70No1xvE0J46zZU7A18vL9bv6gIH0h6XeP6KWd7SE6s9fcnH_B2vc19EhN7cL8gGYN6Po91rRTtheiAbys6UDPiNtPNQ6y5eT_VRmXbD0US_TdGkbO0ilsk0u6KtzAhAPT2veocLE5vUqbPPDZWtd5s-xpCrp0pJE2DZIRctb2fPGNdB6LKrCf2D2uIIeBttj4wLua5SSf17VEDAHlF8Nl79kzIaBZnTsdNcWQrXpJrYKHo7CQi2otj7Q",
      "p": "2tlJ-0q61WAgh2VXYBicdu72hkK2Iaz5Gvgzm1rxSX0f3y8xrMzZs4OKczB2ZuPqtKZw-23Tgqenc9aVZziWVHwYs5wMwO-zpaEbSeHz2FGnKfzRt3joUDAaJ3tr9SwRjxBuQymirVjFdV-Zj1jMVGLiee04yzEcDQfy606L6RU",
      "q": "vgk7fzoJ5J0N7QLVkiiDJruK4H5hQmaihp7W1NT18V-NKmOVRELR8MAlG9IbEjDUOWneRbUSRf4cXYehEHnWzKLycrUetkUXjXFEv5uh4Rx2F4lL30h1LJDk08qoJ1l7EACijmXEmO2dQZHT4OIHP_tyforY9_OCHtHpBRJFpMU",
      "dp": "LpcTqOu_2q0cP7bTYyHaLXeyA-Mv4vtwDM3KLsitlXeN0XW6nY7-ybi3QM6QZlcfIFZOtzgqTyZh_8cvB0qEtne0yofuLyTYmeoyZQg1YVgAvm-UNfsHegrDhqQ_13IKSuq0jpfOihjb8KDpkBrXZR-oF0zpSliqIBwRGwcdwgE",
      "dq": "G7EguGNuuZ-hPd5h3NCGfAhsLlZ8Ob7XBCL1T0JRLIEuAvVL9Qx_p0N4pNszEq2G327r8uB17zkeTQrMj6gRgvRjvA9kkE71JalRyVIWfsU0-C0Ln1fhq0LZ9Xrckg9Z_XT6OyHdSiOl_L4RT_3f7JTplEMLZuZXGcKg4BpPh7k",
      "qi": "Li_jKEyJnPJ3VYbh8Ttyn6UaGt_oMTwqh2KomSOu4063mUznakDpq6SVFsJRDcJ6qLNvlyiaY1fA9JQyW5tqfJSQBYMKiMC94UgPl1OqoumliM3i4ftbQp_5CosqwgC7SKk9WowfOlP6bUMm73jcKZE9KdGEzLgffDlrsTgYt8Q"
    }
  },
  "rsa1024": {
    "private": {
      "kty": "RSA",
      "n": "3is9xgnH0RJ1wa0EWEmafMX1zKt5D0bxfr7UV9779VeEsN5UhQXAhrpt0eSTl9OtyoYyOocFId6Br7hQ29XLhiTuVaXQx68yjFtcVyCVVzhl-0l3AQszJ1yhi_h1L7oacpRbkRFvVdc5odaGWNplvA7Oa9TnlWQUte34izENJyU",
      "e": "AQAB",
      "d": "r2d4aAiEx95JNlYBXisOW4B3pdWjyJn8RFMplw6n4nXsfy-pjzSRYk2CMvrCRFaNmZ4Fvgun06u2vCNLQ6Ux_wfQP1lb-FA3JJbzMFUjqSnAQho08lsjKOTXFIRqwi_k1OinxGX0jppgn9-dFuhsNb79B9iRtsh011ZdC1Sp6sE",
      "p": "-r64uiclgRTlh3OwtxP28r6W42Axq2hqTDVv6g28gQ3KqPPthy06FNAgBN5QeQminWNNVRLM8bxW_T78YJTZOQ",
      "q": "4tM0k7yfxlE91C1TnfC5zqWF0A_hbr7-IqO75Y9iY0b-jhRTVJ8KtxX9beNA00IpwR7udbr1qD8Zcy6SGdtZTQ",
      "dp": "0XRp6Q50VLx488ofTv99ZM7ySiN7ujxxtxFYWEJ7Oa6koLeKrkWIZARrCsYOoNz8ms5ttegLjM8QN2Ha_m64eQ",
      "dq": "KkM2EazcjEQI5CbJ5Ky8wh8fLqQ22b6mdWfSk7RCncLVnT12RVcY1QI45SeppXM3em87-R2rtfyDzm1H9Z9kJQ",
      "qi": "iZcxQiGBE6CqiP9S9FK4GQdaAkH1w7c_xTFtCjLO7yXcUrp596rjAM9Y2Z6KF_G5XhtXZrwDYJxYUKldb7eSNQ"
    }
  },
  "ed25519": {
    "private": {
      "crv": "Ed25519",
      "d": "NMy2uN0FvBA5ZxjxNhcS4Ljtt81RQj5rJb16E48QGuA",
      "x": "sFCkWSAYUOf8KO2oSGJ_hGvFGpL9MTaaj-pVmdwuaH8",
      "kty": "OKP"
    }
  }
};
const SIGS = {
  "es256": "58abe005ff28e49ea8f1168bd4e7a774f71fde4784ba95f430c29b4d8fc5171279cb303639995e3f832194fb0756a364fd6529baa7c2419df9b42cfcc7fec781",
  "es384": "f77deacbc3b89c04142f8d1bc46a308983225228aa85bcfb8c02bedf0c2dadde4bbadb2870ebcfe7dcaa12b213c62ef4cd42b15cf051664d115bec9958045b70f567956bf1f951fa763c47ade01f89fbc20a32e81abc48c35a1a62a07e028dec",
  "ps256": "7fed3466f15013f119d42d1fb3841ef20482e1172028d1355aeb4145e7cfd67662ef0b4c3b2547643e9d00664783db01619759a2528e7555bd839c44aac2189a12b17a00792e1699f475ab39380810dffeca5f8f26fa558575f11d202b26821dbda7d29da7ff9db49a40962724ba588ceed659612df1fe6ed761bfe962759801aff177e2bbf5f7b69bb6a486c4cfaeedc7bdf9322d456e4d1f5eaacb0d8d0946cf4a8086527a5720326fc8f7a08bd4021f65a1d56ad83439bac4eca46c36abbd8fe4c099cb3c943c23b9abb149eb9cff7e1a9ef222cac3b4947deff5d36f4867f792d496d7179c8a8052f620e32d24062f50ae045d6b0050cbd12cb496444459",
  "ps512": "8655c14f243417dd9f2f9eda318d98702f00025fa05f40f6cf923d1cc3e9528491863818d53308e6cf945d20d763412ce0a16c6327753906e21fa9c89b39b2a0b7891a0e02e6a4b34c9fe591e37a82f6370017cf724d72ad8a4928056ba837b4c60d6efe95a5a1b2f8e1ec9d1ba4d83857febdc354e92b2b885b58a3a2057a88f7c1bd76000c4483e9aacd7ee57b6b53bf901c2db7d1acce99b0cf921cb93c179eaa14a38682c5f2141261c0f3c98277a7567289be469dae24deaf84adca29772a4afc7598470d3084979d7f2b5acf792564d0866623eb7450f4d694709a4a69d4ce25c6a4b6ed7882f34c9d339632ffab2433eefd212bb30b48efa57104e362"
};

const subtle = globalThis.crypto.subtle;
const hex = (buf) => Buffer.from(buf instanceof ArrayBuffer ? new Uint8Array(buf) : buf).toString("hex");
const unhex = (s) => Uint8Array.from(Buffer.from(s, "hex"));
const MSG = new TextEncoder().encode("NextRS Web Crypto oracle message");
const outcome = async (fn) => {
  try {
    const value = await fn();
    return { ok: value === undefined ? null : value };
  } catch (error) {
    return { error: error?.code === "ERR_NEXTRS_UNSUPPORTED" ? "ERR_NEXTRS_UNSUPPORTED" : `${error?.name}` };
  }
};
const publicJwk = (jwk) => Object.fromEntries(Object.entries(jwk).filter(([k]) => !["d", "p", "q", "dp", "dq", "qi"].includes(k)));
const sortKeys = (o) => Object.fromEntries(Object.entries(o).sort(([a], [b]) => (a < b ? -1 : 1)));
const describe = (key) => ({
  type: key.type,
  extractable: key.extractable,
  usages: [...key.usages],
  algorithm: Object.fromEntries(Object.entries(key.algorithm).map(([k, v]) => [k, v instanceof Uint8Array ? hex(v) : v && typeof v === "object" ? { ...v } : v])),
  tag: Object.prototype.toString.call(key),
  instance: key instanceof CryptoKey,
});
const flip = (bytes, i = 0) => {
  const out = new Uint8Array(bytes);
  out[i] ^= 1;
  return out;
};

async function ecdsa(r, n, curve, hash, jwk, sig) {
  const alg = { name: "ECDSA", namedCurve: curve };
  const params = { name: "ECDSA", hash };
  const priv = await subtle.importKey("jwk", jwk, alg, true, ["sign"]);
  const pub = await subtle.importKey("jwk", publicJwk(jwk), alg, true, ["verify"]);
  r[`${n}Private`] = describe(priv);
  r[`${n}Public`] = describe(pub);
  const spki = await subtle.exportKey("spki", pub);
  const pkcs8 = await subtle.exportKey("pkcs8", priv);
  const raw = await subtle.exportKey("raw", pub);
  r[`${n}Spki`] = hex(spki);
  r[`${n}Pkcs8`] = hex(pkcs8);
  r[`${n}Raw`] = hex(raw);
  r[`${n}JwkPrivate`] = sortKeys(await subtle.exportKey("jwk", priv));
  r[`${n}JwkPublic`] = sortKeys(await subtle.exportKey("jwk", pub));
  // Every format imports back to a key that verifies Node's signature.
  for (const [format, data, usages] of [["spki", spki, ["verify"]], ["raw", raw, ["verify"]], ["pkcs8", pkcs8, ["sign"]]]) {
    const key = await subtle.importKey(format, data, alg, false, usages);
    r[`${n}Import_${format}`] = describe(key);
    if (format === "pkcs8") {
      const signature = await subtle.sign(params, key, MSG);
      r[`${n}Pkcs8SignVerifies`] = await subtle.verify(params, pub, signature, MSG);
    } else {
      r[`${n}Import_${format}_verifiesNode`] = await subtle.verify(params, key, unhex(sig), MSG);
    }
  }
  r[`${n}VerifyNode`] = await subtle.verify(params, pub, unhex(sig), MSG);
  r[`${n}VerifyTamperedData`] = await subtle.verify(params, pub, unhex(sig), flip(MSG));
  r[`${n}VerifyTamperedSig`] = await subtle.verify(params, pub, flip(unhex(sig), 5), MSG);
  r[`${n}VerifyShortSig`] = await subtle.verify(params, pub, unhex(sig).slice(1), MSG);
  r[`${n}VerifyEmptySig`] = await subtle.verify(params, pub, new Uint8Array(0), MSG);
  const own = await subtle.sign(params, priv, MSG);
  r[`${n}SignLength`] = own.byteLength;
  r[`${n}SignVerifies`] = await subtle.verify(params, pub, own, MSG);
  r[`${n}SignWithPublic`] = await outcome(() => subtle.sign(params, pub, MSG));
  r[`${n}VerifyWithPrivate`] = await outcome(() => subtle.verify(params, priv, unhex(sig), MSG));
  r[`${n}WrongAlgorithm`] = await outcome(() => subtle.verify({ name: "RSA-PSS", saltLength: 32 }, pub, unhex(sig), MSG));
  r[`${n}NoHash`] = await outcome(() => subtle.verify({ name: "ECDSA" }, pub, unhex(sig), MSG));
  // Import errors.
  r[`${n}PublicWithSign`] = await outcome(() => subtle.importKey("spki", spki, alg, true, ["sign"]));
  r[`${n}PrivateNoUsages`] = await outcome(() => subtle.importKey("pkcs8", pkcs8, alg, true, []));
  r[`${n}PublicNoUsages`] = describe(await subtle.importKey("spki", spki, alg, true, []));
  r[`${n}BadUsage`] = await outcome(() => subtle.importKey("spki", spki, alg, true, ["fly"]));
  const other = curve === "P-256" ? "P-384" : "P-256";
  r[`${n}WrongCurveSpki`] = await outcome(() => subtle.importKey("spki", spki, { name: "ECDSA", namedCurve: other }, true, ["verify"]));
  r[`${n}WrongCurvePkcs8`] = await outcome(() => subtle.importKey("pkcs8", pkcs8, { name: "ECDSA", namedCurve: other }, true, ["sign"]));
  r[`${n}WrongCurveJwk`] = await outcome(() => subtle.importKey("jwk", publicJwk(jwk), { name: "ECDSA", namedCurve: other }, true, ["verify"]));
  r[`${n}JwkWrongKty`] = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), kty: "RSA" }, alg, true, ["verify"]));
  r[`${n}JwkShortX`] = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), x: jwk.x.slice(4) }, alg, true, ["verify"]));
  r[`${n}JwkNoY`] = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), y: undefined }, alg, true, ["verify"]));
  r[`${n}JwkWrongAlg`] = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), alg: curve === "P-256" ? "ES384" : "ES256" }, alg, true, ["verify"]));
  r[`${n}JwkRightAlg`] = describe(await subtle.importKey("jwk", { ...publicJwk(jwk), alg: curve === "P-256" ? "ES256" : "ES384" }, alg, true, ["verify"]));
  r[`${n}JwkUseEnc`] = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), use: "enc" }, alg, true, ["verify"]));
  r[`${n}JwkKeyOps`] = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), key_ops: ["sign"] }, alg, true, ["verify"]));
  r[`${n}JwkExtFalse`] = await outcome(() => subtle.importKey("jwk", { ...jwk, ext: false }, alg, true, ["sign"]));
  r[`${n}JwkMismatchedD`] = await outcome(async () => {
    const otherJwk = curve === "P-256" ? KEYS.p256.private : KEYS.p384.private;
    const generated = await subtle.generateKey(alg, true, ["sign"]);
    const d = (await subtle.exportKey("jwk", generated.privateKey)).d;
    return describe(await subtle.importKey("jwk", { ...otherJwk, d }, alg, true, ["sign"]));
  });
  r[`${n}NotExtractable`] = await outcome(async () => subtle.exportKey("jwk", await subtle.importKey("jwk", jwk, alg, false, ["sign"])));
  r[`${n}PrivateAsSpki`] = await outcome(() => subtle.exportKey("spki", priv));
  r[`${n}PublicAsPkcs8`] = await outcome(() => subtle.exportKey("pkcs8", pub));
  r[`${n}TruncatedSpki`] = await outcome(() => subtle.importKey("spki", new Uint8Array(spki).slice(0, 40), alg, true, ["verify"]));
  r[`${n}TrailingSpki`] = await outcome(() => subtle.importKey("spki", Uint8Array.from([...new Uint8Array(spki), 0]), alg, true, ["verify"]));
  r[`${n}RawWrongLength`] = await outcome(() => subtle.importKey("raw", new Uint8Array(raw).slice(0, 20), alg, true, ["verify"]));
  r[`${n}RsaSpkiAsEc`] = await outcome(async () => {
    const rsa = await subtle.importKey("jwk", publicJwk(KEYS.rsa2048.private), { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, true, ["verify"]);
    return subtle.importKey("spki", await subtle.exportKey("spki", rsa), alg, true, ["verify"]);
  });
}

async function rsa(r) {
  const jwk = KEYS.rsa2048.private;
  const pkcs1 = { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" };
  const priv = await subtle.importKey("jwk", jwk, pkcs1, true, ["sign"]);
  const pub = await subtle.importKey("jwk", publicJwk(jwk), pkcs1, true, ["verify"]);
  r.rsaPrivate = describe(priv);
  r.rsaPublic = describe(pub);
  const spki = await subtle.exportKey("spki", pub);
  const pkcs8 = await subtle.exportKey("pkcs8", priv);
  r.rsaSpki = hex(spki);
  r.rsaPkcs8 = hex(pkcs8);
  r.rsaJwkPrivate = sortKeys(await subtle.exportKey("jwk", priv));
  r.rsaJwkPublic = sortKeys(await subtle.exportKey("jwk", pub));
  // RSASSA-PKCS1-v1_5 is deterministic: Node's and NextRS's signatures match.
  const rs256 = await subtle.sign({ name: "RSASSA-PKCS1-v1_5" }, priv, MSG);
  r.rs256 = hex(rs256);
  r.rs256Verify = await subtle.verify({ name: "RSASSA-PKCS1-v1_5" }, pub, rs256, MSG);
  r.rs256TamperedSig = await subtle.verify({ name: "RSASSA-PKCS1-v1_5" }, pub, flip(new Uint8Array(rs256), 100), MSG);
  r.rs256TamperedData = await subtle.verify({ name: "RSASSA-PKCS1-v1_5" }, pub, rs256, flip(MSG, 3));
  r.rs256ShortSig = await subtle.verify({ name: "RSASSA-PKCS1-v1_5" }, pub, new Uint8Array(rs256).slice(1), MSG);
  for (const hash of ["SHA-384", "SHA-512"]) {
    const key = await subtle.importKey("pkcs8", pkcs8, { name: "RSASSA-PKCS1-v1_5", hash }, false, ["sign"]);
    const verifier = await subtle.importKey("spki", spki, { name: "RSASSA-PKCS1-v1_5", hash }, false, ["verify"]);
    const signature = await subtle.sign("RSASSA-PKCS1-v1_5", key, MSG);
    r[`rs${hash.slice(4)}`] = hex(signature);
    r[`rs${hash.slice(4)}Verify`] = await subtle.verify("RSASSA-PKCS1-v1_5", verifier, signature, MSG);
    // A key bound to another hash does not verify it.
    r[`rs${hash.slice(4)}WrongHashKey`] = await subtle.verify("RSASSA-PKCS1-v1_5", pub, signature, MSG);
  }
  // RSA-PSS: Node's signatures verify; NextRS's own verify.
  for (const [hash, salt, sig] of [["SHA-256", 32, SIGS.ps256], ["SHA-512", 64, SIGS.ps512]]) {
    const alg = { name: "RSA-PSS", hash };
    const ps = `ps${hash.slice(4)}`;
    const key = await subtle.importKey("pkcs8", pkcs8, alg, false, ["sign"]);
    const verifier = await subtle.importKey("spki", spki, alg, true, ["verify"]);
    r[`${ps}Public`] = describe(verifier);
    r[`${ps}JwkPublic`] = sortKeys(await subtle.exportKey("jwk", verifier));
    r[`${ps}VerifyNode`] = await subtle.verify({ name: "RSA-PSS", saltLength: salt }, verifier, unhex(sig), MSG);
    r[`${ps}VerifyTampered`] = await subtle.verify({ name: "RSA-PSS", saltLength: salt }, verifier, flip(unhex(sig), 9), MSG);
    const own = await subtle.sign({ name: "RSA-PSS", saltLength: salt }, key, MSG);
    r[`${ps}SignLength`] = own.byteLength;
    r[`${ps}SignVerifies`] = await subtle.verify({ name: "RSA-PSS", saltLength: salt }, verifier, own, MSG);
    r[`${ps}AsPkcs1`] = await outcome(() => subtle.verify({ name: "RSASSA-PKCS1-v1_5" }, verifier, unhex(sig), MSG));
  }
  r.psNoSalt = await outcome(async () => subtle.verify({ name: "RSA-PSS" }, await subtle.importKey("spki", spki, { name: "RSA-PSS", hash: "SHA-256" }, false, ["verify"]), unhex(SIGS.ps256), MSG));
  r.rsaJwkAlgRS256 = describe(await subtle.importKey("jwk", { ...publicJwk(jwk), alg: "RS256" }, pkcs1, false, ["verify"]));
  r.rsaJwkAlgRS384 = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), alg: "RS384" }, pkcs1, false, ["verify"]));
  r.rsaJwkAlgPS256 = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), alg: "PS256" }, pkcs1, false, ["verify"]));
  r.rsaNoHash = await outcome(() => subtle.importKey("spki", spki, { name: "RSASSA-PKCS1-v1_5" }, false, ["verify"]));
  r.rsaRaw = await outcome(() => subtle.importKey("raw", spki, pkcs1, false, ["verify"]));
  r.rsaJwkMissingN = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), n: undefined }, pkcs1, false, ["verify"]));
  r.rsaJwkPartialCrt = await outcome(() => subtle.importKey("jwk", { ...jwk, qi: undefined }, pkcs1, false, ["sign"]));
  r.rsaJwkWrongCrt = await outcome(async () => {
    const swapped = await subtle.importKey("jwk", { ...jwk, dp: jwk.dq, dq: jwk.dp }, pkcs1, false, ["sign"]);
    return subtle.sign("RSASSA-PKCS1-v1_5", swapped, MSG).then(hex);
  });
  r.rsaPkcs8AsSpki = await outcome(() => subtle.importKey("spki", pkcs8, pkcs1, false, ["verify"]));
  r.ecSpkiAsRsa = await outcome(async () => {
    const ec = await subtle.importKey("jwk", publicJwk(KEYS.p256.private), { name: "ECDSA", namedCurve: "P-256" }, true, ["verify"]);
    return subtle.importKey("spki", await subtle.exportKey("spki", ec), pkcs1, false, ["verify"]);
  });
}

async function ed25519(r) {
  const jwk = KEYS.ed25519.private;
  const alg = { name: "Ed25519" };
  const priv = await subtle.importKey("jwk", jwk, alg, true, ["sign"]);
  const pub = await subtle.importKey("jwk", publicJwk(jwk), alg, true, ["verify"]);
  r.edPrivate = describe(priv);
  r.edPublic = describe(pub);
  const spki = await subtle.exportKey("spki", pub);
  const pkcs8 = await subtle.exportKey("pkcs8", priv);
  const raw = await subtle.exportKey("raw", pub);
  r.edSpki = hex(spki);
  r.edPkcs8 = hex(pkcs8);
  r.edRaw = hex(raw);
  r.edJwkPrivate = sortKeys(await subtle.exportKey("jwk", priv));
  r.edJwkPublic = sortKeys(await subtle.exportKey("jwk", pub));
  const signature = await subtle.sign(alg, priv, MSG);
  r.edSignature = hex(signature);
  r.edVerify = await subtle.verify(alg, pub, signature, MSG);
  r.edVerifyTampered = await subtle.verify(alg, pub, flip(new Uint8Array(signature), 40), MSG);
  r.edVerifyTamperedData = await subtle.verify(alg, pub, signature, flip(MSG, 1));
  r.edVerifyShort = await subtle.verify(alg, pub, new Uint8Array(signature).slice(0, 63), MSG);
  for (const [format, data, usages] of [["spki", spki, ["verify"]], ["raw", raw, ["verify"]], ["pkcs8", pkcs8, ["sign"]]]) {
    const key = await subtle.importKey(format, data, "Ed25519", false, usages);
    r[`edImport_${format}`] = describe(key);
    r[`edImport_${format}_works`] = format === "pkcs8" ? hex(await subtle.sign("Ed25519", key, MSG)) : await subtle.verify("Ed25519", key, signature, MSG);
  }
  r.edJwkAlgEdDSA = describe(await subtle.importKey("jwk", { ...publicJwk(jwk), alg: "EdDSA" }, alg, false, ["verify"]));
  r.edJwkWrongCrv = await outcome(() => subtle.importKey("jwk", { ...publicJwk(jwk), crv: "X25519" }, alg, false, ["verify"]));
  r.edJwkMismatched = await outcome(() => subtle.importKey("jwk", { ...jwk, x: publicJwk(KEYS.ed25519.private).x.replace(/^./, (c) => (c === "A" ? "B" : "A")) }, alg, false, ["sign"]));
  r.edRawShort = await outcome(() => subtle.importKey("raw", new Uint8Array(31), alg, false, ["verify"]));
  r.edSignWithPublic = await outcome(() => subtle.sign(alg, pub, MSG));
}

async function generated(r) {
  for (const [name, alg, params] of [
    ["genP256", { name: "ECDSA", namedCurve: "P-256" }, { name: "ECDSA", hash: "SHA-256" }],
    ["genP384", { name: "ECDSA", namedCurve: "P-384" }, { name: "ECDSA", hash: "SHA-384" }],
    ["genEd25519", { name: "Ed25519" }, { name: "Ed25519" }],
  ]) {
    const pair = await subtle.generateKey(alg, false, ["sign", "verify"]);
    r[`${name}Public`] = describe(pair.publicKey);
    r[`${name}Private`] = describe(pair.privateKey);
    const signature = await subtle.sign(params, pair.privateKey, MSG);
    r[`${name}Verifies`] = await subtle.verify(params, pair.publicKey, signature, MSG);
    const exported = await subtle.exportKey("jwk", pair.publicKey);
    r[`${name}JwkMembers`] = Object.keys(exported).sort();
    r[`${name}PrivateExport`] = await outcome(() => subtle.exportKey("pkcs8", pair.privateKey));
    r[`${name}PublicOnlyUsages`] = await outcome(() => subtle.generateKey(alg, true, ["verify"]));
    r[`${name}BadUsage`] = await outcome(() => subtle.generateKey(alg, true, ["sign", "encrypt"]));
  }
  // Two generated keys differ.
  const a = await subtle.generateKey({ name: "Ed25519" }, true, ["sign"]);
  const b = await subtle.generateKey({ name: "Ed25519" }, true, ["sign"]);
  r.genDistinct = hex(await subtle.exportKey("raw", a.publicKey)) !== hex(await subtle.exportKey("raw", b.publicKey));
}

async function hkdf(r) {
  // RFC 5869 test case 1.
  const ikm = new Uint8Array(22).fill(0x0b);
  const key = await subtle.importKey("raw", ikm, "HKDF", false, ["deriveBits"]);
  r.hkdfKey = describe(key);
  const params = { name: "HKDF", hash: "SHA-256", salt: unhex("000102030405060708090a0b0c"), info: unhex("f0f1f2f3f4f5f6f7f8f9") };
  r.hkdfTc1 = hex(await subtle.deriveBits(params, key, 42 * 8));
  // What Auth.js does (@panva/hkdf): SHA-256, 64 bytes for A256CBC-HS512.
  const secret = await subtle.importKey("raw", new TextEncoder().encode("test-secret-value"), "HKDF", false, ["deriveBits"]);
  r.hkdfAuthjs = hex(await subtle.deriveBits({ name: "HKDF", hash: "SHA-256", salt: new TextEncoder().encode("authjs.session-token"), info: new TextEncoder().encode("Auth.js Generated Encryption Key (authjs.session-token)") }, secret, 512));
  r.hkdfSha512 = hex(await subtle.deriveBits({ ...params, hash: "SHA-512" }, key, 256));
  r.hkdfExtractable = await outcome(() => subtle.importKey("raw", ikm, "HKDF", true, ["deriveBits"]));
  r.hkdfExport = await outcome(() => subtle.exportKey("raw", key));
  r.hkdfBadUsage = await outcome(() => subtle.importKey("raw", ikm, "HKDF", false, ["sign"]));
  r.hkdfNullLength = await outcome(() => subtle.deriveBits(params, key, null));
  r.hkdfOddLength = await outcome(() => subtle.deriveBits(params, key, 12));
  r.hkdfTooLong = await outcome(() => subtle.deriveBits(params, key, 255 * 32 * 8 + 8));
  r.hkdfNoInfo = await outcome(() => subtle.deriveBits({ name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0) }, key, 256));
  r.hkdfWrongKey = await outcome(async () => subtle.deriveBits(params, await subtle.importKey("raw", ikm, { name: "HMAC", hash: "SHA-256" }, false, ["sign"]), 256));
}

async function aesCbc(r) {
  // NIST SP 800-38A F.2.5 (CBC-AES256), with the padding block Web Crypto adds.
  const key = await subtle.importKey("raw", unhex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4"), "AES-CBC", true, ["encrypt", "decrypt"]);
  r.cbcKey = describe(key);
  const iv = unhex("000102030405060708090a0b0c0d0e0f");
  const ct = await subtle.encrypt({ name: "AES-CBC", iv }, key, unhex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51"));
  r.cbcCiphertext = hex(ct);
  r.cbcDecrypt = hex(await subtle.decrypt({ name: "AES-CBC", iv }, key, ct));
  r.cbcEmpty = hex(await subtle.encrypt({ name: "AES-CBC", iv }, key, new Uint8Array(0)));
  r.cbcBadPadding = await outcome(() => subtle.decrypt({ name: "AES-CBC", iv }, key, new Uint8Array(ct).slice(0, 32)));
  r.cbcPartialBlock = await outcome(() => subtle.decrypt({ name: "AES-CBC", iv }, key, new Uint8Array(ct).slice(0, 40)));
  r.cbcShortIv = await outcome(() => subtle.encrypt({ name: "AES-CBC", iv: iv.slice(0, 12) }, key, new Uint8Array(4)));
  r.cbcJwk = sortKeys(await subtle.exportKey("jwk", key));
  r.cbcJwkImport = describe(await subtle.importKey("jwk", { kty: "oct", k: r.cbcJwk.k, alg: "A256CBC" }, "AES-CBC", false, ["decrypt"]));
  r.cbcJwkWrongAlg = await outcome(() => subtle.importKey("jwk", { kty: "oct", k: r.cbcJwk.k, alg: "A256GCM" }, "AES-CBC", false, ["decrypt"]));
  r.cbcAsGcm = await outcome(() => subtle.encrypt({ name: "AES-GCM", iv }, key, new Uint8Array(4)));
  const generatedKey = await subtle.generateKey({ name: "AES-CBC", length: 128 }, true, ["encrypt"]);
  r.cbcGenerated = describe(generatedKey);
  // HMAC and AES-GCM keys as JWKs.
  const hmac = await subtle.importKey("jwk", { kty: "oct", k: "c2VjcmV0LWhtYWMta2V5", alg: "HS256" }, { name: "HMAC", hash: "SHA-256" }, true, ["sign", "verify"]);
  r.hmacJwk = describe(hmac);
  r.hmacJwkSign = hex(await subtle.sign("HMAC", hmac, MSG));
  r.hmacJwkExport = sortKeys(await subtle.exportKey("jwk", hmac));
  r.hmacJwkWrongAlg = await outcome(() => subtle.importKey("jwk", { kty: "oct", k: "c2VjcmV0LWhtYWMta2V5", alg: "HS512" }, { name: "HMAC", hash: "SHA-256" }, true, ["sign"]));
  const gcm = await subtle.importKey("raw", new Uint8Array(16).fill(3), "AES-GCM", true, ["encrypt"]);
  r.gcmJwkExport = sortKeys(await subtle.exportKey("jwk", gcm));
}

async function concurrency(r) {
  // Many interleaved operations on shared keys: every result is right.
  const p256 = await subtle.importKey("jwk", KEYS.p256.private, { name: "ECDSA", namedCurve: "P-256" }, false, ["sign"]);
  const p256pub = await subtle.importKey("jwk", publicJwk(KEYS.p256.private), { name: "ECDSA", namedCurve: "P-256" }, false, ["verify"]);
  const rsaPub = await subtle.importKey("jwk", publicJwk(KEYS.rsa2048.private), { name: "RSA-PSS", hash: "SHA-256" }, false, ["verify"]);
  const edPub = await subtle.importKey("jwk", publicJwk(KEYS.ed25519.private), { name: "Ed25519" }, false, ["verify"]);
  const jobs = [];
  for (let i = 0; i < 120; i++) {
    const msg = new TextEncoder().encode(`message ${i}`);
    jobs.push(subtle.sign({ name: "ECDSA", hash: "SHA-256" }, p256, msg).then((s) => subtle.verify({ name: "ECDSA", hash: "SHA-256" }, p256pub, s, msg)));
    jobs.push(subtle.verify({ name: "RSA-PSS", saltLength: 32 }, rsaPub, unhex(SIGS.ps256), i % 2 ? MSG : msg).then((ok) => ok === (i % 2 === 1)));
    jobs.push(subtle.verify("Ed25519", edPub, new Uint8Array(64), msg).then((ok) => ok === false));
  }
  const results = await Promise.all(jobs);
  r.concurrent = { total: results.length, right: results.filter(Boolean).length };
}

async function run() {
  const r = {};
  await ecdsa(r, "p256", "P-256", "SHA-256", KEYS.p256.private, SIGS.es256);
  await ecdsa(r, "p384", "P-384", "SHA-384", KEYS.p384.private, SIGS.es384);
  await rsa(r);
  await ed25519(r);
  await generated(r);
  await hkdf(r);
  await aesCbc(r);
  await concurrency(r);
  const spki = (jwk, alg) => subtle.importKey("jwk", publicJwk(jwk), alg, true, ["verify"]).then((k) => subtle.exportKey("spki", k));
  const nextrsOnly = {
    p521: await outcome(() => subtle.generateKey({ name: "ECDSA", namedCurve: "P-521" }, false, ["sign"])),
    p256Sha384: await outcome(async () => subtle.verify({ name: "ECDSA", hash: "SHA-384" }, await subtle.importKey("jwk", publicJwk(KEYS.p256.private), { name: "ECDSA", namedCurve: "P-256" }, false, ["verify"]), unhex(SIGS.es256), MSG)),
    compressedPoint: await outcome(async () => {
      const raw = new Uint8Array(await subtle.exportKey("raw", await subtle.importKey("jwk", publicJwk(KEYS.p256.private), { name: "ECDSA", namedCurve: "P-256" }, true, ["verify"])));
      const compressed = Uint8Array.from([2 + (raw[64] & 1), ...raw.slice(1, 33)]);
      return subtle.importKey("raw", compressed, { name: "ECDSA", namedCurve: "P-256" }, false, ["verify"]);
    }),
    rsa1024: await outcome(async () => subtle.importKey("spki", await spki(KEYS.rsa1024.private, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }), { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, false, ["verify"])),
    rsaGenerate: await outcome(() => subtle.generateKey({ name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, false, ["sign"])),
    rsaSha1: await outcome(() => subtle.importKey("jwk", publicJwk(KEYS.rsa2048.private), { name: "RSASSA-PKCS1-v1_5", hash: "SHA-1" }, false, ["verify"])),
    pssSalt0: await outcome(async () => subtle.verify({ name: "RSA-PSS", saltLength: 0 }, await subtle.importKey("jwk", publicJwk(KEYS.rsa2048.private), { name: "RSA-PSS", hash: "SHA-256" }, false, ["verify"]), unhex(SIGS.ps256), MSG)),
    ecdh: await outcome(() => subtle.generateKey({ name: "ECDH", namedCurve: "P-256" }, false, ["deriveBits"])),
    rsaOaep: await outcome(() => subtle.importKey("jwk", publicJwk(KEYS.rsa2048.private), { name: "RSA-OAEP", hash: "SHA-256" }, false, ["encrypt"])),
    deriveKey: await outcome(async () => subtle.deriveKey({ name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info: new Uint8Array(0) }, await subtle.importKey("raw", new Uint8Array(8), "HKDF", false, ["deriveKey"]), { name: "AES-GCM", length: 256 }, false, ["encrypt"])),
  };
  return { webcrypto: r, nextrsOnly };
}

exports.run = run;
exports.handler = async (req, res) => {
  const out = await run();
  res.setHeader("content-type", "application/json");
  res.end(JSON.stringify(out));
};

if (require.main === module) {
  run().then((out) => {
    console.log(JSON.stringify(out, null, 2));
  });
}
