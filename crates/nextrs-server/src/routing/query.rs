//! JavaScript data shapes the Next.js router works with: plain objects used
//! as string maps (`parsedUrl.query`, route params, Node request headers),
//! and the encoders Next applies to them. Key order follows JavaScript's
//! own rule, because Next serializes these objects into URLs.

/// A value in a query, params or headers object: a string or an array of
/// strings (repeated query keys, `:path*` params, `set-cookie`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Arr(Vec<String>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            Self::Arr(_) => None,
        }
    }

    /// JavaScript truthiness: an empty string is falsy, an array never is.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Str(s) => !s.is_empty(),
            Self::Arr(_) => true,
        }
    }

    pub fn values(&self) -> impl Iterator<Item = &str> {
        let (one, many): (Option<&str>, &[String]) = match self {
            Self::Str(s) => (Some(s.as_str()), &[]),
            Self::Arr(items) => (None, items.as_slice()),
        };
        one.into_iter().chain(many.iter().map(String::as_str))
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

/// A JavaScript object used as a map. Iteration follows
/// `OrdinaryOwnPropertyKeys`: array-index keys first in ascending numeric
/// order, then the other keys in insertion order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JsMap {
    entries: Vec<(String, Value)>,
}

/// A canonical array index (`"0"`, `"17"`, never `"01"`), below 2^32 - 1.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse::<u32>().ok().filter(|&n| n != u32::MAX)
}

impl JsMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// `obj[key] = value`: replaces in place, or adds the key.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some((_, slot)) => *slot = value,
            None => self.entries.push((key, value)),
        }
    }

    /// `delete obj[key]`.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let at = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(at).1)
    }

    /// `Object.assign(this, other)`.
    pub fn assign(&mut self, other: &JsMap) {
        for (key, value) in other.iter() {
            self.insert(key, value.clone());
        }
    }

    /// Entries in JavaScript's property order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        let mut indexed: Vec<(u32, &(String, Value))> = self
            .entries
            .iter()
            .filter_map(|entry| array_index(&entry.0).map(|n| (n, entry)))
            .collect();
        indexed.sort_by_key(|(n, _)| *n);
        let named = self
            .entries
            .iter()
            .filter(|entry| array_index(&entry.0).is_none());
        indexed
            .into_iter()
            .map(|(_, entry)| entry)
            .chain(named)
            .map(|(k, v)| (k.as_str(), v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.iter().map(|(k, _)| k)
    }
}

impl FromIterator<(String, Value)> for JsMap {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
        let mut map = Self::new();
        for (key, value) in iter {
            map.insert(key, value);
        }
        map
    }
}

/// Next's `searchParamsToUrlQuery`: repeated keys become arrays.
pub fn search_params_to_query(pairs: impl IntoIterator<Item = (String, String)>) -> JsMap {
    let mut query = JsMap::new();
    for (key, value) in pairs {
        let merged = match query.remove_keep_slot(&key) {
            None => Value::Str(value),
            Some(Value::Str(first)) => Value::Arr(vec![first, value]),
            Some(Value::Arr(mut items)) => {
                items.push(value);
                Value::Arr(items)
            }
        };
        query.insert(key, merged);
    }
    query
}

impl JsMap {
    /// Takes a value out while keeping its key's insertion position (the
    /// caller puts a value back with `insert`).
    fn remove_keep_slot(&mut self, key: &str) -> Option<Value> {
        let slot = self.entries.iter_mut().find(|(k, _)| k == key)?;
        Some(std::mem::replace(&mut slot.1, Value::Arr(Vec::new())))
    }
}

/// Parses a query string (without `?`) as `URLSearchParams` does.
pub fn parse_search(search: &str) -> JsMap {
    let search = search.strip_prefix('?').unwrap_or(search);
    search_params_to_query(
        url::form_urlencoded::parse(search.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned())),
    )
}

/// `urlQueryToSearchParams(query).toString()`: the
/// `application/x-www-form-urlencoded` serialization.
pub fn url_query_to_search_params(query: &JsMap) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in query.iter() {
        for item in value.values() {
            serializer.append_pair(key, item);
        }
    }
    serializer.finish()
}

/// Node's `querystring.stringify(obj, '&', '=', { encodeURIComponent })`.
pub fn querystring_stringify(query: &JsMap, encode: impl Fn(&str) -> String) -> String {
    let mut fields = String::new();
    for (key, value) in query.iter() {
        let ks = format!("{}=", encode(key));
        match value {
            Value::Arr(items) => {
                if items.is_empty() {
                    continue;
                }
                if !fields.is_empty() {
                    fields.push('&');
                }
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        fields.push('&');
                    }
                    fields.push_str(&ks);
                    fields.push_str(&encode(item));
                }
            }
            Value::Str(item) => {
                if !fields.is_empty() {
                    fields.push('&');
                }
                fields.push_str(&ks);
                fields.push_str(&encode(item));
            }
        }
    }
    fields
}

/// JavaScript's `encodeURIComponent`.
pub fn encode_uri_component(value: &str) -> String {
    const KEEP: &[u8] = b"-_.!~*'()";
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || KEEP.contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// JavaScript's `decodeURIComponent`: `None` where it would throw (a
/// malformed escape or bytes that are not UTF-8).
pub fn decode_uri_component(value: &str) -> Option<String> {
    if !value.contains('%') {
        return Some(value.to_owned());
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let text = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(text, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Next's `stringifyQuery(req, query)` (server-route-utils): keys and
/// values that appeared in the request's own query are percent-encoded,
/// everything else (values captured by the rule) is written raw.
pub fn stringify_redirect_query(query: &JsMap, initial: &JsMap) -> String {
    querystring_stringify(query, |value| {
        let from_request = initial.contains_key(value)
            || initial.iter().any(|(_, v)| match v {
                Value::Str(s) => s == value,
                Value::Arr(items) => items.iter().any(|s| s == value),
            });
        if from_request {
            encode_uri_component(value)
        } else {
            value.to_owned()
        }
    })
}

/// Next's `normalizeRepeatedSlashes`.
pub fn normalize_repeated_slashes(url: &str) -> String {
    let mut parts = url.split('?');
    let path = parts.next().unwrap_or_default().replace('\\', "/");
    let mut collapsed = String::with_capacity(path.len());
    for c in path.chars() {
        if c == '/' && collapsed.ends_with('/') {
            continue;
        }
        collapsed.push(c);
    }
    let rest: Vec<&str> = parts.collect();
    match rest.first() {
        Some(first) if !first.is_empty() => format!("{collapsed}?{}", rest.join("?")),
        _ => collapsed,
    }
}

/// The `cookie` package's `parse`, as Next's `getCookieParser` uses it.
pub fn parse_cookies(header: &str) -> JsMap {
    let mut cookies = JsMap::new();
    for pair in split_cookie_pairs(header) {
        let Some(eq) = pair.find('=') else { continue };
        let key = pair[..eq].trim();
        let mut value = pair[eq + 1..].trim();
        if value.starts_with('"') {
            // JavaScript's slice(1, -1) on a one-character string is "".
            value = if value.len() >= 2 {
                &value[1..value.len() - 1]
            } else {
                ""
            };
        }
        if !cookies.contains_key(key) {
            let decoded = decode_uri_component(value).unwrap_or_else(|| value.to_owned());
            cookies.insert(key, Value::Str(decoded));
        }
    }
    cookies
}

/// `str.split(/; */)`.
fn split_cookie_pairs(header: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = header;
    while let Some(at) = rest.find(';') {
        parts.push(&rest[..at]);
        rest = rest[at + 1..].trim_start_matches(' ');
    }
    parts.push(rest);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_property_order_puts_indexes_first() {
        let map: JsMap = [("b", "1"), ("2", "x"), ("a", "2"), ("1", "y"), ("01", "z")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), Value::from(v)))
            .collect();
        assert_eq!(map.keys().collect::<Vec<_>>(), ["1", "2", "b", "a", "01"]);
    }

    #[test]
    fn repeated_query_keys_become_arrays_in_first_position() {
        let q = parse_search("?a=1&b=2&a=3&c=%20+");
        assert_eq!(q.get("a"), Some(&Value::Arr(vec!["1".into(), "3".into()])));
        assert_eq!(q.keys().collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(q.get("c"), Some(&Value::Str("  ".into())));
    }

    #[test]
    fn encode_and_decode_uri_component() {
        assert_eq!(encode_uri_component("a b/ä?*"), "a%20b%2F%C3%A4%3F*");
        assert_eq!(decode_uri_component("%E0%A4%A"), None);
        assert_eq!(decode_uri_component("%ff"), None);
        assert_eq!(decode_uri_component("a%2Fb").as_deref(), Some("a/b"));
    }

    #[test]
    fn repeated_slashes() {
        assert_eq!(
            normalize_repeated_slashes("//a\\\\b//c?x=//y"),
            "/a/b/c?x=//y"
        );
        assert_eq!(normalize_repeated_slashes("/a?"), "/a");
        assert_eq!(normalize_repeated_slashes("/a?b?c"), "/a?b?c");
    }

    #[test]
    fn cookies_parse_like_the_cookie_package() {
        let c = parse_cookies("a=1; b=\"two\";c=%E2%9C%93; a=ignored; bad; d=%zz");
        assert_eq!(c.get("a"), Some(&Value::Str("1".into())));
        assert_eq!(c.get("b"), Some(&Value::Str("two".into())));
        assert_eq!(c.get("c"), Some(&Value::Str("✓".into())));
        assert_eq!(c.get("d"), Some(&Value::Str("%zz".into())));
        assert!(!c.contains_key("bad"));
    }
}
