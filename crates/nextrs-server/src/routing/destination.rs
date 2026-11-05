//! Ports of `matchHas`, `compileNonPath` and `prepareDestination`
//! (`next/dist/shared/lib/router/utils/prepare-destination.js`).

use fancy_regex::Regex;

use super::path_to_regexp::{self, KeyName};
use super::query::{JsMap, Value, encode_uri_component, parse_cookies};
use super::url::{ParsedUrl, parse_url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HasKind {
    Header,
    Cookie,
    Query,
    Host,
}

/// One `has` / `missing` item, its value compiled as Next compiles it
/// (`new RegExp("^" + value + "$")`, case-sensitive).
#[derive(Debug, Clone)]
pub struct HasCondition {
    pub kind: HasKind,
    pub key: String,
    pub value: Option<Regex>,
}

impl HasCondition {
    pub fn new(kind: HasKind, key: Option<&str>, value: Option<&str>) -> Result<Self, String> {
        let value = match value.filter(|v| !v.is_empty()) {
            Some(value) => Some(
                Regex::new(&format!("^{value}$"))
                    .map_err(|error| format!("has value {value:?}: {error}"))?,
            ),
            None => None,
        };
        Ok(Self {
            kind,
            key: key.unwrap_or_default().to_owned(),
            value,
        })
    }
}

/// What `matchHas` reads of a request: Node's `req.headers` object and the
/// parsed query.
pub struct RequestView<'a> {
    pub headers: &'a JsMap,
    pub query: &'a JsMap,
}

/// `getSafeParamName`: only ASCII letters survive.
fn safe_param_name(name: &str) -> String {
    name.chars().filter(char::is_ascii_alphabetic).collect()
}

/// Next's `matchHas(req, query, has, missing)`: the captured params when
/// every `has` item matches and no `missing` item does.
pub fn match_has(
    request: &RequestView<'_>,
    has: &[HasCondition],
    missing: &[HasCondition],
) -> Option<JsMap> {
    let mut params = JsMap::new();
    let matches = |item: &HasCondition, params: &mut JsMap| -> bool {
        let mut key = item.key.clone();
        let value: Option<Value> = match item.kind {
            HasKind::Header => {
                key = key.to_ascii_lowercase();
                request.headers.get(&key).cloned()
            }
            HasKind::Cookie => {
                let header = request.headers.get("cookie").map(|v| match v {
                    Value::Str(s) => s.clone(),
                    Value::Arr(items) => items.join("; "),
                });
                header
                    .filter(|h| !h.is_empty())
                    .and_then(|h| parse_cookies(&h).get(&item.key).cloned())
            }
            HasKind::Query => request.query.get(&key).cloned(),
            HasKind::Host => request
                .headers
                .get("host")
                .and_then(Value::as_str)
                .map(|host| {
                    Value::Str(
                        host.split(':')
                            .next()
                            .unwrap_or_default()
                            .to_ascii_lowercase(),
                    )
                }),
        };
        let Some(value) = value.filter(Value::is_truthy) else {
            return false;
        };
        let Some(matcher) = &item.value else {
            params.insert(safe_param_name(&key), value);
            return true;
        };
        let subject = match &value {
            Value::Str(s) => s.as_str(),
            Value::Arr(items) => match items.last() {
                Some(last) => last.as_str(),
                // `[].slice(-1)[0]` is undefined: `.match` throws in
                // JavaScript. An empty array never reaches here (no
                // parser produces one).
                None => return false,
            },
        };
        let Ok(Some(captures)) = matcher.captures(subject) else {
            return false;
        };
        let mut named = false;
        for name in matcher.capture_names().flatten() {
            named = true;
            // An unmatched optional group is `undefined` in JavaScript.
            if let Some(group) = captures.name(name) {
                params.insert(name, Value::Str(group.as_str().to_owned()));
            }
        }
        if !named
            && item.kind == HasKind::Host
            && let Some(whole) = captures.get(0).filter(|m| !m.as_str().is_empty())
        {
            params.insert("host", Value::Str(whole.as_str().to_owned()));
        }
        true
    };
    let all_has = has.iter().all(|item| matches(item, &mut params));
    if !all_has {
        return None;
    }
    let mut scratch = JsMap::new();
    if missing.iter().any(|item| matches(item, &mut scratch)) {
        return None;
    }
    Some(params)
}

/// `escapeStringRegexp` + `new RegExp(…, 'g')` replacement of `:param`.
fn escape_segment(value: &str, param: &str) -> String {
    value.replace(&format!(":{param}"), &format!("__ESC_COLON_{param}"))
}

/// `str.replace(/__ESC_COLON_/gi, ':')`.
fn unescape_segments(value: &str) -> String {
    const MARK: &str = "__esc_colon_";
    let lower = value.to_ascii_lowercase();
    if !lower.contains(MARK) {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        if lower[i..].starts_with(MARK) {
            out.push(':');
            i += MARK.len();
        } else {
            let c = value[i..].chars().next().unwrap_or_default();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Next's `compileNonPath(value, params)`: `:param` in a header value or a
/// query value becomes the param, everything else is literal.
pub fn compile_non_path(value: &str, params: &JsMap) -> Result<String, String> {
    if !value.contains(':') {
        return Ok(value.to_owned());
    }
    let mut value = value.to_owned();
    for key in params.keys() {
        if !value.contains(&format!(":{key}")) {
            continue;
        }
        value = value
            .replace(
                &format!(":{key}*"),
                &format!(":{key}--ESCAPED_PARAM_ASTERISKS"),
            )
            .replace(
                &format!(":{key}?"),
                &format!(":{key}--ESCAPED_PARAM_QUESTION"),
            )
            .replace(&format!(":{key}+"), &format!(":{key}--ESCAPED_PARAM_PLUS"));
        // `:${key}(?!\w)`
        let needle = format!(":{key}");
        let mut out = String::with_capacity(value.len());
        let mut rest = value.as_str();
        while let Some(at) = rest.find(&needle) {
            let after = &rest[at + needle.len()..];
            out.push_str(&rest[..at]);
            if after.chars().next().is_some_and(is_word) {
                out.push_str(&needle);
            } else {
                out.push_str("--ESCAPED_PARAM_COLON");
                out.push_str(key);
            }
            rest = after;
        }
        out.push_str(rest);
        value = out;
    }
    let mut escaped = String::with_capacity(value.len() * 2);
    for c in value.chars() {
        if ":*?+(){}".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    let restored = escaped
        .replace("--ESCAPED_PARAM_PLUS", "+")
        .replace("--ESCAPED_PARAM_COLON", ":")
        .replace("--ESCAPED_PARAM_QUESTION", "?")
        .replace("--ESCAPED_PARAM_ASTERISKS", "*");
    let compiled = safe_compile(&format!("/{restored}"), params, str::to_owned)?;
    Ok(compiled.get(1..).unwrap_or_default().to_owned())
}

/// `safeCompile`: Next's normalization of adjacent parameters
/// (`:a:b`, `(.):x`) is not ported; such patterns are refused.
fn safe_compile(
    pattern: &str,
    params: &JsMap,
    encode: impl Fn(&str) -> String,
) -> Result<String, String> {
    if has_adjacent_parameter_issues(pattern) {
        return Err(format!(
            "destination {pattern:?} uses adjacent parameters, which NextRS does not compile"
        ));
    }
    let tokens = path_to_regexp::parse(pattern)?;
    path_to_regexp::compile(&tokens, params, encode)
}

/// `hasAdjacentParameterIssues` (route-pattern-normalizer.js).
fn has_adjacent_parameter_issues(route: &str) -> bool {
    let chars: Vec<char> = route.chars().collect();
    let name_at = |mut i: usize| -> Option<usize> {
        // `[a-zA-Z_][a-zA-Z0-9_]*`, returning the index after it.
        let first = *chars.get(i)?;
        if !(first.is_ascii_alphabetic() || first == '_') {
            return None;
        }
        i += 1;
        while chars
            .get(i)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
        {
            i += 1;
        }
        Some(i)
    };
    for i in 0..chars.len() {
        // /\/\(\.{1,3}\):[^/\s]+/
        if chars[i] == '/' && chars.get(i + 1) == Some(&'(') {
            let dots = chars[i + 2..].iter().take_while(|c| **c == '.').count();
            let j = i + 2 + dots;
            if (1..=3).contains(&dots)
                && chars.get(j) == Some(&')')
                && chars.get(j + 1) == Some(&':')
                && chars
                    .get(j + 2)
                    .is_some_and(|c| *c != '/' && !c.is_whitespace())
            {
                return true;
            }
        }
        // /:[a-zA-Z_][a-zA-Z0-9_]*:[a-zA-Z_][a-zA-Z0-9_]*/
        if chars[i] == ':'
            && let Some(end) = name_at(i + 1)
            && chars.get(end) == Some(&':')
            && name_at(end + 1).is_some()
        {
            return true;
        }
    }
    false
}

/// Next's `parseDestination`: `:param` placeholders survive URL parsing.
fn parse_destination(
    destination: &str,
    params: &JsMap,
    query: &JsMap,
) -> Result<ParsedUrl, String> {
    let mut merged = params.clone();
    for (key, value) in query.iter() {
        if !merged.contains_key(key) {
            merged.insert(key, value.clone());
        }
    }
    let mut escaped = destination.to_owned();
    for param in merged.keys() {
        if !param.is_empty() {
            escaped = escape_segment(&escaped, param);
        }
    }
    let mut parsed = parse_url(&escaped)?;
    parsed.pathname = unescape_segments(&parsed.pathname);
    parsed.href = unescape_segments(&parsed.href);
    parsed.hostname = parsed.hostname.as_deref().map(unescape_segments);
    parsed.hash = unescape_segments(&parsed.hash);
    parsed.search = unescape_segments(&parsed.search);
    parsed.origin = parsed.origin.as_deref().map(unescape_segments);
    Ok(parsed)
}

/// Next's `prepareDestination`: the destination with params substituted
/// in path, host, hash and query, and the request's query merged under
/// the destination's (`parsedDestination.query`).
pub fn prepare_destination(
    destination: &str,
    params: &JsMap,
    query: &JsMap,
    append_params_to_query: bool,
) -> Result<ParsedUrl, String> {
    let mut parsed = parse_destination(destination, params, query)?;
    let dest_hostname = parsed.hostname.clone().filter(|h| !h.is_empty());
    let dest_search = parsed.search.clone();
    let mut dest_path = parsed.pathname.clone();
    if !parsed.hash.is_empty() {
        dest_path.push_str(&parsed.hash);
    }

    let mut dest_params: Vec<String> = Vec::new();
    let path_tokens = if has_adjacent_parameter_issues(&dest_path) {
        return Err(format!(
            "destination {dest_path:?} uses adjacent parameters, which NextRS does not compile"
        ));
    } else {
        path_to_regexp::parse(&dest_path)?
    };
    dest_params.extend(
        path_to_regexp::keys(&path_tokens)
            .iter()
            .map(|k| k.name.as_key()),
    );
    let host_tokens = match &dest_hostname {
        Some(hostname) => {
            let tokens = path_to_regexp::parse(hostname)?;
            dest_params.extend(
                path_to_regexp::keys(&tokens)
                    .iter()
                    .map(|k| k.name.as_key()),
            );
            Some(tokens)
        }
        None => None,
    };

    // Params in query values.
    let mut dest_query = JsMap::new();
    for (key, value) in parsed.query.iter() {
        let compiled = match value {
            Value::Arr(items) => Value::Arr(
                items
                    .iter()
                    .map(|v| compile_non_path(&unescape_segments(v), params))
                    .collect::<Result<_, _>>()?,
            ),
            Value::Str(item) => Value::Str(compile_non_path(&unescape_segments(item), params)?),
        };
        dest_query.insert(key, compiled);
    }

    // Rewrites pass path params on as query values unless the destination
    // already uses one of them.
    let param_keys: Vec<&str> = params
        .keys()
        .filter(|k| *k != "nextInternalLocale")
        .collect();
    if append_params_to_query
        && !param_keys
            .iter()
            .any(|k| dest_params.iter().any(|d| d == k))
    {
        for key in &param_keys {
            if !dest_query.contains_key(key)
                && let Some(value) = params.get(key)
            {
                dest_query.insert(*key, value.clone());
            }
        }
    }

    let new_url = path_to_regexp::compile(&path_tokens, params, str::to_owned).map_err(|error| {
        if error.contains("to not repeat, but got an array") {
            "To use a multi-match in the destination you must add `*` at the end of the param name to signify it should repeat. https://nextjs.org/docs/messages/invalid-multi-match".to_owned()
        } else {
            error
        }
    })?;
    // `newUrl.split('#', 2)`
    let mut pieces = new_url.split('#');
    let pathname = pieces.next().unwrap_or_default().to_owned();
    let hash = pieces.next().unwrap_or_default().to_owned();
    if let Some(tokens) = host_tokens {
        parsed.hostname = Some(path_to_regexp::compile(
            &tokens,
            params,
            encode_uri_component,
        )?);
    }
    parsed.pathname = pathname;
    parsed.hash = if hash.is_empty() {
        String::new()
    } else {
        format!("#{hash}")
    };
    parsed.search = if dest_search.is_empty() {
        String::new()
    } else {
        compile_non_path(&dest_search, params)?
    };
    let mut merged = query.clone();
    merged.assign(&dest_query);
    parsed.query = merged;
    Ok(parsed)
}

/// Unnamed keys are not params of a config rule (`removeUnnamedParams`).
pub fn is_named(name: &KeyName) -> bool {
    matches!(name, KeyName::Named(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, Value)]) -> JsMap {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    #[test]
    fn non_path_values() {
        let p = params(&[
            ("slug", Value::Str("a b".into())),
            ("path", Value::Arr(vec!["x".into(), "y".into()])),
        ]);
        assert_eq!(compile_non_path("plain", &p).unwrap(), "plain");
        assert_eq!(compile_non_path(":slug", &p).unwrap(), "a b");
        assert_eq!(compile_non_path("v-:slug-(x)", &p).unwrap(), "v-a b-(x)");
        assert_eq!(compile_non_path(":path*", &p).unwrap(), "x/y");
        assert_eq!(compile_non_path(":slugs", &p).unwrap(), ":slugs");
        assert_eq!(compile_non_path(":other", &p).unwrap(), ":other");
    }

    #[test]
    fn destinations_substitute_and_merge_queries() {
        let p = params(&[("slug", Value::Str("x".into()))]);
        let q = params(&[("a", Value::Str("1".into()))]);
        let d = prepare_destination("/blog/:slug?fixed=1", &p, &q, true).unwrap();
        assert_eq!(d.pathname, "/blog/x");
        assert_eq!(d.query.keys().collect::<Vec<_>>(), ["a", "fixed"]);
        let d = prepare_destination("/target", &p, &q, true).unwrap();
        assert_eq!(d.query.get("slug"), Some(&Value::Str("x".into())));
        let d = prepare_destination("https://:slug.example.com/p", &p, &q, false).unwrap();
        assert_eq!(d.hostname.as_deref(), Some("x.example.com"));
    }

    #[test]
    fn has_conditions() {
        let mut headers = JsMap::new();
        headers.insert("x-gate", Value::Str("open".into()));
        headers.insert("cookie", Value::Str("theme=dark; skip=".into()));
        headers.insert("host", Value::Str("127.0.0.1:3000".into()));
        let query = JsMap::new();
        let view = RequestView {
            headers: &headers,
            query: &query,
        };
        let gate = HasCondition::new(HasKind::Header, Some("X-Gate"), Some("open")).unwrap();
        let theme = HasCondition::new(HasKind::Cookie, Some("theme"), Some("(?<theme>dark|light)"))
            .unwrap();
        let host = HasCondition::new(HasKind::Host, None, Some("127.0.0.1")).unwrap();
        let skip = HasCondition::new(HasKind::Cookie, Some("skip"), None).unwrap();
        let got = match_has(&view, &[gate.clone(), theme, host], &[]).unwrap();
        assert_eq!(got.get("theme"), Some(&Value::Str("dark".into())));
        assert_eq!(got.get("host"), Some(&Value::Str("127.0.0.1".into())));
        // An empty cookie value counts as absent.
        assert!(match_has(&view, &[gate], &[skip]).is_some());
    }
}
