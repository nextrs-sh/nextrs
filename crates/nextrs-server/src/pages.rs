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
    /// `pages/404.html`.
    pub not_found: Option<Cached>,
    /// `pages/500.html`.
    pub server_error: Option<Cached>,
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
            match prerender.pathname.as_str() {
                "/404" => state.not_found = Some(html.clone()),
                "/500" => state.server_error = Some(html.clone()),
                _ => {}
            }
            state
                .static_pages
                .insert(prerender.pathname.clone(), (status, html));
        }
        state.error_entry = info.pages_router.error_entry.clone();
        for data in &info.pages_router.data_routes {
            if data.dynamic {
                let regex = Regex::new(&data.regex).map_err(|error| StartError::RouteRegex {
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
    let page = denormalize(&format!("/{rest}"));
    let decoded =
        crate::routing::query::decode_uri_component(&page).unwrap_or_else(|| page.clone());
    [page, decoded]
        .into_iter()
        .find(|candidate| state.pages_router.data_static.contains(candidate))
}

/// `/_next/data/<this build>/...` without the `.json` suffix, in a build
/// with a `server/pages` directory (next-server's `handleNextDataRequest`
/// renders the 404 page for it).

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
    let prefix = format!("/_next/data/{}", state.build_id);
    let pathname = path
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_suffix(".json"))
        .map(denormalize)
        .unwrap_or_else(|| path.to_owned());
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
    if let Some(page) = &state.pages_router.not_found {
        let mut response = html_response(
            state,
            page,
            StatusCode::NOT_FOUND,
            cache_control,
            ctx.headers,
            ctx.method,
        );
        merge(&mut response, keep);
        return Some(response);
    }
    let entry = state.pages_router.error_entry.as_deref()?;
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
        vary: None,
        status: Some(404),
        invocation: nextrs_runtime::Invocation::Handler,
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
    if let Some(page) = &state.pages_router.server_error {
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
        return Some(response);
    }
    let entry = state.pages_router.error_entry.as_deref()?;
    let executor = state.executor.as_deref()?;
    let mut initial = ctx.initial.to_vec();
    initial.push(("cache-control".to_owned(), NO_STORE.to_owned()));
    let policy = dynamic::AppPathPolicy {
        headers: &initial,
        vary: None,
        status: Some(500),
        invocation: nextrs_runtime::Invocation::Handler,
    };
    let response = dynamic::render(executor, entry, &policy, ctx.incoming(Body::empty())).await;
    Some(dynamic::frame_for(ctx.version, response))
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
        if let Some((status, html)) = state.pages_router.static_pages.get(path) {
            let status_page = path == "/404" || path == "/500";
            if !readonly && !status_page {
                return Next::Done(method_not_allowed());
            }
            let mut response = html_response(state, html, *status, None, ctx.headers, ctx.method);
            // Next names the page a data request matched (a data request
            // reaches a page without data only through the proxy's
            // normalization).
            if ctx.headers.contains_key("x-nextjs-data")
                && let Ok(value) = HeaderValue::from_str(path)
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
        vary: None,
        status: None,
        invocation,
    };
    let response = dynamic::render(executor, entry, &policy, ctx.incoming(body)).await;
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
