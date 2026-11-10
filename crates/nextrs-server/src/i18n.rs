//! Next's native `i18n` routing for the Pages Router, ported from the
//! pieces of next start that apply it: router-server's locale redirect for
//! `/` (`getLocaleRedirect`), resolve-routes' locale prefixing and the
//! filesystem router's locale stripping (`normalizeLocalePath`,
//! `detectDomainLocale`, `getHostname`). Next's compiled page handlers
//! read the result as request meta (`locale`, `defaultLocale`).
//!
//! The host a domain locale is chosen by is the `Host` header, as next
//! start reads it (never `X-Forwarded-Host`).

use axum::http::HeaderMap;
use nextrs_adapter::{DomainLocale, I18nConfig, normalize_locale_path};

use crate::routing::query::decode_uri_component;

/// The i18n config with what routing derives from it once.
#[derive(Debug)]
pub(crate) struct I18n {
    pub config: I18nConfig,
    /// The config's default locale and every domain's: the locales static
    /// files may be requested under (fsChecker's legacy behaviour).
    file_locales: Vec<String>,
}

/// The locale state of one request (resolve-routes' request meta).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocaleState {
    /// `defaultLocale`: the domain's default locale, else the config's.
    pub default_locale: String,
    /// `locale`: the locale the request routes under.
    pub locale: String,
    /// The locale the URL itself named (`initialLocaleResult.detectedLocale`).
    pub url_locale: Option<String>,
    /// A domain locale matched the request's host.
    pub locale_domain: bool,
}

impl I18n {
    pub(crate) fn new(config: I18nConfig) -> Self {
        let mut file_locales = vec![config.default_locale.clone()];
        file_locales.extend(config.domains.iter().map(|d| d.default_locale.clone()));
        Self {
            config,
            file_locales,
        }
    }

    /// `normalizeLocalePath(pathname, i18n.locales)`.
    pub(crate) fn split<'p>(&self, pathname: &'p str) -> (&'p str, Option<&str>) {
        normalize_locale_path(pathname, &self.config.locales)
    }

    /// fsChecker's `handleLocale` for files outside `pages/`: only the
    /// default locales are stripped.
    pub(crate) fn split_file<'p>(&self, pathname: &'p str) -> (&'p str, Option<&str>) {
        normalize_locale_path(pathname, &self.file_locales)
    }

    /// `detectDomainLocale(i18n.domains, hostname, detectedLocale)`.
    pub(crate) fn domain(
        &self,
        hostname: Option<&str>,
        detected: Option<&str>,
    ) -> Option<&DomainLocale> {
        let detected = detected.map(str::to_lowercase);
        self.config.domains.iter().find(|item| {
            let domain = item
                .domain
                .split(':')
                .next()
                .unwrap_or_default()
                .to_lowercase();
            hostname == Some(domain.as_str())
                || detected.as_deref() == Some(item.default_locale.to_lowercase().as_str())
                || item
                    .locales
                    .iter()
                    .any(|l| Some(l.to_lowercase().as_str()) == detected.as_deref())
        })
    }

    /// resolve-routes' locale meta for a pathname (without basePath).
    pub(crate) fn state(&self, headers: &HeaderMap, pathname: &str) -> LocaleState {
        let host = hostname(headers);
        let domain = self.domain(host.as_deref(), None);
        let default_locale = domain
            .map(|d| d.default_locale.clone())
            .unwrap_or_else(|| self.config.default_locale.clone());
        let url_locale = self.split(pathname).1.map(str::to_owned);
        LocaleState {
            locale: url_locale.clone().unwrap_or_else(|| default_locale.clone()),
            default_locale,
            url_locale,
            locale_domain: domain.is_some(),
        }
    }

    /// router-server's locale redirect: a request for `/` (or `/index`)
    /// without a locale goes to the locale the domain, the `NEXT_LOCALE`
    /// cookie or `Accept-Language` prefer, when that is not the default.
    /// `path` is the request path without its query; `search` is the
    /// parsed query (`?a=1`, or empty).
    pub(crate) fn root_redirect(
        &self,
        headers: &HeaderMap,
        path: &str,
        search: &str,
        base_path: &str,
        trailing_slash: bool,
    ) -> Option<String> {
        if !self.config.locale_detection {
            return None;
        }
        let url_no_query = remove_path_prefix(path, base_path);
        // getNextPathnameInfo (a locale in a data path counts too).
        let path_locale = self.path_locale(url_no_query);
        let domain_locale = self.domain(hostname(headers).as_deref(), None);
        let default_locale = domain_locale
            .map(|d| d.default_locale.as_str())
            .unwrap_or(self.config.default_locale.as_str());
        // getLocaleRedirect's pathname: with a path locale it is never `/`.
        if path_locale.is_some() || denormalize_page_path(url_no_query) != "/" {
            return None;
        }
        let preferred = accept_language(headers, &self.config.locales);
        let detected = domain_locale
            .map(|d| d.default_locale.clone())
            .or_else(|| self.cookie_locale(headers))
            .or_else(|| preferred.clone())
            .unwrap_or_else(|| self.config.default_locale.clone());
        if let Some(domain_locale) = domain_locale
            && let Some(preferred_domain) = self.domain(None, preferred.as_deref())
        {
            let same_domain = preferred_domain.domain == domain_locale.domain;
            let is_default = preferred.as_deref() == Some(preferred_domain.default_locale.as_str());
            if !same_domain || !is_default {
                let scheme = if preferred_domain.http {
                    "http"
                } else {
                    "https"
                };
                let locale = if is_default {
                    ""
                } else {
                    preferred.as_deref().unwrap_or_default()
                };
                return Some(format!("{scheme}://{}/{locale}", preferred_domain.domain));
            }
        }
        if detected.to_lowercase() != default_locale.to_lowercase() {
            return Some(format!(
                "{base_path}/{detected}{}{search}",
                if trailing_slash { "/" } else { "" }
            ));
        }
        None
    }

    /// `getNextPathnameInfo(pathname).locale`: the path's first segment,
    /// or for a `/_next/data/<build>/...json` path the segment after the
    /// build id.
    pub(crate) fn path_locale(&self, pathname: &str) -> Option<String> {
        if let Some(locale) = self.split(pathname).1 {
            return Some(locale.to_owned());
        }
        let data = pathname
            .strip_prefix("/_next/data/")
            .and_then(|rest| rest.strip_suffix(".json"))?;
        let mut parts = data.split('/');
        parts.next();
        let rest: Vec<&str> = parts.collect();
        let page = if rest.first() == Some(&"index") {
            "/".to_owned()
        } else {
            format!("/{}", rest.join("/"))
        };
        self.split(&page).1.map(str::to_owned)
    }

    /// The `NEXT_LOCALE` cookie, when it names a locale (without case).
    fn cookie_locale(&self, headers: &HeaderMap) -> Option<String> {
        let value = cookie(headers, "NEXT_LOCALE")?.to_lowercase();
        self.config
            .locales
            .iter()
            .find(|l| l.to_lowercase() == value)
            .cloned()
    }
}

/// `getHostname(parsed, headers)` for a relative request URL: the `Host`
/// header without its port, lowercased.
pub(crate) fn hostname(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(axum::http::header::HOST)?;
    let host = crate::resolve::latin1(host.as_bytes());
    Some(host.split(':').next().unwrap_or_default().to_lowercase())
}

/// `removePathPrefix(path, prefix)`.
pub(crate) fn remove_path_prefix<'p>(path: &'p str, prefix: &str) -> &'p str {
    if prefix.is_empty() {
        return path;
    }
    match crate::resolve::strip_base_path(path, prefix) {
        Some("") => "/",
        Some(rest) => rest,
        None => path,
    }
}

/// `denormalizePagePath`.
fn denormalize_page_path(page: &str) -> &str {
    if page == "/index" {
        "/"
    } else if page.starts_with("/index/") && !page.contains('[') {
        &page["/index".len()..]
    } else {
        page
    }
}

/// The `cookie` package's parse, as Next's `getCookieParser` uses it: the
/// first pair with the name, its value unquoted and URI-decoded.
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let header = node_header(headers, "cookie", "; ")?;
    for pair in header.split(';') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key.trim() != name {
            continue;
        }
        let mut value = value.trim();
        if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
            value = &value[1..value.len() - 1];
        }
        return Some(decode_uri_component(value).unwrap_or_else(|| value.to_owned()));
    }
    None
}

/// A request header as Node joins repeated ones.
fn node_header(headers: &HeaderMap, name: &str, separator: &str) -> Option<String> {
    let values: Vec<String> = headers
        .get_all(name)
        .iter()
        .map(|v| crate::resolve::latin1(v.as_bytes()))
        .collect();
    (!values.is_empty()).then(|| values.join(separator))
}

/// Next's `acceptLanguage(header, locales)` (server/accept-header.js): the
/// most preferred configured locale, matching a configured `fr-CA` for a
/// requested `fr` but not the other way round. A malformed header (which
/// Next's parser throws on) prefers nothing.
pub(crate) fn accept_language(headers: &HeaderMap, locales: &[String]) -> Option<String> {
    let header = node_header(headers, "accept-language", ", ")?;
    parse_accept_language(&header, locales).ok().flatten()
}

fn parse_accept_language(raw: &str, preferences: &[String]) -> Result<Option<String>, ()> {
    // lowercased preference -> (original, position), with prefixes.
    let mut lowers: Vec<(String, String, usize)> = Vec::new();
    let mut pos = 0;
    for preference in preferences {
        let lower = preference.to_lowercase();
        if !lowers.iter().any(|(l, ..)| *l == lower) {
            lowers.push((lower.clone(), preference.clone(), pos));
        } else if let Some(entry) = lowers.iter_mut().find(|(l, ..)| *l == lower) {
            // Map#set overwrites the entry.
            *entry = (lower.clone(), preference.clone(), pos);
        }
        pos += 1;
        let mut parts: Vec<&str> = lower.split('-').collect();
        loop {
            parts.pop();
            if parts.is_empty() {
                break;
            }
            let joined = parts.join("-");
            if !lowers.iter().any(|(l, ..)| *l == joined) {
                lowers.push((joined, preference.clone(), pos));
                pos += 1;
            }
        }
    }
    let lookup = |token: &str| lowers.iter().find(|(l, ..)| l == token);

    let header: String = raw.chars().filter(|c| *c != ' ' && *c != '\t').collect();
    struct Selection {
        token: String,
        pos: usize,
        q: f64,
        pref: Option<usize>,
    }
    let mut selections = Vec::new();
    let mut seen = Vec::new();
    for (i, part) in header.split(',').enumerate() {
        if part.is_empty() {
            continue;
        }
        let params: Vec<&str> = part.split(';').collect();
        if params.len() > 2 {
            return Err(());
        }
        let token = params[0].to_lowercase();
        if token.is_empty() {
            return Err(());
        }
        let mut selection = Selection {
            pref: lookup(&token).map(|(.., pos)| *pos),
            token,
            pos: i,
            q: 1.0,
        };
        seen.push(selection.token.clone());
        if params.len() == 2 {
            let mut kv = params[1].split('=');
            let key = kv.next().unwrap_or_default();
            let value = kv.next().unwrap_or_default();
            if value.is_empty() || (key != "q" && key != "Q") {
                return Err(());
            }
            let score = js_parse_float(value);
            if score == Some(0.0) {
                continue;
            }
            if let Some(score) = score
                && score.is_finite()
                && (0.001..=1.0).contains(&score)
            {
                selection.q = score;
            }
        }
        selections.push(selection);
    }
    selections.sort_by(|a, b| {
        use std::cmp::Ordering;
        if b.q != a.q {
            return b.q.partial_cmp(&a.q).unwrap_or(Ordering::Equal);
        }
        if b.pref != a.pref {
            return match (a.pref, b.pref) {
                (None, _) => Ordering::Greater,
                (_, None) => Ordering::Less,
                (Some(a), Some(b)) => a.cmp(&b),
            };
        }
        a.pos.cmp(&b.pos)
    });
    for selection in &selections {
        if selection.token == "*" {
            // The first configured preference the header did not name.
            if let Some((_, orig, _)) = lowers.iter().find(|(l, ..)| !seen.contains(l)) {
                return Ok(Some(orig.clone()));
            }
        } else if let Some((_, orig, _)) = lookup(&selection.token) {
            return Ok(Some(orig.clone()));
        }
    }
    Ok(None)
}

/// JavaScript's `parseFloat`: the longest numeric prefix, `None` for NaN.
fn js_parse_float(value: &str) -> Option<f64> {
    let value = value.trim_start();
    let mut end = 0;
    let bytes = value.as_bytes();
    let mut seen_dot = false;
    let mut seen_digit = false;
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    while end < bytes.len() {
        match bytes[end] {
            b'0'..=b'9' => seen_digit = true,
            b'.' if !seen_dot => seen_dot = true,
            _ => break,
        }
        end += 1;
    }
    if !seen_digit {
        return value.starts_with("Infinity").then_some(f64::INFINITY);
    }
    // An exponent, when one follows.
    let mut exp_end = end;
    if exp_end < bytes.len() && (bytes[exp_end] == b'e' || bytes[exp_end] == b'E') {
        let mut j = exp_end + 1;
        if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
            j += 1;
        }
        let digits = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > digits {
            exp_end = j;
        }
    }
    value[..exp_end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locales() -> Vec<String> {
        vec!["en".into(), "fr".into(), "ja".into()]
    }

    fn al(header: &str, prefs: &[&str]) -> Option<String> {
        let prefs: Vec<String> = prefs.iter().map(|s| (*s).to_owned()).collect();
        parse_accept_language(header, &prefs).ok().flatten()
    }

    #[test]
    fn accept_language_matches_next() {
        // Observed against next start 16.4.3 (fixtures/next-pages-i18n).
        assert_eq!(
            al("fr-FR,fr;q=0.9", &["en", "fr", "ja"]).as_deref(),
            Some("fr")
        );
        assert_eq!(al("fr", &["en", "fr", "ja"]).as_deref(), Some("fr"));
        assert_eq!(al("fr-CA", &["en", "fr", "ja"]), None);
        assert_eq!(
            al("de, ja;q=0.5", &["en", "fr", "ja"]).as_deref(),
            Some("ja")
        );
        assert_eq!(
            al("ja;q=0, fr;q=0.1", &["en", "fr", "ja"]).as_deref(),
            Some("fr")
        );
        // A configured region matches its language.
        assert_eq!(al("fr", &["en", "fr-CA"]).as_deref(), Some("fr-CA"));
        // Equal q: configured order decides, then header order.
        assert_eq!(al("ja, fr", &["en", "fr", "ja"]).as_deref(), Some("fr"));
        assert_eq!(al("*", &["en", "fr"]).as_deref(), Some("en"));
        assert_eq!(
            al("en;q=0.2, *;q=0.5", &["en", "fr"]).as_deref(),
            Some("fr")
        );
        // Out-of-range q counts as 1; malformed headers prefer nothing.
        assert_eq!(al("en;q=0.5, fr;q=7", &["en", "fr"]).as_deref(), Some("fr"));
        assert_eq!(al("fr;x=1", &["en", "fr"]), None);
        assert_eq!(al("fr;q=1;y", &["en", "fr"]), None);
        assert_eq!(al("", &["en", "fr"]), None);
    }

    #[test]
    fn locale_paths() {
        let l = locales();
        assert_eq!(
            normalize_locale_path("/fr/about", &l),
            ("/about", Some("fr"))
        );
        assert_eq!(normalize_locale_path("/FR", &l), ("/", Some("fr")));
        assert_eq!(normalize_locale_path("/%66r", &l), ("/%66r", None));
        assert_eq!(normalize_locale_path("/de/x", &l), ("/de/x", None));
        assert_eq!(normalize_locale_path("/", &l), ("/", None));
        assert_eq!(normalize_locale_path("/fr/", &l), ("/", Some("fr")));
    }

    fn i18n(domains: bool) -> I18n {
        let config = I18nConfig {
            locales: locales(),
            default_locale: "en".into(),
            locale_detection: true,
            domains: if domains {
                vec![
                    DomainLocale {
                        domain: "example.com".into(),
                        default_locale: "en".into(),
                        locales: vec![],
                        http: false,
                    },
                    DomainLocale {
                        domain: "example.fr:8080".into(),
                        default_locale: "fr".into(),
                        locales: vec![],
                        http: false,
                    },
                    DomainLocale {
                        domain: "example.jp".into(),
                        default_locale: "ja".into(),
                        locales: vec![],
                        http: true,
                    },
                ]
            } else {
                vec![]
            },
        };
        I18n::new(config)
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.append(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        map
    }

    #[test]
    fn root_redirects() {
        let i = i18n(false);
        let r = |h: &[(&str, &str)], p: &str| i.root_redirect(&headers(h), p, "", "", false);
        assert_eq!(r(&[("accept-language", "fr")], "/").as_deref(), Some("/fr"));
        assert_eq!(
            r(&[("accept-language", "fr")], "/index").as_deref(),
            Some("/fr")
        );
        assert_eq!(r(&[("accept-language", "fr")], "/about"), None);
        assert_eq!(r(&[("accept-language", "ja")], "/fr"), None);
        assert_eq!(
            r(&[("cookie", "NEXT_LOCALE=JA")], "/").as_deref(),
            Some("/ja")
        );
        assert_eq!(
            r(
                &[("cookie", "NEXT_LOCALE=de"), ("accept-language", "fr")],
                "/"
            )
            .as_deref(),
            Some("/fr")
        );
        assert_eq!(
            r(
                &[("cookie", "NEXT_LOCALE=en"), ("accept-language", "fr")],
                "/"
            ),
            None
        );
        assert_eq!(
            i.root_redirect(
                &headers(&[("accept-language", "fr")]),
                "/",
                "?a=1",
                "/base",
                true
            )
            .as_deref(),
            Some("/base/fr/?a=1")
        );

        let d = i18n(true);
        let r = |h: &[(&str, &str)]| d.root_redirect(&headers(h), "/", "?q=1", "", false);
        // A domain's own default locale: no redirect.
        assert_eq!(
            r(&[("host", "example.fr"), ("accept-language", "fr")]),
            None
        );
        // Another domain's default locale: that domain, http when configured.
        assert_eq!(
            r(&[("host", "example.com"), ("accept-language", "ja")]).as_deref(),
            Some("http://example.jp/")
        );
        assert_eq!(
            r(&[("host", "EXAMPLE.com:3000"), ("accept-language", "fr")]).as_deref(),
            Some("https://example.fr:8080/")
        );
        // The cookie does not override a domain's default locale.
        assert_eq!(
            r(&[("host", "example.fr"), ("cookie", "NEXT_LOCALE=ja")]),
            None
        );
    }
}
