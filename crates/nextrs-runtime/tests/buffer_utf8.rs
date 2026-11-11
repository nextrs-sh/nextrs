//! Buffer's UTF-8 conversions checked against Node:
//! tests/fixtures/handlers/buffer-utf8.expected.json is the output of
//! `node dist/buffer-utf8.js` (Node 24.18.0), and the same checks run in the
//! embedded runtime must produce it exactly. Encoding and decoding are on
//! the server-rendering hot path, so changes to how they are implemented
//! are held to Node's results, edge cases included.

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
async fn buffer_utf8_conversions_match_node() {
    let executor = executor(Limits::default());
    let response = collect(&executor, request("buffer-utf8.js", &[])).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let actual: Value = serde_json::from_str(&response.text()).unwrap();
    let expected: Value =
        serde_json::from_str(include_str!("fixtures/handlers/buffer-utf8.expected.json")).unwrap();
    let mut diffs = Vec::new();
    differences("", &actual, &expected, &mut diffs);
    assert!(
        diffs.is_empty(),
        "{} differences from Node, first ones:\n{}",
        diffs.len(),
        diffs
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Diagnosis aid: prints one random decode case's full output
/// (`NEXTRS_DECODE_CASE=14 cargo test -p nextrs-runtime --test buffer_utf8 -- --ignored --nocapture`);
/// compare with `node tests/fixtures/handlers/dist/buffer-utf8.js 14`.
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn print_one_random_decode_case() {
    let case = std::env::var("NEXTRS_DECODE_CASE").unwrap_or_else(|_| "0".into());
    let executor = executor(Limits::default());
    let response = collect(
        &executor,
        request("buffer-utf8.js", &[("x-decode-case", case.as_str())]),
    )
    .await;
    let value: Value = serde_json::from_str(&response.text()).unwrap();
    println!("{}", serde_json::to_string_pretty(&value).unwrap());
}
