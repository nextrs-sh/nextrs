//! NextRS's port of Next's routing functions against Next itself:
//! `tests/data/routing-vectors.json` holds what Next.js's own functions
//! (`getPathMatch` as `buildCustomRoute` configures it, `matchHas`,
//! `prepareDestination`, `compileNonPath`, `stringifyQuery` + `url.format`
//! for redirects, `getMiddlewareRouteMatcher`, `getRelativeURL`) answered
//! for a corpus of rules and requests. Regenerate it with
//! `node scripts/routing-vectors.mjs` after a Next.js upgrade.

use std::collections::BTreeMap;

use nextrs_adapter::{HasSpec, ProxyMatcher, RuleSpec};
use nextrs_server::routing::destination::{RequestView, compile_non_path, prepare_destination};
use nextrs_server::routing::query::{
    JsMap, Value, normalize_repeated_slashes, parse_search, stringify_redirect_query,
};
use nextrs_server::routing::url::{format_url, relative_url};
use nextrs_server::routing::{ProxyMatchers, Rule, RuleKind};
use serde_json::Value as Json;

fn vectors() -> Json {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/routing-vectors.json"
    );
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn has_list(value: &Json) -> Vec<HasSpec> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|h| HasSpec {
            kind: h["type"].as_str().unwrap().to_owned(),
            key: h["key"].as_str().map(str::to_owned),
            value: h["value"].as_str().map(str::to_owned),
        })
        .collect()
}

/// A JSON params/query object as an order-insensitive map, for comparison.
fn json_map(value: &Json) -> BTreeMap<String, Value> {
    value
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(k, v)| {
            let v = match v {
                Json::String(s) => Value::Str(s.clone()),
                Json::Array(items) => Value::Arr(
                    items
                        .iter()
                        .map(|i| i.as_str().unwrap_or_default().to_owned())
                        .collect(),
                ),
                // `undefined` in JavaScript does not survive JSON.
                _ => return None,
            };
            Some((k.clone(), v))
        })
        .collect()
}

fn js_map(map: &JsMap) -> BTreeMap<String, Value> {
    map.iter().map(|(k, v)| (k.to_owned(), v.clone())).collect()
}

fn headers(value: &Json) -> JsMap {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            (
                k.to_ascii_lowercase(),
                Value::Str(v.as_str().unwrap().to_owned()),
            )
        })
        .collect()
}

#[test]
fn rules_match_and_build_destinations_as_next_does() {
    let data = vectors();
    let rules: Vec<(String, Rule)> = data["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let kind = match r["kind"].as_str().unwrap() {
                "redirect" => RuleKind::Redirect,
                "rewrite" => RuleKind::AfterFiles,
                _ => RuleKind::Header,
            };
            let spec = RuleSpec {
                source: r["source"].as_str().unwrap().to_owned(),
                regex: r["regex"].as_str().unwrap().to_owned(),
                destination: r["destination"].as_str().map(str::to_owned),
                status: None,
                headers: r["headers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|h| {
                        (
                            h["key"].as_str().unwrap().to_owned(),
                            h["value"].as_str().unwrap().to_owned(),
                        )
                    })
                    .collect(),
                has: has_list(&r["has"]),
                missing: has_list(&r["missing"]),
                internal: r["internal"].as_bool().unwrap(),
            };
            let rule = Rule::compile(kind, &spec, false)
                .unwrap_or_else(|e| panic!("{}: {e}", spec.source));
            (r["kind"].as_str().unwrap().to_owned(), rule)
        })
        .collect();
    let requests = data["requests"].as_array().unwrap();

    let mut failures = Vec::new();
    let mut matched = 0;
    let mut destinations = 0;
    let mut locations = 0;
    for v in data["vectors"].as_array().unwrap() {
        let (kind, rule) = &rules[v["rule"].as_u64().unwrap() as usize];
        let request = &requests[v["request"].as_u64().unwrap() as usize];
        let path = v["path"].as_str().unwrap();
        let query = parse_search(request["query"].as_str().unwrap());
        let request_headers = headers(&request["headers"]);
        let view = RequestView {
            headers: &request_headers,
            query: &query,
        };
        let label = format!(
            "{} {path}{}",
            rule.source,
            request["query"].as_str().unwrap()
        );
        let got = rule.matches(path, &view);
        let expected = (!v["params"].is_null()).then(|| json_map(&v["params"]));
        if got.as_ref().map(js_map) != expected {
            failures.push(format!(
                "{label}: params {:?} expected {expected:?}",
                got.as_ref().map(js_map)
            ));
            continue;
        }
        let Some(params) = got else { continue };
        matched += 1;
        if let Some(expected) = v["headers"].as_array() {
            for (i, (key, value)) in rule.headers.iter().enumerate() {
                let compiled = if params.is_empty() {
                    (key.clone(), value.clone())
                } else {
                    (
                        compile_non_path(key, &params).unwrap(),
                        compile_non_path(value, &params).unwrap(),
                    )
                };
                let want = (
                    expected[i][0].as_str().unwrap().to_owned(),
                    expected[i][1].as_str().unwrap().to_owned(),
                );
                if compiled != want {
                    failures.push(format!("{label}: header {compiled:?} expected {want:?}"));
                }
            }
        }
        let Some(destination) = &rule.destination else {
            continue;
        };
        let dest = prepare_destination(destination, &params, &query, kind == "rewrite");
        if let Some(error) = v["destError"].as_str() {
            if dest.is_ok() {
                failures.push(format!(
                    "{label}: built a destination where Next throws {error:?}"
                ));
            }
            continue;
        }
        let mut dest = match dest {
            Ok(dest) => dest,
            Err(error) => {
                failures.push(format!("{label}: {error}"));
                continue;
            }
        };
        destinations += 1;
        let e = &v["dest"];
        let checks = [
            (
                "pathname",
                Some(dest.pathname.clone()),
                e["pathname"].as_str().map(str::to_owned),
            ),
            (
                "hash",
                Some(dest.hash.clone()),
                e["hash"].as_str().map(str::to_owned),
            ),
            (
                "search",
                Some(dest.search.clone()),
                e["search"].as_str().map(str::to_owned),
            ),
            (
                "protocol",
                dest.protocol.clone(),
                e["protocol"].as_str().map(str::to_owned),
            ),
            (
                "hostname",
                dest.hostname.clone().filter(|h| !h.is_empty()),
                e["hostname"]
                    .as_str()
                    .map(str::to_owned)
                    .filter(|h| !h.is_empty()),
            ),
        ];
        for (field, got, want) in checks {
            if got != want {
                failures.push(format!("{label}: {field} {got:?} expected {want:?}"));
            }
        }
        if js_map(&dest.query) != json_map(&e["query"]) {
            failures.push(format!(
                "{label}: query {:?} expected {}",
                js_map(&dest.query),
                e["query"]
            ));
        }
        if let Some(location) = v["location"].as_str() {
            dest.search = stringify_redirect_query(&dest.query, &query);
            dest.pathname = normalize_repeated_slashes(&dest.pathname);
            locations += 1;
            let got = format_url(&dest);
            if got != location {
                failures.push(format!("{label}: Location {got:?} expected {location:?}"));
            }
        }
    }
    assert!(matched > 300, "only {matched} vectors matched a rule");
    assert!(
        destinations > 150 && locations > 80,
        "{destinations} destinations, {locations} locations compared"
    );
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures[..failures.len().min(30)].join("\n")
    );
}

#[test]
fn proxy_matchers_match_as_next_does() {
    let data = vectors();
    let mut failures = Vec::new();
    for v in data["matchers"].as_array().unwrap() {
        let specs: Vec<ProxyMatcher> = v["matchers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| ProxyMatcher {
                regexp: m["regexp"].as_str().unwrap().to_owned(),
                original_source: None,
                has: m.get("has").map(has_list).unwrap_or_default(),
                missing: m.get("missing").map(has_list).unwrap_or_default(),
            })
            .collect();
        let matchers = ProxyMatchers::compile(&specs).unwrap();
        let request_headers = headers(&v["requestHeaders"]);
        let query = JsMap::new();
        let view = RequestView {
            headers: &request_headers,
            query: &query,
        };
        let path = v["path"].as_str().unwrap();
        let expected = v["matches"].as_bool().unwrap();
        if matchers.matches_path(path, &view) != expected {
            failures.push(format!(
                "set {} {path} {:?}: expected {expected}",
                v["set"], v["requestHeaders"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn relative_urls_as_next_computes_them() {
    let data = vectors();
    for v in data["relatives"].as_array().unwrap() {
        let got = relative_url(v["url"].as_str().unwrap(), v["base"].as_str().unwrap()).unwrap();
        assert_eq!(got, v["relative"].as_str().unwrap(), "{v}");
    }
}
