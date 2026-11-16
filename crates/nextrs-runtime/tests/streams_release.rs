//! Releasing WHATWG stream readers and writers, checked against Node:
//! tests/fixtures/handlers/streams-release.expected.json is the output of
//! `node dist/streams-release.js` (Node 24.18.0). NextRS caches the release
//! errors as Node does (src/web_patches.rs); this holds the observable
//! behavior, error identity included, to Node's.

mod common;

use common::{collect, executor, request};
use nextrs_runtime::Limits;
use serde_json::Value;

#[tokio::test(flavor = "multi_thread")]
async fn stream_release_behavior_matches_node() {
    let executor = executor(Limits::default());
    let response = collect(&executor, request("streams-release.js", &[])).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let actual: Value = serde_json::from_str(&response.text()).unwrap();
    let expected: Value = serde_json::from_str(include_str!(
        "fixtures/handlers/streams-release.expected.json"
    ))
    .unwrap();
    let (Value::Object(actual), Value::Object(expected)) = (&actual, &expected) else {
        panic!("expected objects");
    };
    let mut diffs = Vec::new();
    for (key, want) in expected {
        let got = actual.get(key).unwrap_or(&Value::Null);
        if got != want {
            diffs.push(format!("{key}: NextRS {got} != Node {want}"));
        }
    }
    for key in actual.keys().filter(|k| !expected.contains_key(*k)) {
        diffs.push(format!("{key}: only in NextRS"));
    }
    assert!(
        diffs.is_empty(),
        "differences from Node:\n{}",
        diffs.join("\n")
    );
}
