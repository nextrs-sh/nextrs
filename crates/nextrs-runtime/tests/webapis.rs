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
            "ecdsa": { "error": "ERR_NEXTRS_UNSUPPORTED" },
            "hmacJwk": { "error": "ERR_NEXTRS_UNSUPPORTED" },
            "jwk": { "error": "ERR_NEXTRS_UNSUPPORTED" },
        })
    );

    for section in ["crypto", "formData", "buffers"] {
        for (key, value) in expected[section].as_object().unwrap() {
            assert_eq!(&actual[section][key], value, "{section}.{key}");
        }
        assert_eq!(actual[section], expected[section], "{section}");
    }
}
