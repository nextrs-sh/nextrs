//! Next's `parseUrl` / `parseRelativeUrl` (WHATWG `URL`, through the `url`
//! crate, which implements the same standard), `getRelativeURL`, and the
//! legacy Node `url.format` the router uses to write `Location`.

use url::Url;

use super::query::{JsMap, parse_search};

/// The object `parseUrl` returns. Relative URLs have no protocol, host or
/// origin, and `slashes` is `null`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedUrl {
    /// With the colon, e.g. `https:`.
    pub protocol: Option<String>,
    pub auth: Option<String>,
    pub hostname: Option<String>,
    pub port: Option<String>,
    pub pathname: String,
    /// `?…`, or empty.
    pub search: String,
    /// `#…`, or empty.
    pub hash: String,
    pub query: JsMap,
    pub origin: Option<String>,
    pub href: String,
    pub slashes: Option<bool>,
}

const RELATIVE_BASE: &str = "http://n";

/// `new URL(x).search` / `.hash`: empty unless there is something after
/// the delimiter.
fn search_of(url: &Url) -> String {
    match url.query() {
        Some(q) if !q.is_empty() => format!("?{q}"),
        _ => String::new(),
    }
}

fn hash_of(url: &Url) -> String {
    match url.fragment() {
        Some(f) if !f.is_empty() => format!("#{f}"),
        _ => String::new(),
    }
}

/// Next's `parseUrl`.
pub fn parse_url(input: &str) -> Result<ParsedUrl, String> {
    if input.starts_with('/') {
        return parse_relative_url(input);
    }
    let url = Url::parse(input).map_err(|error| format!("Invalid URL {input:?}: {error}"))?;
    let protocol = format!("{}:", url.scheme());
    let auth = (!url.username().is_empty()).then(|| match url.password() {
        Some(password) => format!("{}:{password}", url.username()),
        None => url.username().to_owned(),
    });
    let href = url.as_str().to_owned();
    let slashes = href.get(protocol.len()..protocol.len() + 2) == Some("//");
    Ok(ParsedUrl {
        auth,
        hostname: Some(url.host_str().unwrap_or_default().to_owned()),
        port: Some(url.port().map(|p| p.to_string()).unwrap_or_default()),
        pathname: url.path().to_owned(),
        search: search_of(&url),
        hash: hash_of(&url),
        query: parse_search(url.query().unwrap_or_default()),
        origin: Some(url.origin().ascii_serialization()),
        href,
        protocol: Some(protocol),
        slashes: Some(slashes),
    })
}

/// Next's `parseRelativeUrl(url)` for a path-absolute URL.
pub fn parse_relative_url(input: &str) -> Result<ParsedUrl, String> {
    if let Some(query_start) = unchanged_by_parser(input) {
        let (pathname, query) = match query_start {
            Some(q) => (&input[..q], &input[q + 1..]),
            None => (input, ""),
        };
        return Ok(ParsedUrl {
            pathname: pathname.to_owned(),
            search: if query.is_empty() {
                String::new()
            } else {
                format!("?{query}")
            },
            query: parse_search(query),
            href: input.to_owned(),
            ..ParsedUrl::default()
        });
    }
    parse_relative_url_with_parser(input)
}

/// Whether the WHATWG URL parser would return `input` unchanged as the
/// path and query of `http://n` + `input` (and where the query starts):
/// a path-absolute URL of bytes the parser neither percent-encodes, strips
/// nor reinterprets in a special URL's path or query, without dot segments
/// (`.` or `%2e` forms) and without a fragment. Anything else takes the
/// parser. Requests are almost always such URLs, and every request is
/// parsed (Phase 3A: ~10 % of a prerendered page's CPU went to the general
/// parse). `parses_like_the_url_parser` checks it against the parser.
fn unchanged_by_parser(input: &str) -> Option<Option<usize>> {
    let bytes = input.as_bytes();
    // "//x" would be an authority.
    if bytes.first() != Some(&b'/') || bytes.get(1) == Some(&b'/') {
        return None;
    }
    let mut query = None;
    for (i, &b) in bytes.iter().enumerate() {
        let plain = b.is_ascii_alphanumeric()
            || matches!(
                b,
                b'-' | b'_'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b':'
                    | b'@'
                    | b'/'
                    | b'%'
                    | b'.'
            );
        if b == b'?' {
            if query.is_none() {
                query = Some(i);
            }
            continue;
        }
        if !plain {
            return None;
        }
        if query.is_none() {
            // A path segment starting with "." may be a dot segment;
            // "%2e" may spell one.
            if b == b'.' && bytes[i - 1] == b'/' {
                return None;
            }
            if b == b'%'
                && bytes.get(i + 1) == Some(&b'2')
                && matches!(bytes.get(i + 2), Some(b'e' | b'E'))
            {
                return None;
            }
        }
    }
    Some(query)
}

/// `parse_relative_url` through the `url` crate (any input).
fn parse_relative_url_with_parser(input: &str) -> Result<ParsedUrl, String> {
    let url = Url::parse(&format!("{RELATIVE_BASE}{input}"))
        .map_err(|error| format!("Invalid URL {input:?}: {error}"))?;
    let origin = url.origin().ascii_serialization();
    if origin != RELATIVE_BASE {
        return Err(format!(
            "invariant: invalid relative URL, router received {input}"
        ));
    }
    Ok(ParsedUrl {
        pathname: url.path().to_owned(),
        search: search_of(&url),
        hash: hash_of(&url),
        query: parse_search(url.query().unwrap_or_default()),
        href: url.as_str()[origin.len()..].to_owned(),
        ..ParsedUrl::default()
    })
}

/// Next's `getRelativeURL(url, base)`: the URL without its origin when it
/// has the base's origin, otherwise the whole URL.
pub fn relative_url(input: &str, base: &str) -> Result<String, String> {
    let base = Url::parse(base).map_err(|error| format!("Invalid URL {base:?}: {error}"))?;
    let resolved = base
        .join(input)
        .map_err(|error| format!("Invalid URL {input:?}: {error}"))?;
    let base_origin = base.origin().ascii_serialization();
    if resolved.origin().ascii_serialization() == base_origin {
        Ok(resolved.as_str()[base_origin.len()..].to_owned())
    } else {
        Ok(resolved.as_str().to_owned())
    }
}

/// Protocols Node's `url.format` always writes with `//`.
const SLASHED_PROTOCOLS: &[&str] = &["http:", "https:", "ftp:", "gopher:", "file:", "ws:", "wss:"];

/// Node's legacy `url.format(urlObject)` for the fields the router sets.
pub fn format_url(parsed: &ParsedUrl) -> String {
    let auth = parsed
        .auth
        .as_deref()
        .filter(|a| !a.is_empty())
        .map(|a| format!("{a}@"))
        .unwrap_or_default();
    let mut protocol = parsed.protocol.clone().unwrap_or_default();
    let mut host = String::new();
    if let Some(hostname) = parsed.hostname.as_deref().filter(|h| !h.is_empty()) {
        let bracketed = hostname.contains(':') && !hostname.starts_with('[');
        host = if bracketed {
            format!("{auth}[{hostname}]")
        } else {
            format!("{auth}{hostname}")
        };
        if let Some(port) = parsed.port.as_deref().filter(|p| !p.is_empty()) {
            host.push(':');
            host.push_str(port);
        }
    }
    if !protocol.is_empty() && !protocol.ends_with(':') {
        protocol.push(':');
    }
    let mut pathname = parsed.pathname.replace('#', "%23").replace('?', "%3F");
    if parsed.slashes == Some(true) || SLASHED_PROTOCOLS.contains(&protocol.as_str()) {
        if parsed.slashes == Some(true) || !host.is_empty() {
            if !pathname.is_empty() && !pathname.starts_with('/') {
                pathname.insert(0, '/');
            }
            host = format!("//{host}");
        } else if protocol == "file:" {
            host = "//".to_owned();
        }
    }
    let mut search = parsed.search.replace('#', "%23");
    if !search.is_empty() && !search.starts_with('?') {
        search.insert(0, '?');
    }
    let mut hash = parsed.hash.clone();
    if !hash.is_empty() && !hash.starts_with('#') {
        hash.insert(0, '#');
    }
    format!("{protocol}{host}{pathname}{search}{hash}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_urls_parse_against_the_internal_base() {
        let p = parse_url("/a b/c?x=1&x=2#h").unwrap();
        assert_eq!(p.pathname, "/a%20b/c");
        assert_eq!(p.search, "?x=1&x=2");
        assert_eq!(p.hash, "#h");
        assert_eq!(p.href, "/a%20b/c?x=1&x=2#h");
        assert!(p.protocol.is_none());
        let p = parse_url("//evil.example/x").unwrap();
        assert_eq!(p.pathname, "//evil.example/x");
    }

    #[test]
    fn absolute_urls_keep_origin_and_slashes() {
        let p = parse_url("https://Example.com:8443/p?q=1").unwrap();
        assert_eq!(p.protocol.as_deref(), Some("https:"));
        assert_eq!(p.hostname.as_deref(), Some("example.com"));
        assert_eq!(p.port.as_deref(), Some("8443"));
        assert_eq!(p.origin.as_deref(), Some("https://example.com:8443"));
        assert_eq!(p.slashes, Some(true));
        assert_eq!(format_url(&p), "https://example.com:8443/p?q=1");
    }

    #[test]
    fn relativize_same_origin_only() {
        let base = "http://127.0.0.1:3000/a";
        assert_eq!(
            relative_url("http://127.0.0.1:3000/t?x=1", base).unwrap(),
            "/t?x=1"
        );
        assert_eq!(relative_url("/t", base).unwrap(), "/t");
        assert_eq!(
            relative_url("http://localhost:3000/t", base).unwrap(),
            "http://localhost:3000/t"
        );
    }

    #[test]
    fn format_escapes_like_node() {
        let p = ParsedUrl {
            pathname: "/a#b?c".into(),
            search: "x=1#".into(),
            ..ParsedUrl::default()
        };
        assert_eq!(format_url(&p), "/a%23b%3Fc?x=1%23");
    }

    /// The fast path must agree with the URL parser on every input it
    /// accepts: explicit tricky cases, then random inputs over an alphabet
    /// weighted towards what the parser rewrites (dots, `%2e`, slashes,
    /// backslashes, `?`, `#`, quotes, spaces, tabs, non-ASCII).
    #[test]
    fn parses_like_the_url_parser() {
        let check = |input: &str| {
            if unchanged_by_parser(input).is_some() {
                assert_eq!(
                    parse_relative_url(input),
                    parse_relative_url_with_parser(input),
                    "{input:?}"
                );
            }
        };
        let mut accepted = 0;
        for input in [
            "/",
            "/a",
            "/a/b/",
            "/a?",
            "/a?b",
            "/a?b=c&d=e",
            "/a??b",
            "/a?b?c/d",
            "/_next/static/chunks/x.js",
            "/books/42?ref=b0000000001",
            "/a.b/c.d",
            "/a/.b",
            "/a/./b",
            "/a/../b",
            "/.",
            "/..",
            "/a/.",
            "/%2e",
            "/a/%2E%2e/b",
            "/a%2fb",
            "/a%zz",
            "/a%",
            "/a?%2e",
            "/a?.",
            "/~user",
            "/a:b@c",
            "/a;b=c",
            "//x",
            "/a\\b",
            "/a#b",
            "/a b",
            "/a\tb",
            "/a'b",
            "/a?b'c",
            "/é",
            "/a?é",
            "/a|b",
            "/a[b]",
            "/a^b",
            "/a`b",
        ] {
            check(input);
            accepted += usize::from(unchanged_by_parser(input).is_some());
        }
        assert!(accepted >= 15, "the fast path should accept ordinary URLs");
        let alphabet: Vec<char> = "/////....%%2eEaz09-_~!$&()*+,;=:@??#\\'\" <>`{}|^[] \té€"
            .chars()
            .collect();
        let mut seed: u64 = 0x5eed;
        let mut next = |n: usize| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as usize) % n
        };
        // Half the inputs use only characters the fast path may accept, so
        // that it is exercised (dots, `%2e` and `?` placement still vary).
        let safe: Vec<char> = "/..%2eEaz09-_~!$&()*+,;=:@?".chars().collect();
        let mut random_accepted = 0;
        for i in 0..300_000 {
            let set = if i % 2 == 0 { &alphabet } else { &safe };
            let len = 1 + next(12);
            let mut input = String::from("/");
            for _ in 0..len {
                input.push(set[next(set.len())]);
            }
            check(&input);
            random_accepted += usize::from(unchanged_by_parser(&input).is_some());
        }
        assert!(
            random_accepted > 30_000,
            "{random_accepted} random inputs took the fast path"
        );
    }
}
