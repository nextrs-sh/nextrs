//! JSON Web Keys (RFC 7517/7518, RFC 8037 for OKP): the key material
//! members. JavaScript checks the members Web Crypto compares with the
//! request (`kty`, `alg`, `use`, `key_ops`, `ext`) and adds them on export;
//! this module decodes and validates the material, and refuses what NextRS
//! does not support (RSA `oth`, private RSA keys without CRT parameters).
//!
//! Base64url values are decoded strictly (no padding, no other
//! alphabet); EC coordinates and private scalars must have their curve's
//! fixed length, as RFC 7518 requires.

use base64ct::{Base64UrlUnpadded, Encoding};
use deno_error::JsErrorBox;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::asymmetric::{self, RsaParts};
use super::{Algorithm, Curve, Material, error, secret};

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct Jwk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dq: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<String>,
    /// Only checked for presence (multi-prime RSA is refused).
    #[serde(skip_serializing)]
    pub oth: Option<serde_json::Value>,
}

fn data_error(message: &str) -> JsErrorBox {
    error("DataError", message)
}

fn decode(value: &Option<String>, member: &str) -> Result<Zeroizing<Vec<u8>>, JsErrorBox> {
    let value = value
        .as_deref()
        .ok_or_else(|| data_error(&format!("the JWK has no '{member}'")))?;
    Base64UrlUnpadded::decode_vec(value)
        .map(Zeroizing::new)
        .map_err(|_| data_error(&format!("the JWK's '{member}' is not base64url")))
}

fn encode(bytes: &[u8]) -> Option<String> {
    Some(Base64UrlUnpadded::encode_string(bytes))
}

fn check_kty(jwk: &Jwk, kty: &str) -> Result<(), JsErrorBox> {
    if jwk.kty.as_deref() == Some(kty) {
        Ok(())
    } else {
        Err(data_error(&format!("the JWK's kty must be '{kty}'")))
    }
}

pub(crate) fn import(algorithm: Algorithm, jwk: &Jwk) -> Result<Material, JsErrorBox> {
    match algorithm {
        Algorithm::AesGcm | Algorithm::AesCbc | Algorithm::Hmac(_) => {
            check_kty(jwk, "oct")?;
            secret::import(algorithm, &decode(&jwk.k, "k")?)
        }
        Algorithm::Hkdf => Err(error(
            "NotSupportedError",
            "HKDF keys import from 'raw' only",
        )),
        Algorithm::Ecdsa(curve) => import_ec(curve, jwk),
        Algorithm::RsassaPkcs1(_) | Algorithm::RsaPss(_) => import_rsa(jwk),
        Algorithm::Ed25519 => {
            check_kty(jwk, "OKP")?;
            if jwk.crv.as_deref() != Some("Ed25519") {
                return Err(data_error("the JWK's crv must be 'Ed25519'"));
            }
            let x = decode(&jwk.x, "x")?;
            if jwk.d.is_some() {
                let d = decode(&jwk.d, "d")?;
                Ok(Material::EdPrivate(Box::new(asymmetric::ed_private(
                    &d,
                    Some(&x),
                )?)))
            } else {
                Ok(Material::EdPublic(asymmetric::ed_public(&x)?))
            }
        }
    }
}

fn import_ec(curve: Curve, jwk: &Jwk) -> Result<Material, JsErrorBox> {
    check_kty(jwk, "EC")?;
    if jwk.crv.as_deref() != Some(curve.name()) {
        return Err(data_error("the JWK's crv is not the requested namedCurve"));
    }
    let n = curve.field_len();
    let x = decode(&jwk.x, "x")?;
    let y = decode(&jwk.y, "y")?;
    if x.len() != n || y.len() != n {
        return Err(data_error("the JWK's coordinates have the wrong length"));
    }
    let mut point = Vec::with_capacity(1 + 2 * n);
    point.push(4);
    point.extend_from_slice(&x);
    point.extend_from_slice(&y);
    if jwk.d.is_some() {
        let d = decode(&jwk.d, "d")?;
        if d.len() != n {
            return Err(data_error("the JWK's d has the wrong length"));
        }
        Ok(Material::EcPrivate(Box::new(asymmetric::ec_private(
            curve, &d, &point,
        )?)))
    } else {
        Ok(Material::EcPublic(asymmetric::ec_public(curve, &point)?))
    }
}

fn import_rsa(jwk: &Jwk) -> Result<Material, JsErrorBox> {
    check_kty(jwk, "RSA")?;
    let n = decode(&jwk.n, "n")?;
    let e = decode(&jwk.e, "e")?;
    let public = asymmetric::rsa_public(&n, &e)?;
    if jwk.d.is_none() {
        return Ok(Material::RsaPublic(public));
    }
    if jwk.oth.is_some() {
        return Err(error(
            "NotSupportedError",
            "NextRS does not support multi-prime RSA keys",
        ));
    }
    let crt = [&jwk.p, &jwk.q, &jwk.dp, &jwk.dq, &jwk.qi];
    if crt.iter().all(|member| member.is_none()) {
        return Err(error(
            "NotSupportedError",
            "NextRS needs the CRT parameters of a private RSA JWK",
        ));
    }
    if crt.iter().any(|member| member.is_none()) {
        return Err(data_error("the JWK has some but not all CRT parameters"));
    }
    let strip = |bytes: Zeroizing<Vec<u8>>| {
        let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
        Zeroizing::new(bytes[start..].to_vec())
    };
    let parts = RsaParts {
        d: strip(decode(&jwk.d, "d")?),
        p: strip(decode(&jwk.p, "p")?),
        q: strip(decode(&jwk.q, "q")?),
        dp: strip(decode(&jwk.dp, "dp")?),
        dq: strip(decode(&jwk.dq, "dq")?),
        qi: strip(decode(&jwk.qi, "qi")?),
    };
    Ok(Material::RsaPrivate(Box::new(asymmetric::rsa_private(
        public, parts,
    )?)))
}

pub(crate) fn export(material: &Material) -> Jwk {
    let ec = |public: &asymmetric::EcPublic| {
        let n = public.curve.field_len();
        Jwk {
            kty: Some("EC".into()),
            crv: Some(public.curve.name().into()),
            x: encode(&public.point[1..1 + n]),
            y: encode(&public.point[1 + n..]),
            ..Jwk::default()
        }
    };
    let rsa = |public: &asymmetric::RsaPublic| Jwk {
        kty: Some("RSA".into()),
        n: encode(&public.n),
        e: encode(&public.e),
        ..Jwk::default()
    };
    let okp = |public: &asymmetric::EdPublic| Jwk {
        kty: Some("OKP".into()),
        crv: Some("Ed25519".into()),
        x: encode(&public.0),
        ..Jwk::default()
    };
    match material {
        Material::Secret(bytes) => Jwk {
            kty: Some("oct".into()),
            k: encode(bytes),
            ..Jwk::default()
        },
        Material::EcPublic(public) => ec(public),
        Material::EcPrivate(private) => Jwk {
            d: encode(&private.d),
            ..ec(&private.public)
        },
        Material::RsaPublic(public) => rsa(public),
        Material::RsaPrivate(private) => {
            let parts = &private.parts;
            Jwk {
                d: encode(&parts.d),
                p: encode(&parts.p),
                q: encode(&parts.q),
                dp: encode(&parts.dp),
                dq: encode(&parts.dq),
                qi: encode(&parts.qi),
                ..rsa(&private.public)
            }
        }
        Material::EdPublic(public) => okp(public),
        Material::EdPrivate(private) => Jwk {
            d: encode(private.seed.as_ref()),
            ..okp(&private.public)
        },
    }
}
