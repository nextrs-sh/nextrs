//! Asymmetric signature keys: ECDSA (P-256, P-384), RSASSA-PKCS1-v1_5 and
//! RSA-PSS (SHA-256/384/512), Ed25519.
//!
//! Signing and verification are ring's. ring bounds what NextRS supports,
//! and anything outside it is refused at import, never at use:
//!
//! * ECDSA signs P-256 with SHA-256 and P-384 with SHA-384 (JWS ES256 and
//!   ES384); other curve/hash pairings and P-521 are `NotSupportedError`.
//!   Signatures are IEEE P1363 `r || s`, as Web Crypto and JWS use them.
//!   Raw public keys must be uncompressed points.
//! * RSA public keys of 2048-8192 bits verify, private keys of 2048-4096
//!   bits sign (ring's limits); smaller keys are `NotSupportedError` (JOSE
//!   libraries refuse them too). RSA-PSS's salt length must equal the
//!   hash's length (JWS PS256/384/512). RSA keys are imported, never
//!   generated (ring has no RSA key generation). Multi-prime keys and
//!   private keys without CRT parameters are refused.
//! * EC PKCS#8 keys must carry their public key (ring derives nothing).
//!
//! An EC public point is checked for its encoding at import and for being
//! on the curve by ring when it verifies (an invalid point never verifies).

use deno_error::JsErrorBox;
use der::asn1::{AnyRef, BitStringRef, ObjectIdentifier, OctetStringRef, UintRef};
use der::{Decode, Encode};
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use zeroize::Zeroizing;

use super::{Algorithm, Curve, Hash, Material, SignSpec, StoredKey, error};

const RSA_ENCRYPTION: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const EC_PUBLIC_KEY: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const SECP256R1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");
const SECP384R1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.34");
const ED25519: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.112");

fn data_error(message: &str) -> JsErrorBox {
    error("DataError", message)
}

impl Curve {
    fn oid(self) -> ObjectIdentifier {
        match self {
            Self::P256 => SECP256R1,
            Self::P384 => SECP384R1,
        }
    }

    /// The one hash ring signs this curve with.
    fn hash(self) -> Hash {
        match self {
            Self::P256 => Hash::Sha256,
            Self::P384 => Hash::Sha384,
        }
    }

    fn signing(self) -> &'static signature::EcdsaSigningAlgorithm {
        match self {
            Self::P256 => &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            Self::P384 => &signature::ECDSA_P384_SHA384_FIXED_SIGNING,
        }
    }

    fn verification(self) -> &'static signature::EcdsaVerificationAlgorithm {
        match self {
            Self::P256 => &signature::ECDSA_P256_SHA256_FIXED,
            Self::P384 => &signature::ECDSA_P384_SHA384_FIXED,
        }
    }
}

// --- Key material -------------------------------------------------------------

pub(crate) struct EcPublic {
    pub curve: Curve,
    /// The uncompressed point, `04 || x || y`.
    pub point: Vec<u8>,
}

pub(crate) struct EcPrivate {
    pub public: EcPublic,
    pub d: Zeroizing<Vec<u8>>,
    pair: EcdsaKeyPair,
}

pub(crate) struct RsaPublic {
    /// Big-endian, without leading zero bytes.
    pub n: Vec<u8>,
    pub e: Vec<u8>,
}

/// A two-prime RSA private key's components (big-endian, minimal).
pub(crate) struct RsaParts {
    pub d: Zeroizing<Vec<u8>>,
    pub p: Zeroizing<Vec<u8>>,
    pub q: Zeroizing<Vec<u8>>,
    pub dp: Zeroizing<Vec<u8>>,
    pub dq: Zeroizing<Vec<u8>>,
    pub qi: Zeroizing<Vec<u8>>,
}

pub(crate) struct RsaPrivate {
    pub public: RsaPublic,
    pub parts: RsaParts,
    pair: ring::rsa::KeyPair,
}

pub(crate) struct EdPublic(pub [u8; 32]);

pub(crate) struct EdPrivate {
    pub public: EdPublic,
    pub seed: Zeroizing<[u8; 32]>,
    pair: Ed25519KeyPair,
}

fn strip_zeros(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    &bytes[start..]
}

pub(crate) fn ec_public(curve: Curve, point: &[u8]) -> Result<EcPublic, JsErrorBox> {
    let n = curve.field_len();
    match (point.first(), point.len()) {
        (Some(4), len) if len == 1 + 2 * n => Ok(EcPublic {
            curve,
            point: point.to_vec(),
        }),
        (Some(2 | 3), len) if len == 1 + n => Err(error(
            "NotSupportedError",
            "NextRS imports uncompressed EC points only",
        )),
        _ => Err(data_error("invalid EC public key")),
    }
}

pub(crate) fn ec_private(curve: Curve, d: &[u8], point: &[u8]) -> Result<EcPrivate, JsErrorBox> {
    let public = ec_public(curve, point)?;
    if d.len() != curve.field_len() {
        return Err(data_error("invalid EC private key"));
    }
    let pair = EcdsaKeyPair::from_private_key_and_public_key(
        curve.signing(),
        d,
        &public.point,
        &SystemRandom::new(),
    )
    .map_err(|_| data_error("invalid EC private key, or it does not match its public key"))?;
    Ok(EcPrivate {
        public,
        d: Zeroizing::new(d.to_vec()),
        pair,
    })
}

impl RsaPublic {
    pub(crate) fn modulus_bits(&self) -> u32 {
        match self.n.first() {
            Some(&top) => (self.n.len() as u32 - 1) * 8 + (8 - top.leading_zeros()),
            None => 0,
        }
    }
}

pub(crate) fn rsa_public(n: &[u8], e: &[u8]) -> Result<RsaPublic, JsErrorBox> {
    let public = RsaPublic {
        n: strip_zeros(n).to_vec(),
        e: strip_zeros(e).to_vec(),
    };
    let bits = public.modulus_bits();
    if bits == 0 || public.n.last().is_some_and(|b| b & 1 == 0) {
        return Err(data_error("invalid RSA modulus"));
    }
    if !(2048..=8192).contains(&bits) {
        return Err(error(
            "NotSupportedError",
            "NextRS supports RSA keys of 2048 to 8192 bits",
        ));
    }
    // ring's bounds on the public exponent: odd, at least 3, below 2^33.
    let e_value = public.e.iter().try_fold(0u64, |acc, &b| {
        (acc < (1 << 56)).then_some((acc << 8) | b as u64)
    });
    match e_value {
        Some(e) if e >= 3 && e & 1 == 1 && e < (1 << 33) => Ok(public),
        _ => Err(error(
            "NotSupportedError",
            "NextRS supports odd RSA public exponents from 3 to 2^33 - 1",
        )),
    }
}

pub(crate) fn rsa_private(public: RsaPublic, parts: RsaParts) -> Result<RsaPrivate, JsErrorBox> {
    if public.modulus_bits() > 4096 {
        return Err(error(
            "NotSupportedError",
            "NextRS signs with RSA keys of 2048 to 4096 bits",
        ));
    }
    let components = ring::rsa::KeyPairComponents {
        public_key: ring::rsa::PublicKeyComponents {
            n: public.n.as_slice(),
            e: public.e.as_slice(),
        },
        d: parts.d.as_slice(),
        p: parts.p.as_slice(),
        q: parts.q.as_slice(),
        dP: parts.dp.as_slice(),
        dQ: parts.dq.as_slice(),
        qInv: parts.qi.as_slice(),
    };
    let pair = ring::rsa::KeyPair::from_components(&components)
        .map_err(|_| data_error("invalid RSA private key"))?;
    Ok(RsaPrivate {
        public,
        parts,
        pair,
    })
}

pub(crate) fn ed_private(seed: &[u8], public: Option<&[u8]>) -> Result<EdPrivate, JsErrorBox> {
    let seed: [u8; 32] = seed
        .try_into()
        .map_err(|_| data_error("invalid Ed25519 private key"))?;
    let pair = match public {
        Some(public) => Ed25519KeyPair::from_seed_and_public_key(&seed, public),
        None => Ed25519KeyPair::from_seed_unchecked(&seed),
    }
    .map_err(|_| data_error("invalid Ed25519 private key, or it does not match its public key"))?;
    let public = EdPublic(
        pair.public_key()
            .as_ref()
            .try_into()
            .expect("Ed25519 public keys are 32 bytes"),
    );
    Ok(EdPrivate {
        public,
        seed: Zeroizing::new(seed),
        pair,
    })
}

pub(crate) fn ed_public(bytes: &[u8]) -> Result<EdPublic, JsErrorBox> {
    Ok(EdPublic(
        bytes
            .try_into()
            .map_err(|_| data_error("invalid Ed25519 public key"))?,
    ))
}

// --- Import ---------------------------------------------------------------------

fn parse_spki(data: &[u8]) -> Result<spki::SubjectPublicKeyInfoRef<'_>, JsErrorBox> {
    spki::SubjectPublicKeyInfoRef::try_from(data).map_err(|_| data_error("invalid SPKI"))
}

fn parse_pkcs8(data: &[u8]) -> Result<pkcs8::PrivateKeyInfo<'_>, JsErrorBox> {
    pkcs8::PrivateKeyInfo::try_from(data).map_err(|_| data_error("invalid PKCS #8"))
}

fn named_curve(parameters: Option<&AnyRef<'_>>) -> Result<ObjectIdentifier, JsErrorBox> {
    parameters
        .and_then(|p| p.decode_as::<ObjectIdentifier>().ok())
        .ok_or_else(|| data_error("the EC key does not name its curve"))
}

fn check_curve(curve: Curve, oid: ObjectIdentifier) -> Result<(), JsErrorBox> {
    if oid == curve.oid() {
        Ok(())
    } else {
        Err(data_error(
            "the key's curve is not the requested namedCurve",
        ))
    }
}

fn spki_key<'a>(spki: &spki::SubjectPublicKeyInfoRef<'a>) -> Result<&'a [u8], JsErrorBox> {
    spki.subject_public_key
        .as_bytes()
        .ok_or_else(|| data_error("invalid SPKI"))
}

/// The EC private key inside a PKCS#8 structure: `(d, point)`.
fn ec_from_pkcs8(curve: Curve, data: &[u8]) -> Result<EcPrivate, JsErrorBox> {
    let info = parse_pkcs8(data)?;
    if info.algorithm.oid != EC_PUBLIC_KEY {
        return Err(data_error("the PKCS #8 key is not an EC key"));
    }
    check_curve(curve, named_curve(info.algorithm.parameters.as_ref())?)?;
    let key = sec1::EcPrivateKey::try_from(info.private_key)
        .map_err(|_| data_error("invalid EC private key"))?;
    if let Some(parameters) = key.parameters {
        let oid = parameters
            .named_curve()
            .ok_or_else(|| data_error("invalid EC private key"))?;
        check_curve(curve, oid)?;
    }
    let point = key.public_key.ok_or_else(|| {
        error(
            "NotSupportedError",
            "NextRS imports EC PKCS #8 keys only when they include the public key",
        )
    })?;
    ec_private(curve, key.private_key, point)
}

pub(crate) fn import_ec(curve: Curve, format: &str, data: &[u8]) -> Result<Material, JsErrorBox> {
    match format {
        "raw" => Ok(Material::EcPublic(ec_public(curve, data)?)),
        "spki" => {
            let spki = parse_spki(data)?;
            if spki.algorithm.oid != EC_PUBLIC_KEY {
                return Err(data_error("the SPKI key is not an EC key"));
            }
            check_curve(curve, named_curve(spki.algorithm.parameters.as_ref())?)?;
            Ok(Material::EcPublic(ec_public(curve, spki_key(&spki)?)?))
        }
        "pkcs8" => Ok(Material::EcPrivate(Box::new(ec_from_pkcs8(curve, data)?))),
        _ => Err(error(
            "NotSupportedError",
            "unsupported key format for ECDSA",
        )),
    }
}

fn check_rsa_parameters(parameters: Option<&AnyRef<'_>>) -> Result<(), JsErrorBox> {
    match parameters {
        None => Ok(()),
        Some(p) if p.is_null() => Ok(()),
        Some(_) => Err(data_error("invalid RSA algorithm parameters")),
    }
}

fn rsa_parts(key: &pkcs1::RsaPrivateKey<'_>) -> RsaParts {
    let own = |u: &UintRef<'_>| Zeroizing::new(u.as_bytes().to_vec());
    RsaParts {
        d: own(&key.private_exponent),
        p: own(&key.prime1),
        q: own(&key.prime2),
        dp: own(&key.exponent1),
        dq: own(&key.exponent2),
        qi: own(&key.coefficient),
    }
}

pub(crate) fn import_rsa(format: &str, data: &[u8]) -> Result<Material, JsErrorBox> {
    match format {
        "spki" => {
            let spki = parse_spki(data)?;
            if spki.algorithm.oid != RSA_ENCRYPTION {
                return Err(data_error("the SPKI key is not an rsaEncryption key"));
            }
            check_rsa_parameters(spki.algorithm.parameters.as_ref())?;
            let key = pkcs1::RsaPublicKey::from_der(spki_key(&spki)?)
                .map_err(|_| data_error("invalid RSA public key"))?;
            Ok(Material::RsaPublic(rsa_public(
                key.modulus.as_bytes(),
                key.public_exponent.as_bytes(),
            )?))
        }
        "pkcs8" => {
            let info = parse_pkcs8(data)?;
            if info.algorithm.oid != RSA_ENCRYPTION {
                return Err(data_error("the PKCS #8 key is not an rsaEncryption key"));
            }
            check_rsa_parameters(info.algorithm.parameters.as_ref())?;
            let key = pkcs1::RsaPrivateKey::from_der(info.private_key)
                .map_err(|_| data_error("invalid RSA private key"))?;
            if key.other_prime_infos.is_some() {
                return Err(error(
                    "NotSupportedError",
                    "NextRS does not support multi-prime RSA keys",
                ));
            }
            let public = rsa_public(key.modulus.as_bytes(), key.public_exponent.as_bytes())?;
            Ok(Material::RsaPrivate(Box::new(rsa_private(
                public,
                rsa_parts(&key),
            )?)))
        }
        _ => Err(error("NotSupportedError", "unsupported key format for RSA")),
    }
}

pub(crate) fn import_ed25519(format: &str, data: &[u8]) -> Result<Material, JsErrorBox> {
    match format {
        "raw" => Ok(Material::EdPublic(ed_public(data)?)),
        "spki" => {
            let spki = parse_spki(data)?;
            if spki.algorithm.oid != ED25519 || spki.algorithm.parameters.is_some() {
                return Err(data_error("the SPKI key is not an Ed25519 key"));
            }
            Ok(Material::EdPublic(ed_public(spki_key(&spki)?)?))
        }
        "pkcs8" => {
            let info = parse_pkcs8(data)?;
            if info.algorithm.oid != ED25519 || info.algorithm.parameters.is_some() {
                return Err(data_error("the PKCS #8 key is not an Ed25519 key"));
            }
            let seed = OctetStringRef::from_der(info.private_key)
                .map_err(|_| data_error("invalid Ed25519 private key"))?;
            Ok(Material::EdPrivate(Box::new(ed_private(
                seed.as_bytes(),
                info.public_key,
            )?)))
        }
        _ => Err(error(
            "NotSupportedError",
            "unsupported key format for Ed25519",
        )),
    }
}

// --- Generation -------------------------------------------------------------------

pub(crate) fn generate_ec(curve: Curve) -> Result<(Material, Material), JsErrorBox> {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(curve.signing(), &rng)
        .map_err(|_| error("OperationError", "EC key generation failed"))?;
    let private = ec_from_pkcs8(curve, pkcs8.as_ref())?;
    let public = EcPublic {
        curve,
        point: private.public.point.clone(),
    };
    Ok((
        Material::EcPublic(public),
        Material::EcPrivate(Box::new(private)),
    ))
}

pub(crate) fn generate_ed25519() -> Result<(Material, Material), JsErrorBox> {
    let mut seed = Zeroizing::new([0u8; 32]);
    getrandom::fill(seed.as_mut())
        .map_err(|e| error("OperationError", &format!("getrandom failed: {e}")))?;
    let private = ed_private(seed.as_ref(), None)?;
    let public = EdPublic(private.public.0);
    Ok((
        Material::EdPublic(public),
        Material::EdPrivate(Box::new(private)),
    ))
}

// --- Export -------------------------------------------------------------------------

fn encode_error(_: der::Error) -> JsErrorBox {
    error("OperationError", "the key could not be encoded")
}

fn spki_der(
    oid: ObjectIdentifier,
    parameters: Option<AnyRef<'_>>,
    key: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    spki::SubjectPublicKeyInfoRef {
        algorithm: spki::AlgorithmIdentifierRef { oid, parameters },
        subject_public_key: BitStringRef::from_bytes(key).map_err(encode_error)?,
    }
    .to_der()
    .map_err(encode_error)
}

fn pkcs8_der(
    oid: ObjectIdentifier,
    parameters: Option<AnyRef<'_>>,
    private_key: &[u8],
) -> Result<Vec<u8>, JsErrorBox> {
    pkcs8::PrivateKeyInfo {
        algorithm: spki::AlgorithmIdentifierRef { oid, parameters },
        private_key,
        public_key: None,
    }
    .to_der()
    .map_err(encode_error)
}

fn rsa_public_der(public: &RsaPublic) -> Result<Vec<u8>, JsErrorBox> {
    pkcs1::RsaPublicKey {
        modulus: UintRef::new(&public.n).map_err(encode_error)?,
        public_exponent: UintRef::new(&public.e).map_err(encode_error)?,
    }
    .to_der()
    .map_err(encode_error)
}

fn uint(bytes: &[u8]) -> Result<UintRef<'_>, JsErrorBox> {
    UintRef::new(bytes).map_err(encode_error)
}

pub(crate) fn export(material: &Material, format: &str) -> Result<Vec<u8>, JsErrorBox> {
    let curve_param = |curve: Curve| curve.oid();
    match (material, format) {
        (Material::EcPublic(public), "raw") => Ok(public.point.clone()),
        (Material::EcPublic(public), "spki") => {
            let oid = curve_param(public.curve);
            spki_der(EC_PUBLIC_KEY, Some(AnyRef::from(&oid)), &public.point)
        }
        (Material::EcPrivate(private), "pkcs8") => {
            let inner = Zeroizing::new(
                sec1::EcPrivateKey {
                    private_key: &private.d,
                    parameters: None,
                    public_key: Some(&private.public.point),
                }
                .to_der()
                .map_err(encode_error)?,
            );
            let oid = curve_param(private.public.curve);
            pkcs8_der(EC_PUBLIC_KEY, Some(AnyRef::from(&oid)), &inner)
        }
        (Material::RsaPublic(public), "spki") => {
            spki_der(RSA_ENCRYPTION, Some(AnyRef::NULL), &rsa_public_der(public)?)
        }
        (Material::RsaPrivate(private), "pkcs8") => {
            let parts = &private.parts;
            let inner = Zeroizing::new(
                pkcs1::RsaPrivateKey {
                    modulus: uint(&private.public.n)?,
                    public_exponent: uint(&private.public.e)?,
                    private_exponent: uint(&parts.d)?,
                    prime1: uint(&parts.p)?,
                    prime2: uint(&parts.q)?,
                    exponent1: uint(&parts.dp)?,
                    exponent2: uint(&parts.dq)?,
                    coefficient: uint(&parts.qi)?,
                    other_prime_infos: None,
                }
                .to_der()
                .map_err(encode_error)?,
            );
            pkcs8_der(RSA_ENCRYPTION, Some(AnyRef::NULL), &inner)
        }
        (Material::EdPublic(public), "raw") => Ok(public.0.to_vec()),
        (Material::EdPublic(public), "spki") => spki_der(ED25519, None, &public.0),
        (Material::EdPrivate(private), "pkcs8") => {
            let inner = Zeroizing::new(
                OctetStringRef::new(private.seed.as_ref())
                    .map_err(encode_error)?
                    .to_der()
                    .map_err(encode_error)?,
            );
            pkcs8_der(ED25519, None, &inner)
        }
        (Material::EcPublic(_) | Material::RsaPublic(_) | Material::EdPublic(_), "pkcs8")
        | (
            Material::EcPrivate(_) | Material::RsaPrivate(_) | Material::EdPrivate(_),
            "raw" | "spki",
        )
        | (Material::RsaPublic(_), "raw") => Err(error(
            "InvalidAccessError",
            "the key cannot be exported in this format",
        )),
        _ => Err(error("NotSupportedError", "unsupported export format")),
    }
}

// --- Sign and verify ------------------------------------------------------------------

fn ecdsa_hash(curve: Curve, spec: &SignSpec) -> Result<(), JsErrorBox> {
    let hash = Hash::parse(
        spec.hash
            .as_deref()
            .ok_or_else(|| error("TypeError", "ECDSA needs a hash"))?,
    )?;
    if hash != curve.hash() {
        return Err(error(
            "NotSupportedError",
            "NextRS uses ECDSA with SHA-256 on P-256 and SHA-384 on P-384 only",
        ));
    }
    Ok(())
}

fn pss_salt(hash: Hash, spec: &SignSpec) -> Result<(), JsErrorBox> {
    match spec.salt_length {
        None => Err(error("TypeError", "RSA-PSS needs a saltLength")),
        Some(len) if len as usize == hash.len() => Ok(()),
        Some(_) => Err(error(
            "NotSupportedError",
            "NextRS uses RSA-PSS with a salt as long as the hash only",
        )),
    }
}

fn rsa_encoding(algorithm: Algorithm) -> Option<&'static dyn signature::RsaEncoding> {
    Some(match algorithm {
        Algorithm::RsassaPkcs1(Hash::Sha256) => &signature::RSA_PKCS1_SHA256,
        Algorithm::RsassaPkcs1(Hash::Sha384) => &signature::RSA_PKCS1_SHA384,
        Algorithm::RsassaPkcs1(Hash::Sha512) => &signature::RSA_PKCS1_SHA512,
        Algorithm::RsaPss(Hash::Sha256) => &signature::RSA_PSS_SHA256,
        Algorithm::RsaPss(Hash::Sha384) => &signature::RSA_PSS_SHA384,
        Algorithm::RsaPss(Hash::Sha512) => &signature::RSA_PSS_SHA512,
        _ => return None,
    })
}

fn rsa_verification(algorithm: Algorithm) -> Option<&'static signature::RsaParameters> {
    Some(match algorithm {
        Algorithm::RsassaPkcs1(Hash::Sha256) => &signature::RSA_PKCS1_2048_8192_SHA256,
        Algorithm::RsassaPkcs1(Hash::Sha384) => &signature::RSA_PKCS1_2048_8192_SHA384,
        Algorithm::RsassaPkcs1(Hash::Sha512) => &signature::RSA_PKCS1_2048_8192_SHA512,
        Algorithm::RsaPss(Hash::Sha256) => &signature::RSA_PSS_2048_8192_SHA256,
        Algorithm::RsaPss(Hash::Sha384) => &signature::RSA_PSS_2048_8192_SHA384,
        Algorithm::RsaPss(Hash::Sha512) => &signature::RSA_PSS_2048_8192_SHA512,
        _ => return None,
    })
}

/// The registered name an operation must use with this key.
fn operation_name(algorithm: Algorithm) -> &'static str {
    match algorithm {
        Algorithm::Ecdsa(_) => "ECDSA",
        Algorithm::RsassaPkcs1(_) => "RSASSA-PKCS1-v1_5",
        Algorithm::RsaPss(_) => "RSA-PSS",
        Algorithm::Ed25519 => "Ed25519",
        Algorithm::Hmac(_) => "HMAC",
        _ => "",
    }
}

fn check_operation(key: &StoredKey, spec: &SignSpec) -> Result<(), JsErrorBox> {
    if spec.name != operation_name(key.algorithm) {
        return Err(error(
            "InvalidAccessError",
            "the key's algorithm does not match the operation",
        ));
    }
    match (key.algorithm, spec.name.as_str()) {
        (Algorithm::Ecdsa(curve), _) => ecdsa_hash(curve, spec),
        (Algorithm::RsaPss(hash), _) => pss_salt(hash, spec),
        _ => Ok(()),
    }
}

pub(crate) fn sign(key: &StoredKey, spec: &SignSpec, data: &[u8]) -> Result<Vec<u8>, JsErrorBox> {
    check_operation(key, spec)?;
    let failed = |_| error("OperationError", "signing failed");
    let rng = SystemRandom::new();
    match &key.material {
        Material::EcPrivate(private) => private
            .pair
            .sign(&rng, data)
            .map(|signature| signature.as_ref().to_vec())
            .map_err(failed),
        Material::RsaPrivate(private) => {
            let encoding = rsa_encoding(key.algorithm)
                .ok_or_else(|| error("NotSupportedError", "unsupported RSA algorithm"))?;
            let mut signature = vec![0u8; private.pair.public().modulus_len()];
            private
                .pair
                .sign(encoding, &rng, data, &mut signature)
                .map_err(failed)?;
            Ok(signature)
        }
        Material::EdPrivate(private) => Ok(private.pair.sign(data).as_ref().to_vec()),
        _ => Err(error("InvalidAccessError", "only a private key can sign")),
    }
}

pub(crate) fn verify(
    key: &StoredKey,
    spec: &SignSpec,
    signature: &[u8],
    data: &[u8],
) -> Result<bool, JsErrorBox> {
    check_operation(key, spec)?;
    Ok(match &key.material {
        Material::EcPublic(public) => {
            UnparsedPublicKey::new(public.curve.verification(), &public.point)
                .verify(data, signature)
                .is_ok()
        }
        Material::RsaPublic(public) => {
            let parameters = rsa_verification(key.algorithm)
                .ok_or_else(|| error("NotSupportedError", "unsupported RSA algorithm"))?;
            signature::RsaPublicKeyComponents {
                n: public.n.as_slice(),
                e: public.e.as_slice(),
            }
            .verify(parameters, data, signature)
            .is_ok()
        }
        Material::EdPublic(public) => UnparsedPublicKey::new(&signature::ED25519, &public.0)
            .verify(data, signature)
            .is_ok(),
        _ => return Err(error("InvalidAccessError", "only a public key can verify")),
    })
}
