//! The part of Web Crypto that Next.js Server Actions use: AES-GCM secret
//! keys (`importKey("raw")`, `generateKey`, `exportKey("raw")`) and
//! `encrypt`/`decrypt` with them. Next encrypts the bound arguments of
//! closures it sends to the browser with the build's AES-256 key and a
//! 16-byte random IV (`encryption.ts`); decrypting a tampered payload must
//! fail authentication.
//!
//! The primitive is RustCrypto's `aes-gcm` (audited, constant-time AES and
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
    keys: HashMap<u32, Zeroizing<Vec<u8>>>,
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

fn insert(state: &mut OpState, key: Zeroizing<Vec<u8>>) -> Result<u32, JsErrorBox> {
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
    keys.keys.insert(id, key);
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
    insert(state, Zeroizing::new(data.to_vec()))
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
    insert(state, key)
}

/// `exportKey("raw", key)`; the JavaScript side checks `extractable`.
#[op2]
#[buffer]
pub(crate) fn op_nextrs_aes_export(
    state: &mut OpState,
    #[smi] id: u32,
) -> Result<Vec<u8>, JsErrorBox> {
    let keys = state.borrow::<CryptoKeys>();
    let key = keys
        .keys
        .get(&id)
        .ok_or_else(|| error("InvalidAccessError", "the CryptoKey is not usable"))?;
    Ok(key.to_vec())
}

/// Releases a key (its `CryptoKey` was garbage collected).
#[op2(fast)]
pub(crate) fn op_nextrs_aes_release(state: &mut OpState, #[smi] id: u32) {
    state.borrow_mut::<CryptoKeys>().keys.remove(&id);
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
    if data.len() > MAX_DATA_BYTES {
        return Err(error(
            "OperationError",
            &format!("AES-GCM input above {MAX_DATA_BYTES} bytes is not supported"),
        ));
    }
    let keys = state.borrow::<CryptoKeys>();
    let key = keys
        .keys
        .get(&id)
        .ok_or_else(|| error("InvalidAccessError", "the CryptoKey is not usable"))?;
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
    use super::aes_gcm;

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
