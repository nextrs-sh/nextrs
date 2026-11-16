//! TextEncoder.encode checked against Node:
//! tests/fixtures/handlers/text-encoder.expected.json is the output of
//! `node dist/text-encoder.js` (Node 24.18.0), and the same checks run in
//! the embedded runtime must produce it exactly. The cases include the
//! boundaries of the short-ASCII fast path that Phase 3A measured and
//! dropped (src/web_patches.rs).

mod common;

use common::{collect, executor, request};
use nextrs_runtime::Limits;
use serde_json::Value;

fn differences(path: &str, a: &Value, b: &Value, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                differences(
                    &format!("{path}.{k}"),
                    v,
                    y.get(k).unwrap_or(&Value::Null),
                    out,
                );
            }
            for k in y.keys().filter(|k| !x.contains_key(*k)) {
                out.push(format!("{path}.{k}: only in Node"));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (v, w)) in x.iter().zip(y).enumerate() {
                differences(&format!("{path}[{i}]"), v, w, out);
            }
        }
        _ if a != b => out.push(format!("{path}: NextRS {a} != Node {b}")),
        _ => {}
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn text_encoder_matches_node() {
    let executor = executor(Limits::default());
    let response = collect(&executor, request("text-encoder.js", &[])).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let actual: Value = serde_json::from_str(&response.text()).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/handlers/text-encoder.expected.json")).unwrap();
    let mut diffs = Vec::new();
    differences("", &actual, &expected, &mut diffs);
    assert!(
        diffs.is_empty(),
        "{} differences from Node, first ones:\n{}",
        diffs.len(),
        diffs
            .iter()
            .take(60)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
