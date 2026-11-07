//! Secret-key algorithms: AES-GCM, AES-CBC, HMAC and HKDF.
//!
//! AES-GCM uses RustCrypto's `aes-gcm` (constant-time AES and GHASH in
//! software or AES-NI/CLMUL), which unlike `ring` accepts the 128-bit
//! nonces Next.js uses; only 128-bit tags (the Web Crypto default) and 96-
//! or 128-bit IVs are accepted. AES-CBC (PKCS#7 padding, 128-bit IV) uses
//! `aes` + `cbc`; JWE's `A*CBC-HS*` authenticates with HMAC before it
//! decrypts, which the library does, not Web Crypto. HMAC verification is
//! constant-time. HKDF follows RFC 5869 (`hkdf`).

use aes_gcm::AesGcm;
use aes_gcm::aead::consts::{U12, U16};
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::aes::{Aes128, Aes192, Aes256};
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use deno_error::JsErrorBox;
use hmac::{Hmac, Mac};
use zeroize::Zeroizing;

use super::{Algorithm, Hash, Material, check_data_len, error};

fn check_aes_length(len: usize) -> Result<(), JsErrorBox> {
    match len {
        16 | 24 | 32 => Ok(()),
        _ => Err(error(
            "DataError",
            "AES key data must be 128, 192 or 256 bits long",
        )),
    }
}

/// `importKey("raw")` (and the `k` of a JWK) for a secret algorithm.
pub(crate) fn import(algorithm: Algorithm, data: &[u8]) -> Result<Material, JsErrorBox> {
    match algorithm {
        Algorithm::AesGcm | Algorithm::AesCbc => check_aes_length(data.len())?,
        Algorithm::Hmac(_) => {
            if data.is_empty() {
                return Err(error("DataError", "HMAC key data must not be empty"));
            }
            check_data_len(data.len(), "HMAC key")?;
        }
        Algorithm::Hkdf => check_data_len(data.len(), "HKDF key")?,
        _ => return Err(error("NotSupportedError", "not a secret-key algorithm")),
    }
    Ok(Material::Secret(Zeroizing::new(data.to_vec())))
}

/// `generateKey` for AES (`bits` 128/192/256) or HMAC (`bits == 0` means
/// the hash's block size).
pub(crate) fn generate(algorithm: Algorithm, bits: u32) -> Result<Material, JsErrorBox> {
    let bits = match algorithm {
        Algorithm::AesGcm | Algorithm::AesCbc => match bits {
            128 | 192 | 256 => bits,
            _ => {
                return Err(error(
                    "OperationError",
                    "AES key length must be 128, 192 or 256 bits",
                ));
            }
        },
        Algorithm::Hmac(hash) => {
            let bits = if bits == 0 { hash.block_bits() } else { bits };
            if bits % 8 != 0 || bits > 8192 {
                return Err(error(
                    "NotSupportedError",
                    "NextRS generates HMAC keys of a whole number of bytes, up to 8192 bits",
                ));
            }
            bits
        }
        _ => return Err(error("NotSupportedError", "not a secret-key algorithm")),
    };
    let mut key = Zeroizing::new(vec![0u8; (bits / 8) as usize]);
    getrandom::fill(&mut key)
        .map_err(|e| error("OperationError", &format!("getrandom failed: {e}")))?;
    Ok(Material::Secret(key))
}

/// Runs `$body` with `$mac` an `Hmac<hash>` keyed with `$key`.
macro_rules! with_hmac {
    ($hash:expr, $key:expr, |$mac:ident| $body:expr) => {
        match $hash {
            Hash::Sha1 => {
                let mut $mac = <Hmac<sha1::Sha1> as KeyInit>::new_from_slice($key)
                    .expect("HMAC takes keys of any length");
                $body
            }
            Hash::Sha256 => {
                let mut $mac = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice($key)
                    .expect("HMAC takes keys of any length");
                $body
            }
            Hash::Sha384 => {
                let mut $mac = <Hmac<sha2::Sha384> as KeyInit>::new_from_slice($key)
                    .expect("HMAC takes keys of any length");
                $body
            }
            Hash::Sha512 => {
                let mut $mac = <Hmac<sha2::Sha512> as KeyInit>::new_from_slice($key)
                    .expect("HMAC takes keys of any length");
                $body
            }
        }
    };
}

pub(crate) fn hmac_sign(hash: Hash, key: &[u8], data: &[u8]) -> Vec<u8> {
    with_hmac!(hash, key, |mac| {
        mac.update(data);
        mac.finalize().into_bytes().to_vec()
    })
}

/// A signature of the wrong length is `false`, as Web Crypto says; the
/// comparison itself is constant-time (`verify_slice`).
pub(crate) fn hmac_verify(hash: Hash, key: &[u8], signature: &[u8], data: &[u8]) -> bool {
    with_hmac!(hash, key, |mac| {
        mac.update(data);
        mac.verify_slice(signature).is_ok()
    })
}

pub(crate) fn aes_gcm(
    key: &[u8],
    encrypt: bool,
    iv: &[u8],
    aad: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    match (key.len(), iv.len()) {
        (16, 12) => gcm::<Aes128, U12>(key, encrypt, iv, aad, data),
        (16, 16) => gcm::<Aes128, U16>(key, encrypt, iv, aad, data),
        (24, 12) => gcm::<Aes192, U12>(key, encrypt, iv, aad, data),
        (24, 16) => gcm::<Aes192, U16>(key, encrypt, iv, aad, data),
        (32, 12) => gcm::<Aes256, U12>(key, encrypt, iv, aad, data),
        (32, 16) => gcm::<Aes256, U16>(key, encrypt, iv, aad, data),
        (16 | 24 | 32, 0) => Err(error("OperationError", "the AES-GCM iv must not be empty")),
        (16 | 24 | 32, _) => Err(error(
            "NotSupportedError",
            "NextRS supports AES-GCM ivs of 12 or 16 bytes only",
        )),
        _ => Err(error("OperationError", "invalid AES key length")),
    }
}

fn gcm<C, N>(
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

/// AES-CBC with PKCS#7 padding and a 16-byte IV.
pub(crate) fn aes_cbc(
    key: &[u8],
    encrypt: bool,
    iv: &[u8],
    data: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    if iv.len() != 16 {
        return Err(error("OperationError", "the AES-CBC iv must be 16 bytes"));
    }
    match key.len() {
        16 => cbc_run::<Aes128>(key, encrypt, iv, data),
        24 => cbc_run::<Aes192>(key, encrypt, iv, data),
        32 => cbc_run::<Aes256>(key, encrypt, iv, data),
        _ => Err(error("OperationError", "invalid AES key length")),
    }
}

fn cbc_run<C>(key: &[u8], encrypt: bool, iv: &[u8], data: &[u8]) -> Result<Vec<u8>, JsErrorBox>
where
    C: aes::cipher::BlockCipher
        + aes::cipher::BlockSizeUser<BlockSize = U16>
        + aes::cipher::BlockEncrypt
        + aes::cipher::BlockDecrypt
        + aes::cipher::KeyInit,
{
    if encrypt {
        let cipher = cbc::Encryptor::<C>::new_from_slices(key, iv)
            .map_err(|_| error("OperationError", "invalid AES-CBC key or iv"))?;
        Ok(cipher.encrypt_padded_vec_mut::<Pkcs7>(data))
    } else {
        if data.is_empty() || !data.len().is_multiple_of(16) {
            return Err(error("OperationError", "AES-CBC decryption failed"));
        }
        let cipher = cbc::Decryptor::<C>::new_from_slices(key, iv)
            .map_err(|_| error("OperationError", "invalid AES-CBC key or iv"))?;
        cipher
            .decrypt_padded_vec_mut::<Pkcs7>(data)
            .map_err(|_| error("OperationError", "AES-CBC decryption failed"))
    }
}

/// HKDF-Extract and -Expand (RFC 5869) to `bits` bits, a multiple of 8.
pub(crate) fn hkdf(
    hash: Hash,
    ikm: &[u8],
    salt: &[u8],
    info: &[u8],
    bits: u32,
) -> Result<Vec<u8>, JsErrorBox> {
    if bits == 0 || !bits.is_multiple_of(8) {
        return Err(error(
            "OperationError",
            "the HKDF length must be a non-zero multiple of 8",
        ));
    }
    let len = (bits / 8) as usize;
    if len > 255 * hash.len() {
        return Err(error("OperationError", "the HKDF length is too large"));
    }
    let mut out = vec![0u8; len];
    let expanded = match hash {
        Hash::Sha1 => hkdf::Hkdf::<sha1::Sha1>::new(Some(salt), ikm).expand(info, &mut out),
        Hash::Sha256 => hkdf::Hkdf::<sha2::Sha256>::new(Some(salt), ikm).expand(info, &mut out),
        Hash::Sha384 => hkdf::Hkdf::<sha2::Sha384>::new(Some(salt), ikm).expand(info, &mut out),
        Hash::Sha512 => hkdf::Hkdf::<sha2::Sha512>::new(Some(salt), ikm).expand(info, &mut out),
    };
    expanded.map_err(|_| error("OperationError", "the HKDF length is too large"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{Hash, aes_cbc, aes_gcm, hkdf, hmac_sign, hmac_verify};

    pub(crate) fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// RFC 4231 test cases 1, 2 and 6 (SHA-256 and SHA-512), and the
    /// RFC 2202 case 2 for SHA-1.
    #[test]
    fn hmac_rfc_vectors() {
        let cases: &[(Hash, Vec<u8>, &[u8], &str)] = &[
            (
                Hash::Sha256,
                vec![0x0b; 20],
                b"Hi There",
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            ),
            (
                Hash::Sha512,
                vec![0x0b; 20],
                b"Hi There",
                "87aa7cdea5ef619d4ff0b4241a1d6cb02379f4e2ce4ec2787ad0b30545e17cdedaa833b7d6b8a702038b274eaea3f4e4be9d914eeb61f1702e696c203a126854",
            ),
            (
                Hash::Sha256,
                b"Jefe".to_vec(),
                b"what do ya want for nothing?",
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                Hash::Sha384,
                b"Jefe".to_vec(),
                b"what do ya want for nothing?",
                "af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649",
            ),
            (
                Hash::Sha256,
                vec![0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First",
                "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
            ),
            (
                Hash::Sha1,
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
        assert!(aes_cbc(&[0u8; 32], true, &[0u8; 12], b"x").is_err());
        assert!(aes_cbc(&[0u8; 20], true, &[0u8; 16], b"x").is_err());
    }

    /// NIST SP 800-38A F.2.5 (CBC-AES256.Encrypt), first two blocks; the
    /// padding block PKCS#7 appends is checked by the round trip.
    #[test]
    fn cbc_sp800_38a_vector() {
        let key = hex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4");
        let iv = hex("000102030405060708090a0b0c0d0e0f");
        let pt = hex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51");
        let out = aes_cbc(&key, true, &iv, &pt).unwrap();
        assert_eq!(out.len(), 48);
        assert_eq!(
            &out[..32],
            hex("f58c4c04d6e5f1ba779eabfb5f7bfbd69cfc4e967edb808d679f777bc6702c7d").as_slice()
        );
        assert_eq!(aes_cbc(&key, false, &iv, &out).unwrap(), pt);
        // A corrupted last block fails the padding check (or yields other
        // bytes); a wrong length always fails.
        assert!(aes_cbc(&key, false, &iv, &out[..47]).is_err());
        assert!(aes_cbc(&key, false, &iv, &[]).is_err());
    }

    /// RFC 5869 test cases 1 (SHA-256) and 3 (SHA-256, empty salt and
    /// info), and the length limits.
    #[test]
    fn hkdf_rfc5869_vectors() {
        let okm = hkdf(
            Hash::Sha256,
            &[0x0b; 22],
            &hex("000102030405060708090a0b0c"),
            &hex("f0f1f2f3f4f5f6f7f8f9"),
            42 * 8,
        )
        .unwrap();
        assert_eq!(
            okm,
            hex(
                "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
            )
        );
        let okm = hkdf(Hash::Sha256, &[0x0b; 22], &[], &[], 42 * 8).unwrap();
        assert_eq!(
            okm,
            hex(
                "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
            )
        );
        assert!(hkdf(Hash::Sha256, b"k", &[], &[], 0).is_err());
        assert!(hkdf(Hash::Sha256, b"k", &[], &[], 12).is_err());
        assert!(hkdf(Hash::Sha256, b"k", &[], &[], 255 * 32 * 8).is_ok());
        assert!(hkdf(Hash::Sha256, b"k", &[], &[], (255 * 32 + 1) * 8).is_err());
    }
}
