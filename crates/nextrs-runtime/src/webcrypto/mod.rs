//! The parts of Web Crypto that Next.js and common authentication
//! libraries use, implemented with audited Rust libraries:
//!
//! * Secret keys ([`secret`]): AES-GCM (Next.js encrypts Server Action
//!   closure arguments with it), AES-CBC and HMAC (JWE's `A*CBC-HS*`
//!   content encryption, which Auth.js sessions use; HS256/384/512 JWTs),
//!   and HKDF (`deriveBits`; Auth.js derives its session key with it).
//!   Primitives: RustCrypto's `aes-gcm`, `aes`/`cbc`, `hmac`, `hkdf`.
//! * Asymmetric signatures ([`asymmetric`]): ECDSA on P-256 and P-384,
//!   RSASSA-PKCS1-v1_5, RSA-PSS and Ed25519, the algorithms of JWS
//!   (ES256/ES384, RS*, PS*, EdDSA), which JWKS-based authentication needs.
//!   Primitives: `ring` (BoringSSL's constant-time implementations; already
//!   linked by rustls). Key formats (`raw`, `spki`, `pkcs8`, `jwk`) are
//!   parsed and written with RustCrypto's `der`/`spki`/`pkcs8`/`pkcs1`/
//!   `sec1` crates, which do no cryptography.
//!
//! Key material lives here, in the isolate's [`CryptoKeys`], never in the
//! V8 heap: JavaScript holds an opaque handle (released when the
//! `CryptoKey` is garbage collected), secret bytes are zeroized when the key
//! goes away. Everything this module does not implement is refused
//! (`NotSupportedError`) rather than approximated; the JavaScript side
//! (`js/02_webcrypto.js`) checks usages and extractability before an
//! operation reaches Rust, and Rust checks again that the key's algorithm
//! and type fit the operation.
//!
//! Errors carry the DOMException name Web Crypto specifies as a prefix
//! (`OperationError: ...`), which JavaScript turns into a `DOMException`.
//! Messages never include key material, plaintext or signatures.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use deno_core::{OpState, op2};
use deno_error::JsErrorBox;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub(crate) mod asymmetric;
pub(crate) mod jwk;
pub(crate) mod secret;

/// Live keys per isolate. Keys are released when their `CryptoKey` is
/// garbage collected, so this only bounds a script that holds on to keys.
pub(crate) const MAX_KEYS_PER_ISOLATE: usize = 65_536;
/// Web Crypto bounds AES-GCM plaintext at 2^39 - 256 bits; NextRS bounds a
/// single operation far lower, since it runs on the isolate's thread.
pub(crate) const MAX_DATA_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn error(name: &str, message: &str) -> JsErrorBox {
    JsErrorBox::generic(format!("{name}: {message}"))
}

pub(crate) fn check_data_len(len: usize, what: &str) -> Result<(), JsErrorBox> {
    if len > MAX_DATA_BYTES {
        return Err(error(
            "OperationError",
            &format!("{what} input above {MAX_DATA_BYTES} bytes is not supported"),
        ));
    }
    Ok(())
}

/// A hash algorithm a key or an operation names (`algorithm.hash`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl Hash {
    pub(crate) fn parse(name: &str) -> Result<Self, JsErrorBox> {
        match name {
            "SHA-1" => Ok(Self::Sha1),
            "SHA-256" => Ok(Self::Sha256),
            "SHA-384" => Ok(Self::Sha384),
            "SHA-512" => Ok(Self::Sha512),
            _ => Err(error(
                "NotSupportedError",
                "the hash must be SHA-1, SHA-256, SHA-384 or SHA-512",
            )),
        }
    }

    /// The hash's block size in bits: Web Crypto's default HMAC key length.
    pub(crate) fn block_bits(self) -> u32 {
        match self {
            Self::Sha1 | Self::Sha256 => 512,
            Self::Sha384 | Self::Sha512 => 1024,
        }
    }

    /// The digest's length in bytes.
    pub(crate) fn len(self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
            Self::Sha384 => 48,
            Self::Sha512 => 64,
        }
    }
}

/// An elliptic curve (`namedCurve`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Curve {
    P256,
    P384,
}

impl Curve {
    pub(crate) fn parse(name: &str) -> Result<Self, JsErrorBox> {
        match name {
            "P-256" => Ok(Self::P256),
            "P-384" => Ok(Self::P384),
            "P-521" => Err(error(
                "NotSupportedError",
                "NextRS does not implement P-521 (ES512)",
            )),
            _ => Err(error("NotSupportedError", "unrecognized namedCurve")),
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::P256 => "P-256",
            Self::P384 => "P-384",
        }
    }

    /// The length of a coordinate or of the private scalar, in bytes.
    pub(crate) fn field_len(self) -> usize {
        match self {
            Self::P256 => 32,
            Self::P384 => 48,
        }
    }
}

/// The algorithm a key was created for; fixed for the key's life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Algorithm {
    AesGcm,
    AesCbc,
    Hmac(Hash),
    Hkdf,
    Ecdsa(Curve),
    RsassaPkcs1(Hash),
    RsaPss(Hash),
    Ed25519,
}

/// `algorithm` as JavaScript passes it: the name already normalized to its
/// registered spelling, the hash and curve to their names.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AlgorithmSpec {
    pub name: String,
    pub hash: Option<String>,
    pub named_curve: Option<String>,
    /// AES and HMAC key length in bits (`generateKey`; 0 = HMAC default).
    pub length: Option<u32>,
}

impl Algorithm {
    pub(crate) fn parse(spec: &AlgorithmSpec) -> Result<Self, JsErrorBox> {
        let hash = || {
            spec.hash
                .as_deref()
                .ok_or_else(|| error("TypeError", "the algorithm needs a hash"))
                .and_then(Hash::parse)
        };
        let curve = || {
            spec.named_curve
                .as_deref()
                .ok_or_else(|| error("TypeError", "ECDSA needs a namedCurve"))
                .and_then(Curve::parse)
        };
        Ok(match spec.name.as_str() {
            "AES-GCM" => Self::AesGcm,
            "AES-CBC" => Self::AesCbc,
            "HMAC" => Self::Hmac(hash()?),
            "HKDF" => Self::Hkdf,
            "ECDSA" => Self::Ecdsa(curve()?),
            "RSASSA-PKCS1-v1_5" => Self::RsassaPkcs1(rsa_hash(hash()?)?),
            "RSA-PSS" => Self::RsaPss(rsa_hash(hash()?)?),
            "Ed25519" => Self::Ed25519,
            _ => return Err(error("NotSupportedError", "unsupported algorithm")),
        })
    }
}

fn rsa_hash(hash: Hash) -> Result<Hash, JsErrorBox> {
    if hash == Hash::Sha1 {
        return Err(error(
            "NotSupportedError",
            "NextRS does not implement RSA signatures with SHA-1",
        ));
    }
    Ok(hash)
}

/// A key's material, by kind. Secret and private material is zeroized on
/// drop where NextRS holds the bytes; ring's own key objects keep their
/// internal copies until they are freed.
pub(crate) enum Material {
    Secret(Zeroizing<Vec<u8>>),
    EcPublic(asymmetric::EcPublic),
    EcPrivate(Box<asymmetric::EcPrivate>),
    RsaPublic(asymmetric::RsaPublic),
    RsaPrivate(Box<asymmetric::RsaPrivate>),
    EdPublic(asymmetric::EdPublic),
    EdPrivate(Box<asymmetric::EdPrivate>),
}

impl Material {
    fn kind(&self) -> &'static str {
        match self {
            Self::Secret(_) => "secret",
            Self::EcPublic(_) | Self::RsaPublic(_) | Self::EdPublic(_) => "public",
            Self::EcPrivate(_) | Self::RsaPrivate(_) | Self::EdPrivate(_) => "private",
        }
    }
}

pub(crate) struct StoredKey {
    pub algorithm: Algorithm,
    pub material: Material,
}

/// Keys alive in every isolate of the process (a soak gauge).
static LIVE_KEYS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn live_keys() -> u64 {
    LIVE_KEYS.load(Ordering::Relaxed)
}

// Counted while a key is in an isolate's store: added by `insert`, removed
// on release and when the isolate's store goes away.
impl Drop for CryptoKeys {
    fn drop(&mut self) {
        LIVE_KEYS.fetch_sub(self.keys.len() as u64, Ordering::Relaxed);
    }
}

/// Keys an isolate holds, by handle.
#[derive(Default)]
pub(crate) struct CryptoKeys {
    next_id: u32,
    keys: HashMap<u32, StoredKey>,
}

/// What JavaScript learns about a new key: its handle and the parts of
/// `CryptoKey.algorithm` only the key material determines.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeyInfo {
    pub handle: u32,
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modulus_length: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_exponent: Option<Vec<u8>>,
}

pub(crate) fn key_of(state: &OpState, id: u32) -> Result<&StoredKey, JsErrorBox> {
    state
        .borrow::<CryptoKeys>()
        .keys
        .get(&id)
        .ok_or_else(|| error("InvalidAccessError", "the CryptoKey is not usable"))
}

fn insert(state: &mut OpState, key: StoredKey) -> Result<KeyInfo, JsErrorBox> {
    let info = |handle: u32, key: &StoredKey| {
        let mut info = KeyInfo {
            handle,
            kind: key.material.kind(),
            length: None,
            modulus_length: None,
            public_exponent: None,
        };
        match &key.material {
            Material::Secret(bytes) => info.length = Some((bytes.len() * 8) as u32),
            Material::RsaPublic(public) => {
                info.modulus_length = Some(public.modulus_bits());
                info.public_exponent = Some(public.e.clone());
            }
            Material::RsaPrivate(private) => {
                info.modulus_length = Some(private.public.modulus_bits());
                info.public_exponent = Some(private.public.e.clone());
            }
            _ => {}
        }
        info
    };
    let keys = state.borrow_mut::<CryptoKeys>();
    if keys.keys.len() >= MAX_KEYS_PER_ISOLATE {
        return Err(error(
            "OperationError",
            &format!("this isolate already holds {MAX_KEYS_PER_ISOLATE} CryptoKeys"),
        ));
    }
    // Handles are never reused while a key is live.
    loop {
        keys.next_id = keys.next_id.wrapping_add(1);
        if keys.next_id != 0 && !keys.keys.contains_key(&keys.next_id) {
            break;
        }
    }
    let id = keys.next_id;
    let out = info(id, &key);
    LIVE_KEYS.fetch_add(1, Ordering::Relaxed);
    keys.keys.insert(id, key);
    Ok(out)
}

/// `importKey(format, keyData, algorithm, ...)` for `raw`, `spki` and
/// `pkcs8`; `jwk` goes through [`op_nextrs_key_import_jwk`].
#[op2]
#[serde]
pub(crate) fn op_nextrs_key_import(
    state: &mut OpState,
    #[string] format: &str,
    #[serde] spec: AlgorithmSpec,
    #[buffer] data: &[u8],
) -> Result<KeyInfo, JsErrorBox> {
    check_data_len(data.len(), "key")?;
    let algorithm = Algorithm::parse(&spec)?;
    let material = match algorithm {
        Algorithm::AesGcm | Algorithm::AesCbc | Algorithm::Hmac(_) | Algorithm::Hkdf => {
            if format != "raw" {
                return Err(error(
                    "NotSupportedError",
                    "NextRS imports secret keys from 'raw' and 'jwk' only",
                ));
            }
            secret::import(algorithm, data)?
        }
        Algorithm::Ecdsa(curve) => asymmetric::import_ec(curve, format, data)?,
        Algorithm::RsassaPkcs1(_) | Algorithm::RsaPss(_) => asymmetric::import_rsa(format, data)?,
        Algorithm::Ed25519 => asymmetric::import_ed25519(format, data)?,
    };
    insert(
        state,
        StoredKey {
            algorithm,
            material,
        },
    )
}

/// `importKey("jwk", jwk, algorithm, ...)`. JavaScript has already checked
/// the members Web Crypto checks against the request (`kty`, `alg`, `use`,
/// `key_ops`, `ext`); this decodes and validates the key material.
#[op2]
#[serde]
pub(crate) fn op_nextrs_key_import_jwk(
    state: &mut OpState,
    #[serde] spec: AlgorithmSpec,
    #[serde] jwk: jwk::Jwk,
) -> Result<KeyInfo, JsErrorBox> {
    let algorithm = Algorithm::parse(&spec)?;
    let material = jwk::import(algorithm, &jwk)?;
    insert(
        state,
        StoredKey {
            algorithm,
            material,
        },
    )
}

/// `generateKey(algorithm, ...)`: one key for a secret algorithm, the
/// public key then the private key for a signature algorithm.
#[op2]
#[serde]
pub(crate) fn op_nextrs_key_generate(
    state: &mut OpState,
    #[serde] spec: AlgorithmSpec,
) -> Result<Vec<KeyInfo>, JsErrorBox> {
    let algorithm = Algorithm::parse(&spec)?;
    let materials = match algorithm {
        Algorithm::AesGcm | Algorithm::AesCbc | Algorithm::Hmac(_) => {
            vec![secret::generate(algorithm, spec.length.unwrap_or(0))?]
        }
        Algorithm::Hkdf => {
            return Err(error("NotSupportedError", "HKDF keys cannot be generated"));
        }
        Algorithm::Ecdsa(curve) => {
            let (public, private) = asymmetric::generate_ec(curve)?;
            vec![public, private]
        }
        Algorithm::Ed25519 => {
            let (public, private) = asymmetric::generate_ed25519()?;
            vec![public, private]
        }
        Algorithm::RsassaPkcs1(_) | Algorithm::RsaPss(_) => {
            return Err(error(
                "NotSupportedError",
                "NextRS does not generate RSA keys (import them instead)",
            ));
        }
    };
    materials
        .into_iter()
        .map(|material| {
            insert(
                state,
                StoredKey {
                    algorithm,
                    material,
                },
            )
        })
        .collect()
}

/// `exportKey(format, key)` for `raw`, `spki` and `pkcs8`. JavaScript
/// checks `extractable` first; this checks that the format fits the key.
#[op2]
#[buffer]
pub(crate) fn op_nextrs_key_export(
    state: &mut OpState,
    #[smi] id: u32,
    #[string] format: &str,
) -> Result<Vec<u8>, JsErrorBox> {
    let key = key_of(state, id)?;
    match (&key.material, format) {
        (Material::Secret(bytes), "raw") => {
            if key.algorithm == Algorithm::Hkdf {
                return Err(error("InvalidAccessError", "HKDF keys are not extractable"));
            }
            Ok(bytes.to_vec())
        }
        (Material::Secret(_), _) => Err(error(
            "NotSupportedError",
            "secret keys export as 'raw' or 'jwk'",
        )),
        _ => asymmetric::export(&key.material, format),
    }
}

/// `exportKey("jwk", key)`: the key material members; JavaScript adds
/// `alg`, `key_ops` and `ext`.
#[op2]
#[serde]
pub(crate) fn op_nextrs_key_export_jwk(
    state: &mut OpState,
    #[smi] id: u32,
) -> Result<jwk::Jwk, JsErrorBox> {
    let key = key_of(state, id)?;
    if key.algorithm == Algorithm::Hkdf {
        return Err(error("InvalidAccessError", "HKDF keys are not extractable"));
    }
    Ok(jwk::export(&key.material))
}

/// Releases a key (its `CryptoKey` was garbage collected).
#[op2(fast)]
pub(crate) fn op_nextrs_key_release(state: &mut OpState, #[smi] id: u32) {
    if state.borrow_mut::<CryptoKeys>().keys.remove(&id).is_some() {
        LIVE_KEYS.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The parameters of `sign`/`verify` besides the key.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SignSpec {
    pub name: String,
    /// ECDSA's hash (`{ name: "ECDSA", hash }`).
    pub hash: Option<String>,
    /// RSA-PSS's salt length in bytes.
    pub salt_length: Option<u32>,
}

/// `sign(algorithm, key, data)`: HMAC, ECDSA, RSASSA-PKCS1-v1_5, RSA-PSS,
/// Ed25519.
#[op2]
#[buffer]
pub(crate) fn op_nextrs_sign(
    state: &mut OpState,
    #[smi] id: u32,
    #[serde] spec: SignSpec,
    #[buffer] data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    check_data_len(data.len(), "sign")?;
    let key = key_of(state, id)?;
    match (&key.material, key.algorithm) {
        (Material::Secret(bytes), Algorithm::Hmac(hash)) if spec.name == "HMAC" => {
            Ok(secret::hmac_sign(hash, bytes, data))
        }
        (Material::Secret(_), _) => Err(error(
            "InvalidAccessError",
            "the key cannot sign with this algorithm",
        )),
        _ => asymmetric::sign(key, &spec, data),
    }
}

/// `verify(algorithm, key, signature, data)`. A signature that does not
/// verify, including one of the wrong length, is `false`.
#[op2]
pub(crate) fn op_nextrs_verify(
    state: &mut OpState,
    #[smi] id: u32,
    #[serde] spec: SignSpec,
    #[buffer] signature: &[u8],
    #[buffer] data: &[u8],
) -> Result<bool, JsErrorBox> {
    check_data_len(data.len(), "verify")?;
    let key = key_of(state, id)?;
    match (&key.material, key.algorithm) {
        (Material::Secret(bytes), Algorithm::Hmac(hash)) if spec.name == "HMAC" => {
            Ok(secret::hmac_verify(hash, bytes, signature, data))
        }
        (Material::Secret(_), _) => Err(error(
            "InvalidAccessError",
            "the key cannot verify with this algorithm",
        )),
        _ => asymmetric::verify(key, &spec, signature, data),
    }
}

/// AES-GCM encrypt (`encrypt == true`) or decrypt with a 128-bit tag.
/// Ciphertext is `ciphertext || tag`, as Web Crypto returns it.
#[op2]
#[buffer]
pub(crate) fn op_nextrs_aes_gcm(
    state: &mut OpState,
    #[smi] id: u32,
    encrypt: bool,
    #[buffer] iv: &[u8],
    #[buffer] additional_data: &[u8],
    #[buffer] data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    check_data_len(data.len(), "AES-GCM")?;
    let key = key_of(state, id)?;
    let (Algorithm::AesGcm, Material::Secret(bytes)) = (key.algorithm, &key.material) else {
        return Err(error("InvalidAccessError", "the key is not an AES-GCM key"));
    };
    secret::aes_gcm(bytes, encrypt, iv, additional_data, data)
}

/// AES-CBC encrypt or decrypt with PKCS#7 padding (Web Crypto's AES-CBC).
#[op2]
#[buffer]
pub(crate) fn op_nextrs_aes_cbc(
    state: &mut OpState,
    #[smi] id: u32,
    encrypt: bool,
    #[buffer] iv: &[u8],
    #[buffer] data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    check_data_len(data.len(), "AES-CBC")?;
    let key = key_of(state, id)?;
    let (Algorithm::AesCbc, Material::Secret(bytes)) = (key.algorithm, &key.material) else {
        return Err(error("InvalidAccessError", "the key is not an AES-CBC key"));
    };
    secret::aes_cbc(bytes, encrypt, iv, data)
}

/// `deriveBits({ name: "HKDF", hash, salt, info }, key, length)`.
#[op2]
#[buffer]
pub(crate) fn op_nextrs_hkdf(
    state: &mut OpState,
    #[smi] id: u32,
    #[string] hash: &str,
    #[buffer] salt: &[u8],
    #[buffer] info: &[u8],
    #[smi] bits: u32,
) -> Result<Vec<u8>, JsErrorBox> {
    check_data_len(salt.len() + info.len(), "HKDF")?;
    let hash = Hash::parse(hash)?;
    let key = key_of(state, id)?;
    let (Algorithm::Hkdf, Material::Secret(ikm)) = (key.algorithm, &key.material) else {
        return Err(error("InvalidAccessError", "the key is not an HKDF key"));
    };
    secret::hkdf(hash, ikm, salt, info, bits)
}

/// node:crypto's `hkdf`/`hkdfSync`: HKDF over key bytes JavaScript passes
/// (Node takes the input keying material directly, not a CryptoKey).
/// `len` is in bytes.
#[op2]
#[buffer]
pub(crate) fn op_nextrs_hkdf_bytes(
    #[string] hash: &str,
    #[buffer] ikm: &[u8],
    #[buffer] salt: &[u8],
    #[buffer] info: &[u8],
    #[smi] len: u32,
) -> Result<Vec<u8>, JsErrorBox> {
    check_data_len(ikm.len() + salt.len() + info.len(), "HKDF")?;
    let hash = Hash::parse(hash)?;
    secret::hkdf(hash, ikm, salt, info, len.saturating_mul(8))
}

/// node:crypto's `pbkdf2Sync(password, salt, iterations, keylen, digest)`
/// (`digest` already normalized to `sha1`, `sha256`, `sha384`, `sha512`).
/// Next.js derives its preview-data key with it.
#[op2]
#[buffer]
pub(crate) fn op_nextrs_pbkdf2(
    #[string] digest: &str,
    #[buffer] password: &[u8],
    #[buffer] salt: &[u8],
    iterations: u32,
    #[smi] keylen: u32,
) -> Result<Vec<u8>, JsErrorBox> {
    check_data_len(password.len() + salt.len(), "PBKDF2")?;
    let algorithm = match digest {
        "sha1" => ring::pbkdf2::PBKDF2_HMAC_SHA1,
        "sha256" => ring::pbkdf2::PBKDF2_HMAC_SHA256,
        "sha384" => ring::pbkdf2::PBKDF2_HMAC_SHA384,
        "sha512" => ring::pbkdf2::PBKDF2_HMAC_SHA512,
        _ => return Err(error("NotSupportedError", "unsupported PBKDF2 digest")),
    };
    let iterations = std::num::NonZeroU32::new(iterations)
        .ok_or_else(|| error("OperationError", "PBKDF2 needs at least one iteration"))?;
    // node:crypto caps keylen at 2^31 - 1; NextRS at the data limit.
    if keylen as usize > MAX_DATA_BYTES {
        return Err(error("OperationError", "PBKDF2 key length is too large"));
    }
    let mut out = vec![0u8; keylen as usize];
    ring::pbkdf2::derive(algorithm, iterations, salt, password, &mut out);
    Ok(out)
}

/// node:crypto's AES-GCM cipher and decipher (`createCipheriv`), with a raw
/// key: `data` in, `ciphertext || tag` out when encrypting; `ciphertext ||
/// tag` in, plaintext out when decrypting (a wrong tag is an error).
#[op2]
#[buffer]
pub(crate) fn op_nextrs_node_aes_gcm(
    #[buffer] key: &[u8],
    encrypt: bool,
    #[buffer] iv: &[u8],
    #[buffer] aad: &[u8],
    #[buffer] data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    check_data_len(data.len() + aad.len(), "AES-GCM")?;
    secret::aes_gcm(key, encrypt, iv, aad, data)
}
