//! The parts of Web Crypto that Next.js and common session libraries use:
//!
//! * AES-GCM secret keys (`importKey("raw")`, `generateKey`,
//!   `exportKey("raw")`) and `encrypt`/`decrypt` with them. Next encrypts
//!   the bound arguments of closures it sends to the browser with the
//!   build's AES-256 key and a 16-byte random IV (`encryption.ts`);
//!   decrypting a tampered payload must fail authentication.
//! * HMAC secret keys (SHA-1/256/384/512) and `sign`/`verify` with them:
//!   `jose` signs and verifies HS256/384/512 JWTs this way (the session
//!   cookies of the Next.js authentication guide). Verification compares in
//!   constant time.
//!
//! The primitives are RustCrypto's `hmac` and `aes-gcm` (audited, constant-time AES and
//! GHASH in software or AES-NI/CLMUL), which unlike `ring` accepts the
//! 128-bit nonces Next uses. Key bytes live here, in the isolate's
//! `CryptoKeys`, never in the V8 heap: JavaScript holds an opaque handle,
//! and the bytes are zeroized when the key is released or the isolate goes
//! away. Only 128-bit tags (the Web Crypto default) and 96- or 128-bit IVs
//! are accepted; anything else is refused rather than approximated.
//!
//! Errors carry the DOMException name Web Crypto specifies as a prefix
//! (`OperationError: ...`), which the JavaScript side turns into a
//! `DOMException`. Messages never include key material or plaintext.

use std::collections::HashMap;

use aes_gcm::AesGcm;
use aes_gcm::aead::consts::{U12, U16};
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::aes::{Aes128, Aes192, Aes256};
use deno_core::{OpState, op2};
use deno_error::JsErrorBox;
use hmac::{Hmac, Mac};
use zeroize::Zeroizing;

/// Live keys per isolate. Keys are released when their `CryptoKey` is
/// garbage collected, so this only bounds a script that holds on to keys.
pub(crate) const MAX_KEYS_PER_ISOLATE: usize = 65_536;
/// Web Crypto bounds AES-GCM plaintext at 2^39 - 256 bits; NextRS bounds a
/// single operation far lower, since it runs on the isolate's thread.
pub(crate) const MAX_DATA_BYTES: usize = 64 * 1024 * 1024;

/// Secret keys an isolate holds, by handle.
#[derive(Default)]
pub(crate) struct CryptoKeys {
    next_id: u32,
    keys: HashMap<u32, StoredKey>,
}

/// The hash of an HMAC key (`algorithm.hash`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HmacHash {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

impl HmacHash {
    fn parse(name: &str) -> Result<Self, JsErrorBox> {
        match name {
            "SHA-1" => Ok(Self::Sha1),
            "SHA-256" => Ok(Self::Sha256),
            "SHA-384" => Ok(Self::Sha384),
            "SHA-512" => Ok(Self::Sha512),
            _ => Err(error(
                "NotSupportedError",
                "HMAC hash must be SHA-1, SHA-256, SHA-384 or SHA-512",
            )),
        }
    }

    /// The hash's block size in bits: Web Crypto's default HMAC key length.
    fn block_bits(self) -> u32 {
        match self {
            Self::Sha1 | Self::Sha256 => 512,
            Self::Sha384 | Self::Sha512 => 1024,
        }
    }
}

/// What a key may be used for; checked on every operation, besides the
/// checks JavaScript makes on the `CryptoKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    Aes,
    Hmac(HmacHash),
}

struct StoredKey {
    kind: KeyKind,
    bytes: Zeroizing<Vec<u8>>,
}

fn key_of(state: &OpState, id: u32) -> Result<(KeyKind, &[u8]), JsErrorBox> {
    state
        .borrow::<CryptoKeys>()
        .keys
        .get(&id)
        .map(|key| (key.kind, key.bytes.as_slice()))
        .ok_or_else(|| error("InvalidAccessError", "the CryptoKey is not usable"))
}

fn check_data_len(len: usize, what: &str) -> Result<(), JsErrorBox> {
    if len > MAX_DATA_BYTES {
        return Err(error(
            "OperationError",
            &format!("{what} input above {MAX_DATA_BYTES} bytes is not supported"),
        ));
    }
    Ok(())
}

fn error(name: &str, message: &str) -> JsErrorBox {
    JsErrorBox::generic(format!("{name}: {message}"))
}

fn check_aes_length(len: usize) -> Result<(), JsErrorBox> {
    match len {
        16 | 24 | 32 => Ok(()),
        _ => Err(error(
            "DataError",
            "AES key data must be 128, 192 or 256 bits long",
        )),
    }
}

fn insert(state: &mut OpState, kind: KeyKind, key: Zeroizing<Vec<u8>>) -> Result<u32, JsErrorBox> {
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
    keys.keys.insert(id, StoredKey { kind, bytes: key });
    Ok(id)
}

/// `importKey("raw", data, "AES-GCM", ...)`: copies the key bytes into the
/// host and returns a handle.
#[op2(fast)]
#[smi]
pub(crate) fn op_nextrs_aes_import(
    state: &mut OpState,
    #[buffer] data: &[u8],
) -> Result<u32, JsErrorBox> {
    check_aes_length(data.len())?;
    insert(state, KeyKind::Aes, Zeroizing::new(data.to_vec()))
}

/// `generateKey({ name: "AES-GCM", length })`.
#[op2(fast)]
#[smi]
pub(crate) fn op_nextrs_aes_generate(
    state: &mut OpState,
    #[smi] bits: u32,
) -> Result<u32, JsErrorBox> {
    let len = match bits {
        128 | 192 | 256 => (bits / 8) as usize,
        _ => {
            return Err(error(
                "OperationError",
                "AES key length must be 128, 192 or 256 bits",
            ));
        }
    };
    let mut key = Zeroizing::new(vec![0u8; len]);
    getrandom::fill(&mut key)
        .map_err(|e| error("OperationError", &format!("getrandom failed: {e}")))?;
    insert(state, KeyKind::Aes, key)
}

/// `exportKey("raw", key)` for any secret key; the JavaScript side checks
/// `extractable`.
#[op2]
#[buffer]
pub(crate) fn op_nextrs_key_export(
    state: &mut OpState,
    #[smi] id: u32,
) -> Result<Vec<u8>, JsErrorBox> {
    let (_, bytes) = key_of(state, id)?;
    Ok(bytes.to_vec())
}

/// Releases a key (its `CryptoKey` was garbage collected).
#[op2(fast)]
pub(crate) fn op_nextrs_key_release(state: &mut OpState, #[smi] id: u32) {
    state.borrow_mut::<CryptoKeys>().keys.remove(&id);
}

/// `importKey("raw", data, { name: "HMAC", hash }, ...)`. The key's length
/// is its data's length (JavaScript refuses other `length` values).
#[op2(fast)]
#[smi]
pub(crate) fn op_nextrs_hmac_import(
    state: &mut OpState,
    #[string] hash: &str,
    #[buffer] data: &[u8],
) -> Result<u32, JsErrorBox> {
    let hash = HmacHash::parse(hash)?;
    if data.is_empty() {
        return Err(error("DataError", "HMAC key data must not be empty"));
    }
    check_data_len(data.len(), "HMAC key")?;
    insert(state, KeyKind::Hmac(hash), Zeroizing::new(data.to_vec()))
}

/// `generateKey({ name: "HMAC", hash, length })`; `bits == 0` means the
/// hash's block size. Returns the handle and the key's length in bits.
#[op2]
#[serde]
pub(crate) fn op_nextrs_hmac_generate(
    state: &mut OpState,
    #[string] hash: &str,
    #[smi] bits: u32,
) -> Result<(u32, u32), JsErrorBox> {
    let hash = HmacHash::parse(hash)?;
    let bits = if bits == 0 { hash.block_bits() } else { bits };
    if bits % 8 != 0 || bits > 8192 {
        return Err(error(
            "NotSupportedError",
            "NextRS generates HMAC keys of a whole number of bytes, up to 8192 bits",
        ));
    }
    let mut key = Zeroizing::new(vec![0u8; (bits / 8) as usize]);
    getrandom::fill(&mut key)
        .map_err(|e| error("OperationError", &format!("getrandom failed: {e}")))?;
    Ok((insert(state, KeyKind::Hmac(hash), key)?, bits))
}

/// `sign({ name: "HMAC" }, key, data)`: the MAC.
#[op2]
#[buffer]
pub(crate) fn op_nextrs_hmac_sign(
    state: &mut OpState,
    #[smi] id: u32,
    #[buffer] data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    check_data_len(data.len(), "HMAC")?;
    let (kind, key) = key_of(state, id)?;
    let KeyKind::Hmac(hash) = kind else {
        return Err(error("InvalidAccessError", "the key is not an HMAC key"));
    };
    Ok(hmac_sign(hash, key, data))
}

/// `verify({ name: "HMAC" }, key, signature, data)`, in constant time.
#[op2(fast)]
pub(crate) fn op_nextrs_hmac_verify(
    state: &mut OpState,
    #[smi] id: u32,
    #[buffer] signature: &[u8],
    #[buffer] data: &[u8],
) -> Result<bool, JsErrorBox> {
    check_data_len(data.len(), "HMAC")?;
    let (kind, key) = key_of(state, id)?;
    let KeyKind::Hmac(hash) = kind else {
        return Err(error("InvalidAccessError", "the key is not an HMAC key"));
    };
    Ok(hmac_verify(hash, key, signature, data))
}

/// Runs `$body` with `$mac` an `Hmac<hash>` keyed with `$key`.
macro_rules! with_hmac {
    ($hash:expr, $key:expr, |$mac:ident| $body:expr) => {
        match $hash {
            HmacHash::Sha1 => {
                let mut $mac = <Hmac<sha1::Sha1> as KeyInit>::new_from_slice($key)
                    .expect("HMAC takes keys of any length");
                $body
            }
            HmacHash::Sha256 => {
                let mut $mac = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice($key)
                    .expect("HMAC takes keys of any length");
                $body
            }
            HmacHash::Sha384 => {
                let mut $mac = <Hmac<sha2::Sha384> as KeyInit>::new_from_slice($key)
                    .expect("HMAC takes keys of any length");
                $body
            }
            HmacHash::Sha512 => {
                let mut $mac = <Hmac<sha2::Sha512> as KeyInit>::new_from_slice($key)
                    .expect("HMAC takes keys of any length");
                $body
            }
        }
    };
}

pub(crate) fn hmac_sign(hash: HmacHash, key: &[u8], data: &[u8]) -> Vec<u8> {
    with_hmac!(hash, key, |mac| {
        mac.update(data);
        mac.finalize().into_bytes().to_vec()
    })
}

/// A signature of the wrong length is `false`, as Web Crypto says; the
/// comparison itself is constant-time (`verify_slice`).
pub(crate) fn hmac_verify(hash: HmacHash, key: &[u8], signature: &[u8], data: &[u8]) -> bool {
    with_hmac!(hash, key, |mac| {
        mac.update(data);
        mac.verify_slice(signature).is_ok()
    })
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
    let (kind, key) = key_of(state, id)?;
    if kind != KeyKind::Aes {
        return Err(error("InvalidAccessError", "the key is not an AES-GCM key"));
    }
    aes_gcm(key, encrypt, iv, additional_data, data)
}

pub(crate) fn aes_gcm(
    key: &[u8],
    encrypt: bool,
    iv: &[u8],
    aad: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    match (key.len(), iv.len()) {
        (16, 12) => run::<Aes128, U12>(key, encrypt, iv, aad, data),
        (16, 16) => run::<Aes128, U16>(key, encrypt, iv, aad, data),
        (24, 12) => run::<Aes192, U12>(key, encrypt, iv, aad, data),
        (24, 16) => run::<Aes192, U16>(key, encrypt, iv, aad, data),
        (32, 12) => run::<Aes256, U12>(key, encrypt, iv, aad, data),
        (32, 16) => run::<Aes256, U16>(key, encrypt, iv, aad, data),
        (16 | 24 | 32, 0) => Err(error("OperationError", "the AES-GCM iv must not be empty")),
        (16 | 24 | 32, _) => Err(error(
            "NotSupportedError",
            "NextRS supports AES-GCM ivs of 12 or 16 bytes only",
        )),
        _ => Err(error("OperationError", "invalid AES key length")),
    }
}

fn run<C, N>(
    key: &[u8],
    encrypt: bool,
    iv: &[u8],
    aad: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, JsErrorBox>
where
    C: aes_gcm::aes::cipher::BlockCipher
        + aes_gcm::aes::cipher::BlockSizeUser<BlockSize = U16>
        + aes_gcm::aes::cipher::BlockEncrypt
        + KeyInit,
    N: aes_gcm::aead::generic_array::ArrayLength<u8>,
{
    let cipher = AesGcm::<C, N>::new_from_slice(key)
        .map_err(|_| error("OperationError", "invalid AES key length"))?;
    let nonce = GenericArray::from_slice(iv);
    let payload = Payload { msg: data, aad };
    if encrypt {
        cipher
            .encrypt(nonce, payload)
            .map_err(|_| error("OperationError", "AES-GCM encryption failed"))
    } else {
        if data.len() < 16 {
            return Err(error(
                "OperationError",
                "the AES-GCM ciphertext is shorter than its tag",
            ));
        }
        // One message for every failure: it reveals nothing about why.
        cipher
            .decrypt(nonce, payload)
            .map_err(|_| error("OperationError", "AES-GCM decryption failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::{HmacHash, aes_gcm, hmac_sign, hmac_verify};

    /// RFC 4231 test cases 1, 2 and 6 (SHA-256 and SHA-512), and the
    /// RFC 2202 case 2 for SHA-1.
    #[test]
    fn hmac_rfc_vectors() {
        let cases: &[(HmacHash, Vec<u8>, &[u8], &str)] = &[
            (
                HmacHash::Sha256,
                vec![0x0b; 20],
                b"Hi There",
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            ),
            (
                HmacHash::Sha512,
                vec![0x0b; 20],
                b"Hi There",
                "87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cdedaa833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854",
            ),
            (
                HmacHash::Sha256,
                b"Jefe".to_vec(),
                b"what do ya want for nothing?",
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                HmacHash::Sha384,
                b"Jefe".to_vec(),
                b"what do ya want for nothing?",
                "af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649",
            ),
            (
                HmacHash::Sha256,
                vec![0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First",
                "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
            ),
            (
                HmacHash::Sha1,
                b"Jefe".to_vec(),
                b"what do ya want for nothing?",
                "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79",
            ),
        ];
        for (hash, key, data, expected) in cases {
            let mac = hmac_sign(*hash, key, data);
            assert_eq!(mac, hex(expected), "{hash:?}");
            assert!(hmac_verify(*hash, key, &mac, data));
            let mut bad = mac.clone();
            bad[0] ^= 1;
            assert!(!hmac_verify(*hash, key, &bad, data));
            assert!(!hmac_verify(*hash, key, &mac[..mac.len() - 1], data));
            assert!(!hmac_verify(*hash, b"other key", &mac, data));
        }
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// McGrew & Viega, "The Galois/Counter Mode of Operation", test case 16
    /// (AES-256, 96-bit IV, with additional data).
    #[test]
    fn gcm_spec_test_case_16() {
        let key = hex("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308");
        let iv = hex("cafebabefacedbaddecaf888");
        let pt = hex(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a721c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
        );
        let aad = hex("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let ct = hex(
            "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662",
        );
        let tag = hex("76fc6ece0f4e1768cddf8853bb2d551b");
        let out = aes_gcm(&key, true, &iv, &aad, &pt).unwrap();
        assert_eq!(out, [ct.clone(), tag.clone()].concat());
        let back = aes_gcm(&key, false, &iv, &aad, &out).unwrap();
        assert_eq!(back, pt);
    }

    /// Test case 14: AES-256, zero key and IV, one zero block.
    #[test]
    fn gcm_spec_test_case_14() {
        let key = vec![0u8; 32];
        let iv = vec![0u8; 12];
        let pt = vec![0u8; 16];
        let out = aes_gcm(&key, true, &iv, &[], &pt).unwrap();
        assert_eq!(
            out,
            hex("cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919")
        );
    }

    #[test]
    fn sixteen_byte_iv_round_trips_and_detects_tampering() {
        let key = [7u8; 32];
        let iv = [9u8; 16];
        let ct = aes_gcm(&key, true, &iv, &[], b"actionid-and-args").unwrap();
        assert_eq!(ct.len(), 17 + 16);
        assert_eq!(
            aes_gcm(&key, false, &iv, &[], &ct).unwrap(),
            b"actionid-and-args"
        );
        for i in [0, ct.len() / 2, ct.len() - 1] {
            let mut bad = ct.clone();
            bad[i] ^= 1;
            assert!(aes_gcm(&key, false, &iv, &[], &bad).is_err());
        }
        let mut wrong_iv = iv;
        wrong_iv[0] ^= 1;
        assert!(aes_gcm(&key, false, &wrong_iv, &[], &ct).is_err());
        assert!(aes_gcm(&[8u8; 32], false, &iv, &[], &ct).is_err());
        assert!(aes_gcm(&key, false, &iv, &[], &ct[..15]).is_err());
    }

    #[test]
    fn unsupported_parameters_are_refused() {
        assert!(aes_gcm(&[0u8; 32], true, &[0u8; 8], &[], b"x").is_err());
        assert!(aes_gcm(&[0u8; 32], true, &[], &[], b"x").is_err());
        assert!(aes_gcm(&[0u8; 20], true, &[0u8; 12], &[], b"x").is_err());
    }
}
