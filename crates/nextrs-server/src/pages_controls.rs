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

/// `cross-user`: getServerSideProps output is shared between requests of
/// the same path, whoever sent them (a cache that ignores the cookie).
pub(crate) fn shared_ssr()
-> Option<&'static Mutex<HashMap<String, (u16, Vec<(String, String)>, Bytes)>>> {
    static SHARED: std::sync::OnceLock<
        Mutex<HashMap<String, (u16, Vec<(String, String)>, Bytes)>>,
    > = std::sync::OnceLock::new();
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
