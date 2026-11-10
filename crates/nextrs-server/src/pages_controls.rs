//! Deliberate Pages Router defects for the suites' negative controls
//! (`NEXTRS_PAGES_NEGATIVE_CONTROL`, comma-separated). Compiled only with
//! the `negative-controls` feature, which release builds never enable: the
//! suites run a separately built binary to prove they catch each defect.

use std::collections::HashMap;
use std::sync::Mutex;

use axum::body::Bytes;

pub(crate) fn active(name: &str) -> bool {
    static CONTROL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CONTROL
        .get_or_init(|| std::env::var("NEXTRS_PAGES_NEGATIVE_CONTROL").unwrap_or_default())
        .split(',')
        .any(|c| c == name)
}

/// `wrong-page`: a page route renders another page's module.
pub(crate) fn wrong_page(state: &crate::AppState, page: &str) -> Option<std::path::PathBuf> {
    if !active("wrong-page") {
        return None;
    }
    state
        .routes
        .iter()
        .filter(|r| r.kind == nextrs_adapter::RouteKind::PagesPage && r.page != page)
        .filter_map(|r| r.entry.clone())
        .find(|e| e.extension().is_some_and(|x| x == "js"))
}

/// `skip-ssr`: a getServerSideProps page answers without its handler.
pub(crate) fn skip_ssr() -> bool {
    active("skip-ssr")
}

/// `skip-api`: an API route answers `{}` without its handler.
pub(crate) fn skip_api() -> bool {
    active("skip-api")
}

/// `wrong-params`: a dynamic route's last path segment is replaced before
/// the handler parses it.
pub(crate) fn params(uri: &axum::http::Uri, dynamic: bool) -> Option<axum::http::Uri> {
    if !active("wrong-params") || !dynamic {
        return None;
    }
    let path = uri.path();
    let (head, _) = path.rsplit_once('/')?;
    let rest = if path.ends_with(".json") {
        "wrong.json"
    } else {
        "wrong"
    };
    let pq = match uri.query() {
        Some(q) => format!("{head}/{rest}?{q}"),
        None => format!("{head}/{rest}"),
    };
    pq.parse().ok()
}

/// A stored response: status, headers, body.
type SharedResponse = (u16, Vec<(String, String)>, Bytes);
type SharedResponses = Mutex<HashMap<String, SharedResponse>>;

/// `cross-user`: getServerSideProps output is shared between requests of
/// the same path, whoever sent them (a cache that ignores the cookie).
pub(crate) fn shared_ssr() -> Option<&'static SharedResponses> {
    static SHARED: std::sync::OnceLock<SharedResponses> = std::sync::OnceLock::new();
    active("cross-user").then(|| SHARED.get_or_init(Default::default))
}

/// `replay-post`: a POST to an API route runs its handler twice.
pub(crate) fn replay_post() -> bool {
    active("replay-post")
}

/// `skip-proxy-data`: the proxy does not run for /_next/data requests.
pub(crate) fn skip_proxy_data(pathname: &str) -> bool {
    active("skip-proxy-data") && pathname.contains("/_next/data/")
}

/// `wrong-data-route`: every dynamic data request maps to the first
/// dynamic data route's page.
pub(crate) fn wrong_data_route() -> bool {
    active("wrong-data-route")
}

/// `wrong-locale`: every request routes under the default locale (the
/// locale meta Next's handler gets ignores the URL's locale).
pub(crate) fn wrong_locale() -> bool {
    active("wrong-locale")
}

/// `locale-cache-leak`: pages without data functions are looked up without
/// their locale (another locale's stored HTML answers).
pub(crate) fn locale_cache_leak() -> bool {
    active("locale-cache-leak")
}

/// `skip-preview`: Preview Mode cookies are dropped before the page's
/// handler sees the request.
pub(crate) fn skip_preview() -> bool {
    active("skip-preview")
}

/// `skip-proxy-locale`: the proxy does not run for paths with a locale
/// prefix.
pub(crate) fn skip_proxy_locale(state: &crate::AppState, pathname: &str) -> bool {
    active("skip-proxy-locale")
        && state
            .i18n
            .as_ref()
            .is_some_and(|i18n| i18n.split(pathname).1.is_some())
}
