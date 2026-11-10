//! Pages Router serving: what next start's router-server and base server do
//! around a Pages Router route. Next's compiled handlers do all rendering
//! and data fetching (`getServerSideProps`, `getStaticProps` with its cache
//! and fallbacks, API routes, `/_next/data`); Rust serves only what Next
//! itself serves as a file (pages without data functions, the 404 and 500
//! pages) and decides which page renders an error, as next start's
//! `renderErrorToResponse` does.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use axum::body::Body;
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use nextrs_adapter::{BuildInfo, RouteKind};
use regex::Regex;

use crate::{Cached, HTML, NO_STORE, StartError, dynamic};

/// How a `/_next/data/<build id>/<page>.json` request matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DataMatch {
    /// A page without parameters (`fsChecker.getItem`'s `nextDataRoutes`).
    Static,
    /// A dynamic data route: next start then marks the request as a data
    /// request (`x-nextjs-data`) before rendering.
    Dynamic,
}

/// The markers JavaScript leaves on a response that Rust must replace
/// (see `js/03_server.js`).
pub(crate) const MARKER: &str = "x-nextrs-error";
/// `routerServerContext.render404`: the handler decided the page is not
/// found (`notFound: true`) after setting its own status and headers.
const RENDER_404: &str = "render-404";
const HANDLER_EXCEPTION: &str = "handler-exception";
const NO_FALLBACK: &str = "no-fallback";

#[derive(Default)]
pub(crate) struct PagesState {
    /// Pages without data functions (`pages/about.html`), by pathname,
    /// with the status next start sends them with (404 and 500 for the
    /// status pages requested by name).
    pub static_pages: HashMap<String, (StatusCode, Cached)>,
    /// `pages/404.html` (with i18n, the default locale's).
    pub not_found: Option<Cached>,
    /// `pages/500.html` (with i18n, the default locale's).
    pub server_error: Option<Cached>,
    /// With i18n: `pages/<locale>/404.html` and `pages/<locale>/500.html`.
    pub localized_not_found: HashMap<String, Cached>,
    pub localized_server_error: HashMap<String, Cached>,
    /// `pages/404.js`, a 404 page with getStaticProps.
    pub not_found_entry: Option<PathBuf>,
    /// `pages/_error.js`.
    pub error_entry: Option<PathBuf>,
    /// Pages with a data route and no parameters.
    pub data_static: HashSet<String>,
    /// Dynamic data routes, in routes-manifest order, with their page.
    pub data_dynamic: Vec<(Regex, String)>,
    /// Pages with getStaticProps.
    pub ssg: HashSet<String>,
    /// The build has `server/pages` (next-server's `enabledDirectories.pages`,
    /// true for App Router builds too: they carry the default 404 page).
    pub pages_dir: bool,
}

impl PagesState {
    pub(crate) async fn load(info: &BuildInfo) -> Result<Self, StartError> {
        let mut state = Self {
            pages_dir: info.dist_dir.join("server/pages").is_dir(),
            ..Self::default()
        };
        if !info.routers.pages {
            return Ok(state);
        }
        for prerender in info
            .prerenders
            .iter()
            .filter(|p| p.pages && p.data.is_none())
        {
            let Some(html) = crate::load_cached(prerender.html.as_ref(), true).await? else {
                continue;
            };
            let status = StatusCode::from_u16(prerender.status).unwrap_or(StatusCode::OK);
            let default = info.config.i18n.as_ref().map(|i| i.default_locale.as_str());
            let is_default = prerender.locale.as_deref() == default;
            match (prerender.route.as_str(), &prerender.locale) {
                ("/404", locale) => {
                    if is_default {
                        state.not_found = Some(html.clone());
                    }
                    if let Some(locale) = locale {
                        state
                            .localized_not_found
                            .insert(locale.clone(), html.clone());
                    }
                }
                ("/500", locale) => {
                    if is_default {
                        state.server_error = Some(html.clone());
                    }
                    if let Some(locale) = locale {
                        state
                            .localized_server_error
                            .insert(locale.clone(), html.clone());
                    }
                }
                _ => {}
            }
            state
                .static_pages
                .insert(prerender.pathname.clone(), (status, html));
        }
        state.error_entry = info.pages_router.error_entry.clone();
        state.not_found_entry = info.pages_router.not_found_entry.clone();
        for data in &info.pages_router.data_routes {
            if data.dynamic {
                let source = if info.config.i18n.is_some() {
                    add_locale_to_data_regex(&data.regex, &info.build_id)
                } else {
                    data.regex.clone()
                };
                let regex = Regex::new(&source).map_err(|error| StartError::RouteRegex {
                    page: data.page.clone(),
                    error,
                })?;
                state.data_dynamic.push((regex, data.page.clone()));
            } else {
                state.data_static.insert(data.page.clone());
            }
        }
        state.ssg = info.pages_router.ssg_pages.iter().cloned().collect();
        Ok(state)
    }
}

/// Next's `addLocalePrefixToDataRouteRegex` (router-utils/build-data-route):
/// with i18n, a data route's regex takes one more segment after the build
/// id (any segment: the locale is checked later).
fn add_locale_to_data_regex(regex: &str, build_id: &str) -> String {
    let segment = format!("/{}", escape_string_regexp(build_id));
    match regex.find(&segment) {
        Some(index) => {
            let at = index + segment.len();
            format!("{}/(?:[^/]+?){}", &regex[..at], &regex[at..])
        }
        None => regex.to_owned(),
    }
}

/// Next's `escapeStringRegexp`.
fn escape_string_regexp(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "|\\{}()[]^$+*?.-".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A page's key among the outputs Next keys by locale (`/fr/about`, and
/// `/fr` for the index page). Without i18n, the path itself.
pub(crate) fn locale_key(path: &str, locale: Option<&crate::resolve::RouteLocale>) -> String {
    match locale {
        Some(l) if path == "/" => format!("/{}", l.locale),
        Some(l) => format!("/{}{path}", l.locale),
        None => path.to_owned(),
    }
}

/// `denormalizePagePath` of a data path's page part.
fn denormalize(page: &str) -> String {
    if page == "/index" {
        "/".to_owned()
    } else if page.starts_with("/index/") && !page.contains('[') {
        page["/index".len()..].to_owned()
    } else {
        page.to_owned()
    }
}

/// `fsChecker.getItem` for `/_next/data/<build id>/<page>.json` of a page
/// without parameters: the page's pathname.
pub(crate) fn static_data_page(state: &crate::AppState, item: &str) -> Option<String> {
    if state.pages_router.data_static.is_empty() {
        return None;
    }
    let prefix = format!("/_next/data/{}/", state.build_id);
    let rest = item.strip_prefix(&prefix)?.strip_suffix(".json")?;
    let page = format!("/{rest}");
    // With i18n, fsChecker strips the locale before `/index` becomes `/`.
    let page = match &state.i18n {
        Some(i18n) => match i18n.split(&page).0 {
            "/index" => "/".to_owned(),
            other => other.to_owned(),
        },
        None => denormalize(&page),
    };
    state
        .pages_router
        .data_static
        .contains(&page)
        .then_some(page)
}

/// A data path whose page names a data route only once percent-decoded
/// (`/_next/data/<build>/%70rotected/secret.json`): next start's filesystem
/// router matches it, but its page matching does not, and it renders the
/// 404 page instead (as for page paths: `Output::EncodedApp`).
pub(crate) fn encoded_data_page(state: &crate::AppState, item: &str) -> bool {
    let prefix = format!("/_next/data/{}/", state.build_id);
    let Some(rest) = item
        .strip_prefix(&prefix)
        .and_then(|r| r.strip_suffix(".json"))
    else {
        return false;
    };
    let Some(decoded) = crate::routing::query::decode_uri_component(&format!("/{rest}")) else {
        return false;
    };
    let decoded_item = format!("{prefix}{}.json", &decoded[1..]);
    decoded_item != item && static_data_page(state, &decoded_item).is_some()
}

/// With i18n: a `/_next/data/<build>/<page>.json` path whose page part names
/// no locale (getNextPathnameInfo finds none).
pub(crate) fn data_path_without_locale(state: &crate::AppState, pathname: &str) -> bool {
    let Some(i18n) = &state.i18n else {
        return false;
    };
    let path = crate::i18n::remove_path_prefix(pathname, &state.config.base_path);
    path.starts_with("/_next/data/") && i18n.path_locale(path).is_none()
}

/// The page path a data response names (`x-nextjs-matched-path`): without
/// basePath or a trailing slash, as next start's page pathnames are.
pub(crate) fn matched_path(state: &crate::AppState, pathname: &str) -> String {
    let base = state.config.base_path.as_str();
    let path = if base.is_empty() {
        pathname
    } else {
        crate::resolve::strip_base_path(pathname, base).unwrap_or(pathname)
    };
    if path.len() > 1 && path.ends_with('/') {
        path[..path.len() - 1].to_owned()
    } else {
        path.to_owned()
    }
}

/// `/_next/data/<this build>/...` without the `.json` suffix, in a build
/// with a `server/pages` directory (next-server's `handleNextDataRequest`
/// renders the 404 page for it).
pub(crate) fn is_unservable_data_path(state: &crate::AppState, pathname: &str) -> bool {
    if !state.pages_router.pages_dir {
        return false;
    }
    // Matched on the path as requested: with a basePath it starts with the
    // basePath, which next-server's matchNextDataPathname does not expect,
    // and the proxy runs.
    let prefix = format!("/_next/data/{}", state.build_id);
    let Some(rest) = pathname.strip_prefix(&prefix) else {
        return false;
    };
    // matchNextDataPathname: the build id must be a whole segment.
    if !(rest.is_empty() || rest.starts_with('/')) {
        return false;
    }
    let last = rest.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    !last.ends_with(".json")
}

/// A dynamic data route matching `path` (without basePath): its page and
/// the page pathname the data is for.
pub(crate) fn dynamic_data_page<'s>(
    state: &'s crate::AppState,
    path: &str,
) -> Option<(&'s str, String)> {
    let (_, page) = state
        .pages_router
        .data_dynamic
        .iter()
        .find(|(regex, _)| regex.is_match(path))?;
    #[cfg(feature = "negative-controls")]
    let page = if crate::pages_controls::wrong_data_route() {
        &state.pages_router.data_dynamic[0].1
    } else {
        page
    };
    let prefix = format!("/_next/data/{}", state.build_id);
    let pathname = path
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix(".json"))
        .map(denormalize)
        .unwrap_or_else(|| path.to_owned());
    let pathname = match &state.i18n {
        Some(i18n) => i18n.split(&pathname).0.to_owned(),
        None => pathname,
    };
    Some((page.as_str(), pathname))
}

/// A page Next wrote as plain HTML: next start sends the file with its
/// ETag and `X-Powered-By`, without Cache-Control (no `x-nextjs-cache`:
/// it is not a cache entry).
pub(crate) fn html_response(
    state: &crate::AppState,
    cached: &Cached,
    status: StatusCode,
    cache_control: Option<&'static str>,
    request: &HeaderMap,
    method: &Method,
) -> Response {
    let mut response = Response::new(Body::empty());
    let headers = response.headers_mut();
    if let Some(cache_control) = cache_control {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control),
        );
    }
    if state.config.powered_by_header {
        headers.insert("x-powered-by", HeaderValue::from_static("Next.js"));
    }
    let etag = cached.etag.as_ref().filter(|_| state.config.generate_etags);
    if let Some(etag) = etag {
        headers.insert(header::ETAG, etag.clone());
    }
    // sendRenderResult: whatever the status (a 404 page too).
    if crate::etag::fresh(request, etag.and_then(|e| e.to_str().ok()), None) {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        return response;
    }
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(HTML));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(cached.body.len()));
    *response.status_mut() = status;
    if method != Method::HEAD {
        *response.body_mut() = Body::from(cached.body.clone());
    }
    response
}

/// base server's setVaryHeader for a Pages Router page: the RSC Vary list
/// when the request is an RSC request (rsc: 1), whatever the page.
pub(crate) fn rsc_vary<'s>(state: &'s crate::AppState, request: &HeaderMap) -> Option<&'s str> {
    request
        .get(&state.rsc.header)
        .is_some_and(|v| v == "1")
        .then_some(state.rsc.vary_header.as_str())
}

fn add_rsc_vary(state: &crate::AppState, request: &HeaderMap, response: &mut Response) {
    if let Some(vary) = rsc_vary(state, request)
        && let Ok(value) = HeaderValue::from_str(vary)
    {
        response.headers_mut().append(header::VARY, value);
    }
}

/// next start's answer to a method other than GET or HEAD on a static or
/// getStaticProps page (base server, before rendering).
pub(crate) fn method_not_allowed() -> Response {
    let mut response = Response::new(Body::from("Method Not Allowed"));
    *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
    let headers = response.headers_mut();
    headers.append(header::ALLOW, HeaderValue::from_static("GET"));
    headers.append(header::ALLOW, HeaderValue::from_static("HEAD"));
    response
}

/// The not-found page of a build whose 404 comes from the Pages Router
/// (no App Router `_not-found`): `pages/404.html`, else `_error` rendered
/// with status 404. `headers` are the response headers already set (the
/// routing layer's or the handler's); `cache_control` is router-server's
/// no-store for a request routing did not match.
pub(crate) async fn not_found(
    state: &crate::AppState,
    ctx: Ctx<'_>,
    keep: Vec<(HeaderName, HeaderValue)>,
    cache_control: Option<&'static str>,
) -> Option<Response> {
    // With i18n, the page of the request's locale (render404 renders
    // `/<locale>/404`).
    let localized = match ctx.locale {
        Some(l) => state.pages_router.localized_not_found.get(&l.locale),
        None => state.pages_router.not_found.as_ref(),
    };
    if let Some(page) = localized {
        let mut response = html_response(
            state,
            page,
            StatusCode::NOT_FOUND,
            cache_control,
            ctx.headers,
            ctx.method,
        );
        merge(&mut response, keep);
        add_rsc_vary(state, ctx.headers, &mut response);
        return Some(response);
    }
    // A 404 page with getStaticProps renders through its own handler (it
    // serves its cache entry, `/<locale>/404`); else _error with 404.
    let entry = state
        .pages_router
        .not_found_entry
        .as_deref()
        .or(state.pages_router.error_entry.as_deref())?;
    let executor = state.executor.as_deref()?;
    let mut initial = ctx.initial.to_vec();
    if let Some(cache_control) = cache_control {
        initial.push(("cache-control".to_owned(), cache_control.to_owned()));
    }
    initial.extend(
        keep.iter()
            .filter_map(|(n, v)| Some((n.as_str().to_owned(), v.to_str().ok()?.to_owned()))),
    );
    let policy = dynamic::AppPathPolicy {
        headers: &initial,
        vary: rsc_vary(state, ctx.headers),
        status: Some(404),
        invocation: locale_invocation(ctx.locale),
    };
    let response = dynamic::render(executor, entry, &policy, ctx.incoming(Body::empty())).await;
    Some(dynamic::frame_for(ctx.version, response))
}

/// The error page for an exception in a page's handler (next start's
/// renderErrorToResponse with status 500): `pages/500.html`, else `_error`
/// rendered with status 500.
/// Headers the handler set before it failed stay (next start renders the
/// error page into the same response).
async fn server_error(
    state: &crate::AppState,
    ctx: Ctx<'_>,
    keep: Vec<(HeaderName, HeaderValue)>,
) -> Option<Response> {
    let localized = match ctx.locale {
        Some(l) => state.pages_router.localized_server_error.get(&l.locale),
        None => state.pages_router.server_error.as_ref(),
    };
    if let Some(page) = localized {
        let mut response = html_response(
            state,
            page,
            StatusCode::INTERNAL_SERVER_ERROR,
            None,
            ctx.headers,
            ctx.method,
        );
        merge(&mut response, keep);
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
        add_rsc_vary(state, ctx.headers, &mut response);
        return Some(response);
    }
    let entry = state.pages_router.error_entry.as_deref()?;
    let executor = state.executor.as_deref()?;
    let mut initial = ctx.initial.to_vec();
    initial.push(("cache-control".to_owned(), NO_STORE.to_owned()));
    let policy = dynamic::AppPathPolicy {
        headers: &initial,
        vary: rsc_vary(state, ctx.headers),
        status: Some(500),
        invocation: locale_invocation(ctx.locale),
    };
    let response = dynamic::render(executor, entry, &policy, ctx.incoming(Body::empty())).await;
    Some(dynamic::frame_for(ctx.version, response))
}

/// An error page's invocation: with i18n, the request's locale meta.
fn locale_invocation(locale: Option<&crate::resolve::RouteLocale>) -> nextrs_runtime::Invocation {
    match locale {
        Some(locale) => nextrs_runtime::Invocation::Routed(nextrs_runtime::RouteMeta {
            locale: Some(crate::locale_meta(locale)),
            ..Default::default()
        }),
        None => nextrs_runtime::Invocation::Handler,
    }
}

/// Headers set before a response is replaced by an error page are kept,
/// except those describing the body that was not sent.
fn kept_headers(response: &Response) -> Vec<(HeaderName, HeaderValue)> {
    response
        .headers()
        .iter()
        .filter(|(name, _)| {
            !matches!(
                name.as_str(),
                MARKER | "content-type" | "content-length" | "etag" | "x-powered-by"
            )
        })
        .map(|(n, v)| (n.clone(), v.clone()))
        .collect()
}

fn merge(response: &mut Response, keep: Vec<(HeaderName, HeaderValue)>) {
    let headers = response.headers_mut();
    for (name, value) in keep {
        if name == header::CACHE_CONTROL {
            headers.insert(name, value);
        } else {
            headers.append(name, value);
        }
    }
}

fn marker(response: &Response) -> Option<&str> {
    response.headers().get(MARKER).and_then(|v| v.to_str().ok())
}

/// One routed request, as `serve_resolved` has it.
#[derive(Clone, Copy)]
pub(crate) struct Ctx<'a> {
    pub method: &'a Method,
    pub uri: &'a axum::http::Uri,
    pub headers: &'a HeaderMap,
    pub client: Option<std::net::SocketAddr>,
    pub version: axum::http::Version,
    pub initial: &'a [(String, String)],
    pub max_body: u64,
    pub listen: &'a crate::Listen,
    /// With i18n: the request's locale meta.
    pub locale: Option<&'a crate::resolve::RouteLocale>,
}

impl<'a> Ctx<'a> {
    pub(crate) fn incoming(&self, body: Body) -> dynamic::Incoming<'a> {
        dynamic::Incoming {
            method: self.method,
            uri: self.uri,
            headers: self.headers,
            body,
            client: self.client,
            max_body: self.max_body,
            listen: self.listen,
        }
    }
}

pub(crate) enum Next {
    Done(Response),
    /// The handler found no page for these params (fallback: false):
    /// routing continues and ends at the not-found page.
    NotFound,
}

/// Serves a request routed to a Pages Router route.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn serve(
    state: &crate::AppState,
    route: &crate::CompiledRoute,
    path: &str,
    data: Option<DataMatch>,
    ctx: Ctx<'_>,
    body: Body,
    invocation: nextrs_runtime::Invocation,
) -> Next {
    let readonly = ctx.method == Method::GET || ctx.method == Method::HEAD;
    if route.kind == RouteKind::PagesPage {
        // base server validates an RSC request's cache-busting parameter
        // before it renders any page (the 404 page aside), a Pages Router
        // page too, and before its method checks.
        if path != "/404"
            && ctx.headers.get(&state.rsc.header).is_some_and(|v| v == "1")
            && let Some(redirect) =
                crate::rsc_redirect(state, ctx.uri.path(), ctx.uri.query(), ctx.headers)
        {
            return Next::Done(redirect);
        }
        // base server's middleware prefetch bail-out: next/link prefetches
        // a link the proxy matches with `x-middleware-prefetch`, and for a
        // page that is not statically generated next start answers `{}`
        // without rendering it (getServerSideProps never runs for a
        // prefetch).
        if ctx.headers.contains_key("x-middleware-prefetch")
            && !state.pages_router.ssg.contains(&route.page)
            && route.page != "/404"
            && route.page != "/_error"
        {
            return Next::Done(middleware_prefetch_skip(&route.page));
        }
        if let Some((status, html)) = state
            .pages_router
            .static_pages
            .get(&locale_key(path, ctx.locale))
        {
            let status_page = path == "/404" || path == "/500";
            if !readonly && !status_page {
                return Next::Done(method_not_allowed());
            }
            let mut response = html_response(state, html, *status, None, ctx.headers, ctx.method);
            add_rsc_vary(state, ctx.headers, &mut response);
            // Next names the page a data request matched (a data request
            // reaches a page without data only through the proxy's
            // normalization).
            if ctx.headers.contains_key("x-nextjs-data")
                && let Ok(value) = HeaderValue::from_str(&locale_key(path, ctx.locale))
            {
                response
                    .headers_mut()
                    .insert("x-nextjs-matched-path", value);
            }
            return Next::Done(response);
        }
        if !readonly && state.pages_router.ssg.contains(&route.page) {
            return Next::Done(method_not_allowed());
        }
        if ctx.method == Method::OPTIONS {
            // base server: an OPTIONS request to a page handler is invalid.
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::BAD_REQUEST;
            return Next::Done(response);
        }
    }
    let (Some(executor), Some(entry)) = (state.executor.as_deref(), crate::executable_entry(route))
    else {
        return Next::Done(crate::unsupported(
            "javascript-required",
            &format!(
                "Pages Router route {} runs Next's compiled handler; start with --js-runtime embedded-v8",
                route.page
            ),
        ));
    };
    // A dynamic data route marks the request as a data request.
    let marked;
    let headers = if data == Some(DataMatch::Dynamic) && !ctx.headers.contains_key("x-nextjs-data")
    {
        let mut map = ctx.headers.clone();
        map.insert("x-nextjs-data", HeaderValue::from_static("1"));
        marked = map;
        &marked
    } else {
        ctx.headers
    };
    let ctx = Ctx { headers, ..ctx };
    let policy = dynamic::AppPathPolicy {
        headers: ctx.initial,
        // API routes are not rendered by base server: no Vary.
        vary: if route.kind == RouteKind::PagesPage {
            rsc_vary(state, ctx.headers)
        } else {
            None
        },
        status: None,
        invocation,
    };
    #[cfg(feature = "negative-controls")]
    let wrong_uri;
    #[cfg(feature = "negative-controls")]
    let (entry, ctx, body) = {
        use crate::pages_controls as controls;
        let gssp =
            route.kind == RouteKind::PagesPage && !state.pages_router.ssg.contains(&route.page);
        if gssp && controls::skip_ssr() {
            let mut response =
                Response::new(Body::from("<!DOCTYPE html><html><body></body></html>"));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(HTML));
            return Next::Done(response);
        }
        if route.kind == RouteKind::PagesApi && controls::skip_api() {
            let mut response = Response::new(Body::from("{}"));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            return Next::Done(response);
        }
        if route.kind == RouteKind::PagesApi
            && ctx.method == Method::POST
            && controls::replay_post()
        {
            let first =
                dynamic::render(executor, entry, &policy, ctx.incoming(Body::empty())).await;
            let _ = axum::body::to_bytes(first.into_body(), usize::MAX).await;
        }
        let dynamic_route = matches!(route.matcher, crate::CompiledMatcher::Pattern(_));
        let ctx = match controls::params(ctx.uri, dynamic_route) {
            Some(uri) => {
                wrong_uri = uri;
                Ctx {
                    uri: &wrong_uri,
                    ..ctx
                }
            }
            None => ctx,
        };
        let swapped = if route.kind == RouteKind::PagesPage {
            controls::wrong_page(state, &route.page)
        } else {
            None
        };
        if gssp
            && let Some(shared) = controls::shared_ssr()
            && let Some((status, headers, bytes)) =
                shared.lock().unwrap().get(ctx.uri.path()).cloned()
        {
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            for (name, value) in headers {
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(&value),
                ) {
                    response.headers_mut().append(name, value);
                }
            }
            return Next::Done(response);
        }
        (
            swapped
                .map(std::borrow::Cow::Owned)
                .unwrap_or(std::borrow::Cow::Borrowed(entry)),
            ctx,
            body,
        )
    };
    #[cfg(feature = "negative-controls")]
    let entry: &std::path::Path = &entry;
    let response = dynamic::render(executor, entry, &policy, ctx.incoming(body)).await;
    #[cfg(feature = "negative-controls")]
    let response = match crate::pages_controls::shared_ssr() {
        Some(shared)
            if route.kind == RouteKind::PagesPage
                && !state.pages_router.ssg.contains(&route.page) =>
        {
            let (parts, body) = response.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX)
                .await
                .unwrap_or_default();
            let headers = parts
                .headers
                .iter()
                .filter_map(|(n, v)| Some((n.as_str().to_owned(), v.to_str().ok()?.to_owned())))
                .collect();
            shared.lock().unwrap().insert(
                ctx.uri.path().to_owned(),
                (parts.status.as_u16(), headers, bytes.clone()),
            );
            Response::from_parts(parts, Body::from(bytes))
        }
        _ => response,
    };
    match marker(&response) {
        Some(NO_FALLBACK) => Next::NotFound,
        Some(RENDER_404) => {
            let keep = kept_headers(&response);
            match crate::render_not_found_for_handler(state, ctx, keep).await {
                Some(response) => Next::Done(dynamic::frame_for(ctx.version, response)),
                None => Next::Done(dynamic::frame_for(ctx.version, plain_not_found())),
            }
        }
        Some(HANDLER_EXCEPTION) if route.kind == RouteKind::PagesPage => {
            let keep = kept_headers(&response);
            match server_error(state, ctx, keep).await {
                Some(response) => Next::Done(dynamic::frame_for(ctx.version, response)),
                None => Next::Done(dynamic::frame_for(ctx.version, response)),
            }
        }
        _ => Next::Done(dynamic::frame_for(ctx.version, response)),
    }
}

/// base server's answer to a middleware prefetch of a page that is not
/// statically generated: an empty object, nothing rendered.
fn middleware_prefetch_skip(page: &str) -> Response {
    let mut response = Response::new(Body::from("{}"));
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(page) {
        headers.insert("x-matched-path", value);
    }
    headers.insert("x-middleware-skip", HeaderValue::from_static("1"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    response
}

/// What the handler writes when there is no router to render a 404 page.
fn plain_not_found() -> Response {
    let mut response = Response::new(Body::from("This page could not be found"));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

/// router-server's answer when routing found nothing for a data request
/// and the build has a proxy (`invokeRender` of `/404` with
/// `x-nextjs-data`): an empty JSON object naming the requested path.
pub(crate) fn data_not_found(pathname: &str) -> Response {
    let mut response = Response::new(Body::from("{}"));
    *response.status_mut() = StatusCode::NOT_FOUND;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    if let Ok(value) = HeaderValue::from_str(pathname) {
        headers.insert("x-nextjs-matched-path", value);
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}

/// base server's answer to a data request that routing matched but no page
/// handler did, in a build with a proxy: 200 and an empty JSON object.
pub(crate) fn data_unmatched(pathname: &str) -> Response {
    let mut response = Response::new(Body::from("{}"));
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(pathname) {
        headers.insert("x-nextjs-matched-path", value);
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::denormalize;

    #[test]
    fn data_paths_denormalize_as_next_does() {
        assert_eq!(denormalize("/index"), "/");
        assert_eq!(denormalize("/index/index"), "/index");
        assert_eq!(denormalize("/books/42"), "/books/42");
        assert_eq!(denormalize("/index/[slug]"), "/index/[slug]");
    }

    #[test]
    fn denormalize_keeps_unrelated_index_prefixes() {
        assert_eq!(denormalize("/indexes"), "/indexes");
    }
}
