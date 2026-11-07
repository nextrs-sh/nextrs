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
    DEFAULT_PROXY_BODY_LIMIT, HasSpec, InspectError, ProxyInfo, ProxyMatcher, RoutingRules,
    RuleSpec, parse_byte_size,
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

fn rule(item: &Value) -> Option<RuleSpec> {
    let text = |key: &str| item.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(RuleSpec {
        source: text("source")?,
        regex: text("regex")?,
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
        headers: item
            .get("headers")
            .and_then(Value::as_array)
            .map(|headers| {
                headers
                    .iter()
                    .filter_map(|h| {
                        Some((
                            h.get("key")?.as_str()?.to_owned(),
                            h.get("value")?.as_str()?.to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default(),
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
        .filter_map(|item| {
            let parsed = rule(item);
            if parsed.is_none() {
                invalid.push(format!("{list}: {item}"));
            }
            parsed
        })
        .collect()
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
        on_match_headers: rules(manifest.get("onMatchHeaders"), invalid, "onMatchHeaders"),
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
