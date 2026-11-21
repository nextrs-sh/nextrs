//! Routing rules and the compiled proxy, read from the manifests
//! `next start` reads: `routes-manifest.json` (headers, redirects,
//! rewrites, as Next compiled them from `next.config`) and
//! `server/functions-config-manifest.json` (the Node.js proxy's matchers).
//!
//! Nothing is reinterpreted here: sources keep the regex Next generated,
//! and `has`/`missing` conditions are passed through.

use std::path::Path;

use serde_json::Value;

use crate::manifests::read_json_optional;
use crate::{
    DEFAULT_PROXY_BODY_LIMIT, DEFAULT_PROXY_TIMEOUT_MS, HasSpec, InspectError, ProxyInfo,
    ProxyMatcher, RoutingRules, RuleSpec, parse_byte_size,
};

fn has_list(value: Option<&Value>) -> Vec<HasSpec> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| HasSpec {
                    kind: item
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    key: item.get("key").and_then(Value::as_str).map(str::to_owned),
                    value: item.get("value").and_then(Value::as_str).map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One rule, or why it cannot be applied as written: a rule without its
/// source or regex, or a header entry that is not a `{ key, value }` pair
/// of strings (dropping it could drop a security header).
fn rule(item: &Value) -> Result<RuleSpec, &'static str> {
    let text = |key: &str| item.get(key).and_then(Value::as_str).map(str::to_owned);
    let headers = match item.get("headers") {
        None => Vec::new(),
        Some(Value::Array(headers)) => headers
            .iter()
            .map(|h| {
                Some((
                    h.get("key")?.as_str()?.to_owned(),
                    h.get("value")?.as_str()?.to_owned(),
                ))
            })
            .collect::<Option<Vec<_>>>()
            .ok_or("a header entry is not a { key, value } pair of strings")?,
        Some(_) => return Err("headers is not an array"),
    };
    Ok(RuleSpec {
        source: text("source").ok_or("rule without source or regex")?,
        regex: text("regex").ok_or("rule without source or regex")?,
        destination: text("destination"),
        status: item
            .get("statusCode")
            .and_then(Value::as_u64)
            .and_then(|s| u16::try_from(s).ok())
            .or_else(|| {
                // Next resolves `permanent` into `statusCode` at build time;
                // honour it if a manifest still carries it.
                item.get("permanent")
                    .and_then(Value::as_bool)
                    .map(|p| if p { 308 } else { 307 })
            }),
        headers,
        has: has_list(item.get("has")),
        missing: has_list(item.get("missing")),
        internal: item
            .get("internal")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn rules(value: Option<&Value>, invalid: &mut Vec<String>, list: &str) -> Vec<RuleSpec> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| match rule(item) {
            Ok(rule) => Some(rule),
            Err(reason) => {
                invalid.push(format!("{list} {reason}: {item}"));
                None
            }
        })
        .collect()
}

/// The keys an `onMatchHeaders` entry may have: Next writes them from
/// `loadCustomRoutes` (header rules for a deployment ID) through
/// `buildCustomRoute('header')`, which adds `regex`.
const ON_MATCH_KEYS: &[&str] = &["source", "regex", "headers", "has", "missing"];

/// `onMatchHeaders`: header rules resolve-routes applies once routing
/// found an output (a file, a page or a route). Applied exactly like the
/// `headers` list, so anything that is not a plain header rule is refused
/// rather than reinterpreted: an unknown key (a destination would make
/// handleRoute rewrite), no headers, or a malformed `has` / `missing`.
fn on_match_rules(value: Option<&Value>, invalid: &mut Vec<String>) -> Vec<RuleSpec> {
    const LIST: &str = "onMatchHeaders";
    let items = match value {
        None | Some(Value::Null) => return Vec::new(),
        Some(Value::Array(items)) => items,
        Some(other) => {
            invalid.push(format!("{LIST} is not an array: {other}"));
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for item in items {
        let Some(object) = item.as_object() else {
            invalid.push(format!("{LIST} entry is not an object: {item}"));
            continue;
        };
        if let Some(key) = object.keys().find(|k| !ON_MATCH_KEYS.contains(&k.as_str())) {
            invalid.push(format!(
                "{LIST} entry has an unsupported key {key:?}: {item}"
            ));
            continue;
        }
        if let Some(reason) = ["has", "missing"]
            .into_iter()
            .find_map(|key| condition_problem(object.get(key)))
        {
            invalid.push(format!("{LIST} entry {reason}: {item}"));
            continue;
        }
        match rule(item) {
            Ok(rule) if rule.headers.is_empty() => {
                invalid.push(format!("{LIST} entry sets no headers: {item}"));
            }
            Ok(rule) => out.push(rule),
            Err(reason) => invalid.push(format!("{LIST} {reason}: {item}")),
        }
    }
    out
}

/// What is wrong with a `has` / `missing` list, if anything: Next's
/// `checkCustomRoutes` shape (`type` one of header, cookie, query or host;
/// `key` a string except for host, which needs a `value`; `value` an
/// optional string).
fn condition_problem(value: Option<&Value>) -> Option<String> {
    let items = match value {
        None => return None,
        Some(Value::Array(items)) => items,
        Some(_) => return Some("has a has/missing that is not an array".into()),
    };
    for item in items {
        let kind = item.get("type").and_then(Value::as_str);
        let key = item.get("key");
        let value = item.get("value");
        let ok = match kind {
            Some("header" | "cookie" | "query") => key.is_some_and(Value::is_string),
            Some("host") => value.is_some_and(Value::is_string),
            _ => false,
        } && value.is_none_or(Value::is_string)
            && item.as_object().is_some_and(|o| {
                o.keys()
                    .all(|k| matches!(k.as_str(), "type" | "key" | "value"))
            });
        if !ok {
            return Some(format!("has an unsupported has/missing condition {item}"));
        }
    }
    None
}

/// The rule lists of `routes-manifest.json`. Entries without a source or
/// regex are reported in `invalid` (refused by the caller: a rule that is
/// silently skipped could be a security header).
pub(crate) fn routing_rules(
    dist_dir: &Path,
    invalid: &mut Vec<String>,
) -> Result<RoutingRules, InspectError> {
    let manifest: Value =
        read_json_optional(&dist_dir.join("routes-manifest.json"))?.unwrap_or(Value::Null);
    let rewrites = manifest.get("rewrites");
    let (before_files, after_files, fallback) = match rewrites {
        // Legacy shape: a plain array is afterFiles.
        Some(Value::Array(_)) => (Vec::new(), rules(rewrites, invalid, "rewrites"), Vec::new()),
        Some(groups) => (
            rules(groups.get("beforeFiles"), invalid, "beforeFiles"),
            rules(groups.get("afterFiles"), invalid, "afterFiles"),
            rules(groups.get("fallback"), invalid, "fallback"),
        ),
        None => (Vec::new(), Vec::new(), Vec::new()),
    };
    Ok(RoutingRules {
        case_sensitive: manifest
            .get("caseSensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        headers: rules(manifest.get("headers"), invalid, "headers"),
        redirects: rules(manifest.get("redirects"), invalid, "redirects"),
        before_files,
        after_files,
        fallback,
        proxy_timeout_ms: Some(DEFAULT_PROXY_TIMEOUT_MS),
        on_match_headers: on_match_rules(manifest.get("onMatchHeaders"), invalid),
    })
}

/// The Node.js proxy, as `next start` finds it: listed in
/// `functions-config-manifest.json` as `/_middleware`, compiled to
/// `server/middleware.js`. Without matchers it runs on every request.
pub(crate) fn proxy_info(
    dist_dir: &Path,
    config: Option<&Value>,
) -> Result<Option<ProxyInfo>, InspectError> {
    let functions: Value =
        read_json_optional(&dist_dir.join("server/functions-config-manifest.json"))?
            .unwrap_or(Value::Null);
    let Some(function) = functions
        .get("functions")
        .and_then(|f| f.get("/_middleware"))
    else {
        return Ok(None);
    };
    let matchers = match function.get("matchers").and_then(Value::as_array) {
        Some(items) => items
            .iter()
            .filter_map(|m| {
                Some(ProxyMatcher {
                    regexp: m.get("regexp")?.as_str()?.to_owned(),
                    original_source: m
                        .get("originalSource")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    has: has_list(m.get("has")),
                    missing: has_list(m.get("missing")),
                })
            })
            .collect(),
        None => vec![ProxyMatcher {
            regexp: ".*".into(),
            original_source: Some("/:path*".into()),
            has: Vec::new(),
            missing: Vec::new(),
        }],
    };
    let limit = config
        .and_then(|c| c.get("experimental"))
        .and_then(|e| {
            e.get("proxyClientMaxBodySize")
                .or_else(|| e.get("middlewareClientMaxBodySize"))
        })
        .and_then(|v| match v {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => parse_byte_size(s),
            _ => None,
        })
        .unwrap_or(DEFAULT_PROXY_BODY_LIMIT);
    Ok(Some(ProxyInfo {
        entry: dist_dir.join("server/middleware.js"),
        matchers,
        client_max_body_size: limit,
    }))
}

/// `experimental.proxyTimeout` from the resolved next.config
/// (router-server passes `proxyTimeout === null ? undefined : proxyTimeout || 30000`).
pub(crate) fn proxy_timeout_ms(config: Option<&Value>) -> Option<u64> {
    match config
        .and_then(|c| c.get("experimental"))
        .and_then(|e| e.get("proxyTimeout"))
    {
        Some(Value::Null) => None,
        Some(Value::Number(n)) => match n.as_f64() {
            Some(ms) if ms > 0.0 => Some(ms.ceil() as u64),
            _ => Some(DEFAULT_PROXY_TIMEOUT_MS),
        },
        _ => Some(DEFAULT_PROXY_TIMEOUT_MS),
    }
}

#[cfg(test)]
mod proxy_timeout_tests {
    use super::{DEFAULT_PROXY_TIMEOUT_MS, proxy_timeout_ms};
    use serde_json::json;

    /// router-server's `proxyTimeout === null ? undefined : proxyTimeout || 30000`.
    #[test]
    fn proxy_timeout_follows_router_server() {
        let with = |v| json!({ "experimental": { "proxyTimeout": v } });
        assert_eq!(proxy_timeout_ms(None), Some(DEFAULT_PROXY_TIMEOUT_MS));
        assert_eq!(
            proxy_timeout_ms(Some(&json!({}))),
            Some(DEFAULT_PROXY_TIMEOUT_MS)
        );
        assert_eq!(proxy_timeout_ms(Some(&with(json!(5000)))), Some(5000));
        assert_eq!(
            proxy_timeout_ms(Some(&with(json!(0)))),
            Some(DEFAULT_PROXY_TIMEOUT_MS)
        );
        assert_eq!(proxy_timeout_ms(Some(&with(json!(null)))), None);
        assert_eq!(proxy_timeout_ms(Some(&with(json!(1.5)))), Some(2));
    }
}

#[cfg(test)]
mod on_match_tests {
    use super::{on_match_rules, rules};
    use serde_json::{Value, json};

    /// What Next 16.4.6 writes for `deploymentId: "dpl-1"`.
    fn next_generated() -> Value {
        json!([
            {
                "source": "/:path*",
                "has": [{ "type": "header", "key": "rsc", "value": "1" }],
                "headers": [{ "key": "x-nextjs-deployment-id", "value": "dpl-1" }],
                "regex": "^(?:/((?:[^/]+?)(?:/(?:[^/]+?))*))?(?:/)?$"
            },
            {
                "source": "/_next/data/(.*)",
                "headers": [{ "key": "x-nextjs-deployment-id", "value": "dpl-1" }],
                "regex": "^/_next/data(?:/(.*))(?:/)?$"
            }
        ])
    }

    fn parse(value: Value) -> (usize, Vec<String>) {
        let mut invalid = Vec::new();
        let rules = on_match_rules(Some(&value), &mut invalid);
        (rules.len(), invalid)
    }

    #[test]
    fn accepts_the_rules_next_writes_for_a_deployment_id() {
        let mut invalid = Vec::new();
        let value = next_generated();
        let parsed = on_match_rules(Some(&value), &mut invalid);
        assert!(invalid.is_empty(), "{invalid:?}");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].has.len(), 1);
        assert_eq!(parsed[0].has[0].kind, "header");
        assert_eq!(parsed[0].has[0].key.as_deref(), Some("rsc"));
        assert_eq!(parsed[0].has[0].value.as_deref(), Some("1"));
        assert_eq!(
            parsed[1].headers,
            vec![("x-nextjs-deployment-id".to_owned(), "dpl-1".to_owned())]
        );
        assert!(parsed.iter().all(|r| r.destination.is_none()));
    }

    #[test]
    fn absent_or_empty_lists_are_nothing() {
        assert_eq!(parse(json!([])), (0, vec![]));
        let mut invalid = Vec::new();
        assert!(on_match_rules(None, &mut invalid).is_empty());
        assert!(on_match_rules(Some(&Value::Null), &mut invalid).is_empty());
        assert!(invalid.is_empty());
    }

    #[test]
    fn refuses_what_is_not_a_plain_header_rule() {
        let base = || next_generated()[1].clone();
        let with = |key: &str, value: Value| {
            let mut rule = base();
            rule[key] = value;
            json!([rule])
        };
        let without = |key: &str| {
            let mut rule = base();
            rule.as_object_mut().unwrap().remove(key);
            json!([rule])
        };
        for (name, manifest, reason) in [
            ("not an array", json!({}), "is not an array"),
            ("entry not an object", json!(["x"]), "is not an object"),
            (
                "destination",
                with("destination", json!("/elsewhere")),
                "unsupported key \"destination\"",
            ),
            (
                "statusCode",
                with("statusCode", json!(307)),
                "unsupported key \"statusCode\"",
            ),
            (
                "basePath",
                with("basePath", json!(false)),
                "unsupported key \"basePath\"",
            ),
            ("no source", without("source"), "without source or regex"),
            ("no regex", without("regex"), "without source or regex"),
            ("no headers", without("headers"), "sets no headers"),
            (
                "empty headers",
                with("headers", json!([])),
                "sets no headers",
            ),
            (
                "headers not array",
                with("headers", json!({ "key": "a" })),
                "headers is not an array",
            ),
            (
                "header without value",
                with("headers", json!([{ "key": "a" }])),
                "{ key, value }",
            ),
            (
                "header value not string",
                with("headers", json!([{ "key": "a", "value": 1 }])),
                "{ key, value }",
            ),
            (
                "has not array",
                with("has", json!({ "type": "header" })),
                "not an array",
            ),
            (
                "has unknown type",
                with("has", json!([{ "type": "ip", "key": "a" }])),
                "unsupported has/missing",
            ),
            (
                "has without key",
                with("has", json!([{ "type": "header" }])),
                "unsupported has/missing",
            ),
            (
                "has value not string",
                with(
                    "has",
                    json!([{ "type": "header", "key": "rsc", "value": 1 }]),
                ),
                "unsupported has/missing",
            ),
            (
                "host without value",
                with("missing", json!([{ "type": "host" }])),
                "unsupported has/missing",
            ),
            (
                "condition extra key",
                with(
                    "has",
                    json!([{ "type": "header", "key": "rsc", "regex": "1" }]),
                ),
                "unsupported has/missing",
            ),
        ] {
            let (count, invalid) = parse(manifest);
            assert_eq!(count, 0, "{name}: accepted");
            assert_eq!(invalid.len(), 1, "{name}: {invalid:?}");
            assert!(invalid[0].contains(reason), "{name}: {invalid:?}");
            assert!(
                invalid[0].starts_with("onMatchHeaders"),
                "{name}: {invalid:?}"
            );
        }
    }

    #[test]
    fn accepts_the_conditions_next_accepts() {
        let rule = |has: Value| {
            json!([{
                "source": "/:path*",
                "regex": "^(?:/((?:[^/]+?)(?:/(?:[^/]+?))*))?(?:/)?$",
                "has": has,
                "headers": [{ "key": "x-a", "value": "b" }]
            }])
        };
        for has in [
            json!([{ "type": "cookie", "key": "c" }]),
            json!([{ "type": "query", "key": "q", "value": "v" }]),
            json!([{ "type": "host", "value": "example.com" }]),
            json!([{ "type": "host", "key": "ignored", "value": "example.com" }]),
        ] {
            assert_eq!(parse(rule(has.clone())), (1, vec![]), "{has}");
        }
    }

    /// The other lists now report a malformed header entry instead of
    /// dropping it.
    #[test]
    fn header_lists_refuse_malformed_header_entries() {
        let mut invalid = Vec::new();
        let value = json!([
            { "source": "/a", "regex": "^/a$", "headers": [{ "key": "x-frame-options" }] },
            { "source": "/b", "regex": "^/b$", "headers": [{ "key": "x-b", "value": "1" }] }
        ]);
        let parsed = rules(Some(&value), &mut invalid, "headers");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].source, "/b");
        assert_eq!(invalid.len(), 1);
        assert!(
            invalid[0].starts_with("headers a header entry"),
            "{invalid:?}"
        );
    }
}
