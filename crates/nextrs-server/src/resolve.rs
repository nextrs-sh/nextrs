//! A port of Next's `resolveRoutes` (`next/dist/server/lib/router-utils/
//! resolve-routes.js`) and the parts of `router-server.js` that act on its
//! result: the order in which `next start` applies repeated-slash
//! normalization, `next.config` headers and redirects, the proxy,
//! beforeFiles rewrites, the filesystem, afterFiles rewrites, dynamic
//! routes and fallback rewrites.
//!
//! The proxy runs in the embedded executor (Next's compiled
//! `server/middleware.js`); this module applies the `NextResponse` it
//! returns exactly as resolve-routes applies it (request header overrides,
//! rewrites, redirects, direct responses, cookies).

use std::path::PathBuf;

use axum::body::{Body, Bytes};
use axum::http::header::{HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode, Uri, Version};
use axum::response::Response;
use nextrs_runtime::Invocation;

use crate::routing::destination::{RequestView, compile_non_path, prepare_destination};
use crate::routing::query::{
    JsMap, Value, decode_uri_component, normalize_repeated_slashes, stringify_redirect_query,
    url_query_to_search_params,
};
use crate::routing::url::{ParsedUrl, format_url, parse_relative_url, parse_url, relative_url};
use crate::routing::{Rule, RuleKind};
use crate::{AppState, dynamic};

/// Redirect statuses resolve-routes treats a proxy `Location` as a
/// redirect for (`allowedStatusCodes`).
const REDIRECT_STATUSES: &[u16] = &[301, 302, 303, 307, 308];
/// Headers resolve-routes never copies from a proxy response
/// (`ipcForbiddenHeaders`).
const IPC_FORBIDDEN: &[&str] = &[
    "accept-encoding",
    "keepalive",
    "keep-alive",
    "content-encoding",
    "transfer-encoding",
    "connection",
    "expect",
];
/// Next's `INTERNAL_HEADERS` (server-ipc/utils.js): router-server deletes
/// them from every incoming request before routing.
pub(crate) const NEXT_INTERNAL_HEADERS: &[&str] = &[
    "x-middleware-rewrite",
    "x-middleware-redirect",
    "x-middleware-set-cookie",
    "x-middleware-skip",
    "x-middleware-override-headers",
    "x-middleware-next",
    "x-now-route-matches",
    "x-matched-path",
    "x-nextjs-data",
    "x-next-resume-state-length",
    "next-resume",
];
/// Header and name `app-router-headers.js` uses for rewritten RSC requests.
const REWRITTEN_PATH: &str = "x-nextjs-rewritten-path";
const REWRITTEN_QUERY: &str = "x-nextjs-rewritten-query";

/// What the filesystem check found (`fsChecker.getItem`).
#[derive(Debug)]
pub(crate) enum Output {
    /// A file under `.next/static`, resolved.
    Static(PathBuf),
    /// `/_next/image`.
    Image,
    /// A file in `public/`.
    Public(PathBuf),
    /// An App Router route; `item_path` is the pathname without basePath
    /// and trailing slash (the key of its prerender, when there is one).
    App {
        route: usize,
        item_path: String,
        /// A Pages Router `/_next/data` request for this route's page.
        data: Option<crate::pages::DataMatch>,
    },
    /// A path that names an app route only once decoded (`/%70rotected`):
    /// next start matches the file but its handler answers not found.
    EncodedApp,
}

pub(crate) enum Outcome {
    /// A redirect router-server writes (`Location`, body, `Refresh` for 308).
    /// Config redirects carry no routing headers (`resHeaders: null`).
    Redirect {
        status: u16,
        location: String,
        with_res_headers: bool,
    },
    /// A complete response: the proxy answered directly, or a refusal.
    Respond(Response),
    /// The proxy threw: next start's 500.
    ProxyFailed,
    /// A rewrite to another origin: forwarded there under the network
    /// policy (`external.rs`).
    External(String),
    Output(Output),
    NotFound,
}

/// What routing decided about the request (everything but the outcome
/// and the body, which [`resolve`] returns beside it).
pub(crate) struct Resolved {
    /// Node's `req.headers` after routing.
    pub headers: JsMap,
    /// The proxy changed the request's headers: the route must see these.
    pub headers_modified: bool,
    /// Response headers routing set (`resHeaders`), lowercase names.
    pub res_headers: JsMap,
    /// `parsedUrl.pathname` / `.query` after routing.
    pub pathname: String,
    pub query: JsMap,
    pub did_rewrite: bool,
    /// The cookies the proxy set (`middlewareCookie`).
    pub middleware_cookies: Vec<String>,
    /// The outcome came from the proxy's response (not a config rule).
    pub from_proxy: bool,
}

/// The request as routing sees it.
pub(crate) struct RouteRequest<'a> {
    pub method: &'a Method,
    pub uri: &'a Uri,
    pub version: Version,
    pub headers: &'a HeaderMap,
    pub body: Body,
    pub client: Option<std::net::SocketAddr>,
}

/// Node's view of a header value: the bytes as Latin-1.
pub(crate) fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

pub(crate) fn latin1_bytes(text: &str) -> Vec<u8> {
    if text.chars().all(|c| (c as u32) < 256) {
        text.chars().map(|c| c as u32 as u8).collect()
    } else {
        text.as_bytes().to_vec()
    }
}

/// Node's `IncomingMessage#headers` (`_addHeaderLine`): lowercase names,
/// `set-cookie` an array, `cookie` joined with `; `, single-valued headers
/// keep the first, the rest joined with `, `.
pub(crate) fn node_headers(headers: &HeaderMap) -> JsMap {
    const DROP_DUPLICATES: &[&str] = &[
        "age",
        "authorization",
        "content-length",
        "content-type",
        "etag",
        "expires",
        "from",
        "host",
        "if-modified-since",
        "if-unmodified-since",
        "last-modified",
        "location",
        "max-forwards",
        "proxy-authorization",
        "referer",
        "retry-after",
        "server",
        "user-agent",
    ];
    let mut out = JsMap::new();
    for (name, value) in headers {
        let name = name.as_str();
        let value = latin1(value.as_bytes());
        match out.get(name).cloned() {
            None if name == "set-cookie" => out.insert(name, Value::Arr(vec![value])),
            None => out.insert(name, Value::Str(value)),
            Some(Value::Arr(mut items)) => {
                items.push(value);
                out.insert(name, Value::Arr(items));
            }
            Some(Value::Str(existing)) => {
                if DROP_DUPLICATES.contains(&name) {
                    continue;
                }
                let separator = if name == "cookie" { "; " } else { ", " };
                out.insert(name, Value::Str(format!("{existing}{separator}{value}")));
            }
        }
    }
    out
}

/// Node headers back to an HTTP header map (array values repeat).
pub(crate) fn header_map(headers: &JsMap) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in headers.iter() {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        for item in value.values() {
            if let Ok(value) = HeaderValue::from_bytes(&latin1_bytes(item)) {
                map.append(name.clone(), value);
            }
        }
    }
    map
}

/// Flattened `(name, value)` pairs (array values repeat).
pub(crate) fn pairs(headers: &JsMap) -> Vec<(String, String)> {
    headers
        .iter()
        .flat_map(|(name, value)| value.values().map(move |v| (name.to_owned(), v.to_owned())))
        .collect()
}

struct Router<'s> {
    state: &'s AppState,
    headers: JsMap,
    /// The client's request had `rsc: 1` (before any proxy change).
    rsc_request: bool,
    headers_modified: bool,
    res_headers: JsMap,
    parsed: ParsedUrl,
    init_query: JsMap,
    did_rewrite: bool,
    middleware_cookies: Vec<String>,
    /// The proxy's response ended routing.
    from_proxy: bool,
}

impl Router<'_> {
    fn view(&self) -> RequestView<'_> {
        RequestView {
            headers: &self.headers,
            query: &self.parsed.query,
        }
    }

    fn is_rsc(&self) -> bool {
        if !self.headers_modified {
            return self.rsc_request;
        }
        self.headers
            .get(&self.state.rsc.header)
            .and_then(Value::as_str)
            .is_some_and(|v| v == "1")
    }

    /// `resHeaders[key] = value`, with `set-cookie` accumulating.
    fn set_res_header(&mut self, key: &str, value: Value) {
        let key = key.to_ascii_lowercase();
        if key == "set-cookie" {
            let mut items = match self.res_headers.get(&key) {
                Some(Value::Arr(items)) => items.clone(),
                Some(Value::Str(item)) => vec![item.clone()],
                None => Vec::new(),
            };
            items.extend(value.values().map(str::to_owned));
            self.res_headers.insert(key, Value::Arr(items));
        } else {
            self.res_headers.insert(key, value);
        }
    }

    fn apply_header_rule(&mut self, rule: &Rule, params: &JsMap) -> Result<(), String> {
        for (key, value) in &rule.headers {
            let (key, value) = if params.is_empty() {
                (key.clone(), value.clone())
            } else {
                (
                    compile_non_path(key, params)?,
                    compile_non_path(value, params)?,
                )
            };
            if key.eq_ignore_ascii_case("set-cookie") {
                self.set_res_header(&key, Value::Arr(vec![value]));
            } else {
                self.set_res_header(&key, Value::Str(value));
            }
        }
        Ok(())
    }

    /// A rewrite rule matched. Returns the external URL for a rewrite to
    /// another origin.
    fn apply_rewrite(
        &mut self,
        destination: &str,
        params: &JsMap,
    ) -> Result<Option<String>, String> {
        let dest = prepare_destination(destination, params, &self.parsed.query, true)?;
        if self.is_rsc() && dest.protocol.is_none() {
            if self.parsed.pathname != dest.pathname {
                self.res_headers
                    .insert(REWRITTEN_PATH, Value::Str(dest.pathname.clone()));
            }
            if self.parsed.search != dest.search {
                let search = dest
                    .search
                    .strip_prefix('?')
                    .unwrap_or(&dest.search)
                    .to_owned();
                self.res_headers.insert(REWRITTEN_QUERY, Value::Str(search));
            }
        }
        if dest.protocol.is_some() {
            // proxyRequest: `parsedUrl.search = stringifyQuery(req, query)`
            // (values from the request's own query encoded, the rest as the
            // rule wrote them).
            let mut external = dest;
            external.search = stringify_redirect_query(&external.query, &self.init_query);
            return Ok(Some(format_url(&external)));
        }
        self.did_rewrite = true;
        self.parsed.pathname = dest.pathname;
        self.parsed.query.assign(&dest.query);
        Ok(None)
    }
}

/// Runs the routing phases for one request.
pub(crate) async fn resolve(
    state: &AppState,
    request: RouteRequest<'_>,
) -> (Outcome, Resolved, Body) {
    let RouteRequest {
        method,
        uri,
        version,
        headers,
        body,
        client,
    } = request;
    let raw_url = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_owned(), |pq| pq.as_str().to_owned());
    let url_no_query = raw_url.split('?').next().unwrap_or_default().to_owned();
    // Node's header object is only built when something reads it: `has` /
    // `missing` conditions or the proxy (decided once, at startup).
    let node = if state.needs_node_headers {
        node_headers(headers)
    } else {
        JsMap::new()
    };
    let rsc_request = headers
        .get(state.rsc.header.as_str())
        .is_some_and(|v| v.as_bytes() == b"1");

    let done =
        |outcome: Outcome, router: Option<Router<'_>>, body: Body| -> (Outcome, Resolved, Body) {
            let resolved = match router {
                Some(router) => Resolved {
                    headers: router.headers,
                    headers_modified: router.headers_modified,
                    res_headers: router.res_headers,
                    pathname: router.parsed.pathname,
                    query: router.parsed.query,
                    did_rewrite: router.did_rewrite,
                    middleware_cookies: router.middleware_cookies,
                    from_proxy: router.from_proxy,
                },
                None => Resolved {
                    headers: JsMap::new(),
                    headers_modified: false,
                    res_headers: JsMap::new(),
                    pathname: url_no_query.clone(),
                    query: JsMap::new(),
                    did_rewrite: false,
                    middleware_cookies: Vec::new(),
                    from_proxy: false,
                },
            };
            (outcome, resolved, body)
        };

    // Repeated slashes and backslashes: a 308 to the normalized URL.
    if url_no_query.contains("//") || url_no_query.contains('\\') {
        let normalized = normalize_repeated_slashes(&raw_url);
        let location = match parse_url(&normalized) {
            Ok(parsed) => format_url(&parsed),
            Err(_) => normalized,
        };
        return done(
            Outcome::Redirect {
                status: 308,
                location,
                with_res_headers: true,
            },
            None,
            body,
        );
    }

    let parsed = match parse_relative_url(&raw_url) {
        Ok(parsed) => parsed,
        Err(_) => {
            return done(
                Outcome::Respond(dynamic::error_response(
                    StatusCode::BAD_REQUEST,
                    "bad-request-url",
                    "the request URL could not be parsed",
                )),
                None,
                body,
            );
        }
    };
    let mut router = Router {
        state,
        init_query: parsed.query.clone(),
        headers: node,
        rsc_request,
        headers_modified: false,
        res_headers: JsMap::new(),
        parsed,
        did_rewrite: false,
        middleware_cookies: Vec::new(),
        from_proxy: false,
    };
    let mut body = body;

    // `middleware_next_data`: with a proxy, `/_next/data/<build>/x.json`
    // routes as `/x` and is marked as a data request (the proxy's adapter
    // then answers redirects with `x-nextjs-redirect`).
    if state.proxy.is_some() {
        next_data(state, &mut router);
    }

    macro_rules! fail {
        ($error:expr) => {{
            tracing::warn!(error = %$error, "routing failed");
            return done(Outcome::ProxyFailed, Some(router), body);
        }};
    }

    // next.config headers: every matching rule adds to resHeaders.
    for rule in &state.rules.headers {
        if let Some(params) = rule.matches(&router.parsed.pathname, &router.view())
            && let Err(error) = router.apply_header_rule(rule, &params)
        {
            fail!(error);
        }
    }

    // Redirects: the first match ends routing.
    for rule in &state.rules.redirects {
        let Some(params) = rule.matches(&router.parsed.pathname, &router.view()) else {
            continue;
        };
        let Some(destination) = &rule.destination else {
            continue;
        };
        let mut dest = match prepare_destination(destination, &params, &router.parsed.query, false)
        {
            Ok(dest) => dest,
            Err(error) => fail!(error),
        };
        dest.search = stringify_redirect_query(&dest.query, &router.init_query);
        dest.pathname = normalize_repeated_slashes(&dest.pathname);
        let status = rule.status.unwrap_or(307);
        return done(
            Outcome::Redirect {
                status,
                location: format_url(&dest),
                with_res_headers: false,
            },
            Some(router),
            body,
        );
    }

    // The proxy.
    if let Some(proxy) = &state.proxy
        && proxy
            .matchers
            .matches(&router.parsed.pathname, &router.view())
    {
        // next-server checks a data path before it runs the proxy (its
        // handleNextDataRequest runs for the proxy's invocation too): one
        // for this build that does not end in .json renders the 404 page
        // at once, without the proxy and before routing applied its
        // response headers.
        if crate::pages::is_unservable_data_path(state, &router.parsed.pathname) {
            router.res_headers = JsMap::new();
            return done(Outcome::NotFound, Some(router), body);
        }
        match run_proxy(state, &mut router, method, &raw_url, version, client, body).await {
            ProxyStep::Continue(rest) => body = rest,
            ProxyStep::Finish(outcome, rest) => {
                router.from_proxy = true;
                return done(outcome, Some(router), rest);
            }
        }
    }

    // beforeFiles rewrites: applied in order, no filesystem check.
    for rule in &state.rules.before_files {
        let Some(params) = rule.matches(&router.parsed.pathname, &router.view()) else {
            continue;
        };
        let Some(destination) = rule.destination.clone() else {
            continue;
        };
        match router.apply_rewrite(&destination, &params) {
            Ok(Some(external)) => return done(Outcome::External(external), Some(router), body),
            Ok(None) => {}
            Err(error) => fail!(error),
        }
    }

    // Exact filesystem matches before afterFiles rewrites.
    if let Some(output) = get_item(state, &router.parsed.pathname).await {
        return done(Outcome::Output(output), Some(router), body);
    }

    // afterFiles rewrites, each followed by a check (files, then dynamic
    // routes); then the check on its own; then fallback rewrites.
    for rule in &state.rules.after_files {
        if let Some(outcome) = rewrite_and_check(&mut router, rule).await {
            match outcome {
                Ok(outcome) => return done(outcome, Some(router), body),
                Err(error) => fail!(error),
            }
        }
    }
    if let Some(output) = check_true(state, &router.parsed.pathname).await {
        return done(Outcome::Output(output), Some(router), body);
    }
    for rule in &state.rules.fallback {
        if let Some(outcome) = rewrite_and_check(&mut router, rule).await {
            match outcome {
                Ok(outcome) => return done(outcome, Some(router), body),
                Err(error) => fail!(error),
            }
        }
    }
    done(Outcome::NotFound, Some(router), body)
}

/// A request routed by its pathname alone (filesystem, then dynamic
/// routes), without next.config rules or the proxy: next-server's own
/// route matching, which its `revalidate()` requests take.
pub(crate) async fn direct(
    state: &AppState,
    request: RouteRequest<'_>,
) -> (Outcome, Resolved, Body) {
    let pathname = request.uri.path().to_owned();
    let query = request
        .uri
        .query()
        .and_then(|q| parse_relative_url(&format!("/?{q}")).ok())
        .map(|parsed| parsed.query)
        .unwrap_or_default();
    let outcome = match check_true(state, &pathname).await {
        Some(output) => Outcome::Output(output),
        None => Outcome::NotFound,
    };
    let resolved = Resolved {
        headers: JsMap::new(),
        headers_modified: false,
        res_headers: JsMap::new(),
        pathname,
        query,
        did_rewrite: false,
        middleware_cookies: Vec::new(),
        from_proxy: false,
    };
    (outcome, resolved, request.body)
}

fn has_prefix(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// `denormalizePagePath` for a data path.
fn denormalize_page_path(page: &str) -> String {
    if page.starts_with("/index/") && !page.contains('[') {
        page["/index".len()..].to_owned()
    } else if page == "/index" {
        "/".to_owned()
    } else {
        page.to_owned()
    }
}

fn next_data(state: &AppState, router: &mut Router<'_>) {
    let base = state.config.base_path.as_str();
    let pathname = router.parsed.pathname.clone();
    let had_base = !base.is_empty() && has_prefix(&pathname, base);
    let mut normalized = if had_base {
        strip_base_path(&pathname, base).unwrap_or("/").to_owned()
    } else {
        pathname.clone()
    };
    let is_data_path = has_prefix(&normalized, "/_next/data") && normalized.ends_with(".json");
    let prefix = format!("/_next/data/{}", state.build_id);
    let current_build = has_prefix(&normalized, &prefix) && normalized.ends_with(".json");
    if current_build {
        let rest = if normalized.len() == prefix.len() {
            "/"
        } else {
            &normalized[prefix.len()..]
        };
        let rest = rest.strip_suffix(".json").unwrap_or(rest);
        normalized = denormalize_page_path(rest);
    }
    if is_data_path {
        router
            .headers
            .insert("x-nextjs-data", Value::Str("1".into()));
        router.headers_modified = true;
    }
    if current_build {
        if had_base {
            normalized = if normalized == "/" {
                base.to_owned()
            } else {
                format!("{base}{normalized}")
            };
        }
        if state.config.trailing_slash && !normalized.ends_with('/') {
            normalized.push('/');
        }
        router.parsed.pathname = normalized;
    }
}

async fn rewrite_and_check(
    router: &mut Router<'_>,
    rule: &Rule,
) -> Option<Result<Outcome, String>> {
    debug_assert!(matches!(
        rule.kind,
        RuleKind::AfterFiles | RuleKind::Fallback
    ));
    let params = rule.matches(&router.parsed.pathname, &router.view())?;
    let destination = rule.destination.clone()?;
    match router.apply_rewrite(&destination, &params) {
        Ok(Some(external)) => return Some(Ok(Outcome::External(external))),
        Ok(None) => {}
        Err(error) => return Some(Err(error)),
    }
    let state = router.state;
    check_true(state, &router.parsed.pathname)
        .await
        .map(|output| Ok(Outcome::Output(output)))
}

/// `fsChecker.getItem(pathname)`.
pub(crate) async fn get_item(state: &AppState, pathname: &str) -> Option<Output> {
    let base = state.config.base_path.as_str();
    let mut item = pathname;
    if !base.is_empty() {
        item = strip_base_path(item, base)?;
    }
    let item = if item != "/" && item.ends_with('/') {
        &item[..item.len() - 1]
    } else {
        item
    };
    let decoded = decode_uri_component(item).unwrap_or_else(|| item.to_owned());
    if item == "/_next/image" {
        return Some(Output::Image);
    }
    // `/_next/static` and `public/`: `static_files::resolve` decodes the
    // path itself (so the raw and the decoded spelling resolve alike), and
    // `public/` is listed once at startup, as next start does, so a page
    // request costs no filesystem lookup.
    if let Some(rest) = item.strip_prefix("/_next/static/")
        && let Some(root) = &state.static_root
        && let Some(file) = crate::static_files::resolve(root, rest).await
    {
        return Some(Output::Static(file));
    }
    if let Some(root) = &state.public_root
        && state.public_files.contains(decoded.trim_start_matches('/'))
        && let Some(file) = crate::static_files::resolve(root, item.trim_start_matches('/')).await
    {
        return Some(Output::Public(file));
    }
    // App routes match the path as routed, not its decoded form: the
    // proxy saw the encoded path (`/%70rotected` is not `/protected` to its
    // checks), and next start answers such a request with a 404.
    if let Some(route) = state.exact_route(item) {
        return Some(Output::App {
            route,
            item_path: item.to_owned(),
            data: None,
        });
    }
    // A Pages Router data request for a page without parameters.
    if let Some(page) = crate::pages::static_data_page(state, item)
        && let Some(route) = state.exact_route(&page)
    {
        return Some(Output::App {
            route,
            item_path: page,
            data: Some(crate::pages::DataMatch::Static),
        });
    }
    (decoded != item && state.exact_route(&decoded).is_some()).then_some(Output::EncodedApp)
}

/// `checkTrue()`: the filesystem, then dynamic routes on the pathname
/// without basePath.
pub(crate) async fn check_true(state: &AppState, pathname: &str) -> Option<Output> {
    if let Some(output) = get_item(state, pathname).await {
        return Some(output);
    }
    let base = state.config.base_path.as_str();
    let path = if base.is_empty() {
        pathname
    } else {
        strip_base_path(pathname, base)?
    };
    // Data routes come first among next start's dynamic routes (a page
    // catch-all would match their paths too).
    if let Some((page, pathname)) = crate::pages::dynamic_data_page(state, path)
        && let Some(route) = state.routes.iter().position(|r| r.page == page)
    {
        return Some(Output::App {
            route,
            item_path: pathname,
            data: Some(crate::pages::DataMatch::Dynamic),
        });
    }
    state.dynamic_route(path).map(|route| Output::App {
        route,
        item_path: path.to_owned(),
        data: None,
    })
}

/// `removePathPrefix` when the path has the prefix (`pathHasPrefix`).
pub(crate) fn strip_base_path<'p>(path: &'p str, base: &str) -> Option<&'p str> {
    let rest = path.strip_prefix(base)?;
    if rest.is_empty() {
        Some("/")
    } else if rest.starts_with('/') || rest.starts_with('?') || rest.starts_with('#') {
        Some(rest)
    } else {
        None
    }
}

enum ProxyStep {
    Continue(Body),
    Finish(Outcome, Body),
}

/// Buffers a request body for the proxy and the route (next start's
/// cloneable body). Over the limit: 413, where next start would truncate
/// both copies silently.
async fn buffer_body(body: Body, limit: u64) -> Result<Bytes, Box<Response>> {
    use http_body::Body as _;
    if body.is_end_stream() {
        return Ok(Bytes::new());
    }
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| Box::new(dynamic::too_large(limit as u64)))
}

#[allow(clippy::too_many_arguments)]
async fn run_proxy(
    state: &AppState,
    router: &mut Router<'_>,
    method: &Method,
    raw_url: &str,
    version: Version,
    client: Option<std::net::SocketAddr>,
    body: Body,
) -> ProxyStep {
    let Some(proxy) = &state.proxy else {
        return ProxyStep::Continue(body);
    };
    let Some(executor) = &state.executor else {
        return ProxyStep::Finish(
            Outcome::Respond(crate::unsupported(
                "proxy",
                "this build has a proxy (middleware), which requires JavaScript execution (start with --js-runtime embedded-v8)",
            )),
            body,
        );
    };

    // The body is buffered whole for the proxy and kept for the route.
    let has_body = *method != Method::GET && *method != Method::HEAD;
    let limit = proxy.body_limit.min(state.max_request_body);
    let bytes = if has_body {
        match buffer_body(body, limit).await {
            Ok(bytes) => bytes,
            Err(response) => return ProxyStep::Finish(Outcome::Respond(*response), Body::empty()),
        }
    } else {
        drop(body);
        Bytes::new()
    };
    let rest = || {
        if bytes.is_empty() {
            Body::empty()
        } else {
            Body::from(bytes.clone())
        }
    };

    // `${initProtocol}://${fetchHostname || 'localhost'}:${port}${pathname}?query`
    // (the host is next start's hostname; see `Listen`).
    let protocol = init_protocol(&router.headers);
    let query = url_query_to_search_params(&router.parsed.query);
    let url = format!(
        "{protocol}://{}:{}{}{}",
        state.listen.host,
        state.listen.port,
        router.parsed.pathname,
        if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        }
    );
    let init_url = format!(
        "{protocol}://{}:{}{raw_url}",
        state.listen.host, state.listen.port
    );

    // base-server's defaults for the forwarded headers, set on the request
    // before the proxy runs (they stay on it for the route).
    let defaults = [
        ("x-forwarded-host", router.headers.get("host").cloned()),
        (
            "x-forwarded-port",
            Some(Value::Str(state.listen.port.to_string())),
        ),
        ("x-forwarded-proto", Some(Value::Str(protocol.to_owned()))),
        (
            "x-forwarded-for",
            client.map(|c| Value::Str(c.ip().to_string())),
        ),
    ];
    for (name, value) in defaults {
        if !router.headers.contains_key(name)
            && let Some(value) = value
        {
            router.headers.insert(name, value);
            router.headers_modified = true;
        }
    }
    let request_headers = header_map(&router.headers);
    let uri: Uri = match raw_url.parse() {
        Ok(uri) => uri,
        Err(_) => return ProxyStep::Finish(Outcome::ProxyFailed, rest()),
    };
    let policy = dynamic::AppPathPolicy {
        headers: &[],
        vary: None,
        status: None,
        invocation: Invocation::Proxy { url },
    };
    let incoming = dynamic::Incoming {
        method,
        uri: &uri,
        headers: &request_headers,
        body: rest(),
        client,
        max_body: limit,
        listen: &state.listen,
    };
    let response = dynamic::render(executor, &proxy.entry, &policy, incoming).await;
    let response = dynamic::frame_for(version, response);
    let (parts, proxy_body) = response.into_parts();

    // Executor failures and proxy exceptions end the request: the route is
    // never served without its proxy.
    match parts
        .headers
        .get("x-nextrs-error")
        .map(HeaderValue::as_bytes)
    {
        Some(b"proxy-exception") => return ProxyStep::Finish(Outcome::ProxyFailed, rest()),
        Some(_) => {
            return ProxyStep::Finish(
                Outcome::Respond(Response::from_parts(parts, proxy_body)),
                rest(),
            );
        }
        None => {}
    }

    // middlewareHeaders: Node outgoing headers of the NextResponse.
    let mut mh = JsMap::new();
    let mut set_cookies = Vec::new();
    for (name, value) in &parts.headers {
        let value = latin1(value.as_bytes());
        if name == "set-cookie" {
            set_cookies.push(value);
        } else {
            match mh.get(name.as_str()) {
                Some(Value::Str(existing)) => {
                    let joined = format!("{existing}, {value}");
                    mh.insert(name.as_str(), Value::Str(joined));
                }
                _ => mh.insert(name.as_str(), Value::Str(value)),
            }
        }
    }
    if !set_cookies.is_empty() {
        router.middleware_cookies = set_cookies.clone();
        let value = if set_cookies.len() == 1 {
            Value::Str(set_cookies.remove(0))
        } else {
            Value::Arr(set_cookies)
        };
        mh.insert("set-cookie", value);
    }
    let status = parts.status.as_u16();
    let mut direct_body = Some(proxy_body);

    // x-middleware-override-headers: the request headers the route sees.
    if let Some(list) = mh.remove("x-middleware-override-headers") {
        router.headers_modified = true;
        let overridden: Vec<String> = list
            .values()
            .flat_map(|v| v.split(','))
            .map(|k| k.trim().to_owned())
            .collect();
        let existing: Vec<String> = router.headers.keys().map(str::to_owned).collect();
        for key in existing {
            if !overridden.contains(&key) {
                router.headers.remove(&key);
            }
        }
        for key in &overridden {
            let value_key = format!("x-middleware-request-{key}");
            match mh.remove(&value_key) {
                Some(value) => router.headers.insert(key.clone(), value),
                None => {
                    router.headers.remove(key);
                }
            }
        }
    }
    let rewrite = mh.get("x-middleware-rewrite").cloned();
    let location = mh.get("location").cloned();
    if rewrite.is_none() && !mh.contains_key("x-middleware-next") && location.is_none() {
        mh.insert("x-middleware-refresh", Value::Str("1".into()));
    }
    mh.remove("x-middleware-next");
    if mh.get("content-length").and_then(Value::as_str) == Some("0") {
        mh.remove("content-length");
    }
    let entries: Vec<(String, Value)> = mh
        .iter()
        .filter(|(key, _)| !IPC_FORBIDDEN.contains(key))
        .map(|(k, v)| (k.to_owned(), v.clone()))
        .collect();
    for (key, value) in entries {
        if matches!(
            key.as_str(),
            "content-length"
                | "x-middleware-rewrite"
                | "x-middleware-redirect"
                | "x-middleware-refresh"
        ) {
            continue;
        }
        if key == "x-middleware-set-cookie" {
            router.headers.insert(key, value);
            router.headers_modified = true;
            continue;
        }
        if value.is_truthy() {
            router.res_headers.insert(key.clone(), value.clone());
            router.headers.insert(key, value);
            router.headers_modified = true;
        }
    }

    if let Some(rewrite) = rewrite.as_ref().and_then(Value::as_str) {
        let destination = match relative_url(rewrite, &init_url) {
            Ok(destination) => destination,
            Err(error) => {
                tracing::warn!(%error, "invalid proxy rewrite");
                return ProxyStep::Finish(Outcome::ProxyFailed, rest());
            }
        };
        router
            .res_headers
            .insert("x-middleware-rewrite", Value::Str(destination.clone()));
        match parse_url(&destination) {
            Ok(mut parsed) if parsed.protocol.is_some() => {
                // As for config rewrites: proxyRequest re-stringifies the
                // destination's query (stringifyQuery).
                parsed.search = stringify_redirect_query(&parsed.query, &router.init_query);
                return ProxyStep::Finish(Outcome::External(format_url(&parsed)), rest());
            }
            Ok(parsed) => {
                // resolve-routes replaces parsedUrl (query included); the
                // route needs the rewritten params and query as meta.
                router.parsed = parsed;
                router.did_rewrite = true;
            }
            Err(error) => {
                tracing::warn!(%error, "invalid proxy rewrite");
                return ProxyStep::Finish(Outcome::ProxyFailed, rest());
            }
        }
    }
    if let Some(location) = location.as_ref().and_then(Value::as_str) {
        if REDIRECT_STATUSES.contains(&status) {
            let relative = match relative_url(location, &init_url) {
                Ok(relative) => relative,
                Err(error) => {
                    tracing::warn!(%error, "invalid proxy redirect");
                    return ProxyStep::Finish(Outcome::ProxyFailed, rest());
                }
            };
            router
                .res_headers
                .insert("location", Value::Str(relative.clone()));
            let location = match parse_url(&relative) {
                Ok(parsed) => format_url(&parsed),
                Err(_) => relative,
            };
            return ProxyStep::Finish(
                Outcome::Redirect {
                    status,
                    location,
                    with_res_headers: true,
                },
                rest(),
            );
        }
        // Not a redirect status: Location passes through, the body is sent.
        router
            .res_headers
            .insert("location", Value::Str(location.to_owned()));
        let response = direct_response(router, status, direct_body.take().unwrap_or_default());
        return ProxyStep::Finish(Outcome::Respond(response), rest());
    }
    if mh.contains_key("x-middleware-refresh") {
        let response = direct_response(router, status, direct_body.take().unwrap_or_default());
        return ProxyStep::Finish(Outcome::Respond(response), rest());
    }
    // Continue: drain the (empty) body so the request's waitUntil work in
    // JavaScript keeps running instead of being cancelled.
    if let Some(body) = direct_body.take() {
        let _ = axum::body::to_bytes(body, 1024 * 1024).await;
    }
    ProxyStep::Continue(rest())
}

/// router-server for a proxy body response: `resHeaders`, then the body
/// with the proxy's status.
fn direct_response(router: &Router<'_>, status: u16, body: Body) -> Response {
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    let headers = header_map(&router.res_headers);
    *response.headers_mut() = headers;
    response
}

/// resolve-routes' `initProtocol`: `https` when `X-Forwarded-Proto` says
/// so (the socket is never TLS here).
fn init_protocol(headers: &JsMap) -> &'static str {
    match headers.get("x-forwarded-proto") {
        Some(value) if value.values().any(|v| v.contains("https")) => "https",
        _ => "http",
    }
}
