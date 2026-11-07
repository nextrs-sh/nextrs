//! Web Crypto (AES-GCM, HMAC) and FormData, the Web APIs Server Actions and
//! session libraries need,
//! checked against Node: tests/fixtures/handlers/webapis.expected.json is
//! the output of `node dist/webapis.js` (Node 22), and the same checks run
//! in the embedded runtime must produce it, apart from the differences
//! listed here.

mod common;

use common::{collect, executor, request};
use nextrs_runtime::Limits;
use serde_json::{Value, json};

#[tokio::test(flavor = "multi_thread")]
async fn web_crypto_and_form_data_match_node() {
    let executor = executor(Limits::default());
    let response = collect(&executor, request("webapis.js", &[])).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let mut actual: Value = serde_json::from_str(&response.text()).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/handlers/webapis.expected.json")).unwrap();

    // Node names this DOMException "InvalidAccessException"; Web Crypto
    // specifies InvalidAccessError, which NextRS throws.
    assert_eq!(
        actual["crypto"]["exportNonExtractable"],
        json!({ "error": "InvalidAccessError" })
    );
    actual["crypto"]["exportNonExtractable"] = expected["crypto"]["exportNonExtractable"].clone();

    // NextRS refuses what it does not implement instead of approximating.
    let refusals = actual
        .as_object_mut()
        .unwrap()
        .remove("nextrsOnly")
        .unwrap();
    assert_eq!(
        refusals,
        json!({
            "iv8": { "error": "NotSupportedError" },
            "tag96": { "error": "ERR_NEXTRS_UNSUPPORTED" },
        })
    );

    for section in ["crypto", "formData", "buffers"] {
        for (key, value) in expected[section].as_object().unwrap() {
            assert_eq!(&actual[section][key], value, "{section}.{key}");
        }
        assert_eq!(actual[section], expected[section], "{section}");
    }
}

/// Asymmetric signatures, key formats, HKDF and AES-CBC, checked against
/// Node: tests/fixtures/handlers/webcrypto-asym.expected.json is the
/// `webcrypto` part of `node dist/webcrypto-asym.js` (Node 22). Every
/// value must match; `nextrsOnly` holds NextRS's documented refusals,
/// where Node implements more.
#[tokio::test(flavor = "multi_thread")]
async fn asymmetric_web_crypto_matches_node() {
    let executor = executor(Limits::default());
    let response = collect(&executor, request("webcrypto-asym.js", &[])).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let mut actual: Value = serde_json::from_str(&response.text()).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "fixtures/handlers/webcrypto-asym.expected.json"
    ))
    .unwrap();
    let refusals = actual
        .as_object_mut()
        .unwrap()
        .remove("nextrsOnly")
        .unwrap();
    // Where Node's result is not the one Web Crypto specifies, NextRS
    // follows the specification (or refuses something ring cannot do):
    let mut expected = expected;
    let differences = [
        // Node names this DOMException InvalidAccessException.
        (
            "p256NotExtractable",
            json!({ "error": "InvalidAccessError" }),
        ),
        (
            "p384NotExtractable",
            json!({ "error": "InvalidAccessError" }),
        ),
        (
            "genP256PrivateExport",
            json!({ "error": "InvalidAccessError" }),
        ),
        (
            "genP384PrivateExport",
            json!({ "error": "InvalidAccessError" }),
        ),
        (
            "genEd25519PrivateExport",
            json!({ "error": "InvalidAccessError" }),
        ),
        // Exporting a private key as spki (or a public one as pkcs8) is an
        // InvalidAccessError in Web Crypto; Node says NotSupportedError.
        (
            "p256PrivateAsSpki",
            json!({ "error": "InvalidAccessError" }),
        ),
        (
            "p256PublicAsPkcs8",
            json!({ "error": "InvalidAccessError" }),
        ),
        (
            "p384PrivateAsSpki",
            json!({ "error": "InvalidAccessError" }),
        ),
        (
            "p384PublicAsPkcs8",
            json!({ "error": "InvalidAccessError" }),
        ),
        // DER with a trailing byte is not a valid SubjectPublicKeyInfo;
        // Node (OpenSSL) ignores the byte.
        ("p256TrailingSpki", json!({ "error": "DataError" })),
        ("p384TrailingSpki", json!({ "error": "DataError" })),
        // A private JWK whose CRT exponents are swapped: OpenSSL detects
        // the fault and signs without CRT; ring refuses to sign with an
        // inconsistent key.
        ("rsaJwkWrongCrt", json!({ "error": "OperationError" })),
    ];
    for (key, value) in differences {
        assert_ne!(expected[key], value, "{key} is no longer a difference");
        expected[key] = value;
    }
    let mut mismatches = Vec::new();
    for (key, value) in expected.as_object().unwrap() {
        if &actual["webcrypto"][key] != value {
            mismatches.push(format!(
                "{key}: expected {value}, got {}",
                actual["webcrypto"][key]
            ));
        }
    }
    for key in actual["webcrypto"].as_object().unwrap().keys() {
        if expected.get(key).is_none() {
            mismatches.push(format!("{key}: not in the expected results"));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    assert_eq!(
        refusals,
        json!({
            "p521": { "error": "NotSupportedError" },
            "p256Sha384": { "error": "NotSupportedError" },
            "compressedPoint": { "error": "NotSupportedError" },
            "rsa1024": { "error": "NotSupportedError" },
            "rsaGenerate": { "error": "ERR_NEXTRS_UNSUPPORTED" },
            "rsaSha1": { "error": "NotSupportedError" },
            "pssSalt0": { "error": "NotSupportedError" },
            "ecdh": { "error": "ERR_NEXTRS_UNSUPPORTED" },
            "rsaOaep": { "error": "ERR_NEXTRS_UNSUPPORTED" },
            "deriveKey": { "error": "ERR_NEXTRS_UNSUPPORTED" },
        })
    );
}
