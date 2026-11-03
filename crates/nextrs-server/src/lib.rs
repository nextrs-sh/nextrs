//! HTTP server for official Next.js build output. Serves what `next build`
//! already produced (`/_next/static`, `public/`, prerendered HTML/RSC/segment
//! responses) from Rust. Dynamic App Router routes are rendered by the
//! embedded V8 executor when one is configured (`ServerOptions::executor`);
//! everything else that needs JavaScript gets an explicit
//! `501 Not Implemented`.

mod dynamic;
mod etag;
mod rsc;
mod static_files;

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use nextrs_adapter::{
    BuildInfo, Diagnostic, Matcher, Rendering, RouteKind, RoutingConfig, RscConfig, Runtime,
    Severity, StoredResponse,
};
use nextrs_runtime::Executor;
use regex::Regex;

use crate::rsc::RouterHeaders;

/// Header value Next.js uses for prerendered responses that never revalidate.
const CACHE_FOREVER: &str = "s-maxage=31536000";
const NO_STORE: &str = "private, no-cache, no-store, max-age=0, must-revalidate";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const PUBLIC_FILE: &str = "public, max-age=0";
const HTML: &str = "text/html; charset=utf-8";
const UNSUPPORTED_HEADER: &str = "x-nextrs-unsupported";
/// Headers in stored prerenders that `next start` does not send to clients.
const INTERNAL_HEADERS: &[&str] = &["x-next-cache-tags"];

#[derive(Debug, Clone, Default)]
pub struct ServerOptions {
    /// Start even when the build uses features NextRS cannot serve.
    pub allow_unsupported: bool,
    /// Renders dynamic App Router routes. Without it they get a 501.
    pub executor: Option<Arc<Executor>>,
}

#[derive(Debug)]
pub enum StartError {
    Unsupported(Vec<Diagnostic>),
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    RouteRegex {
        page: String,
        error: regex::Error,
    },
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(diagnostics) => {
                writeln!(
                    f,
                    "the build uses features NextRS cannot serve correctly yet:"
                )?;
                for d in diagnostics {
                    writeln!(f, "  - [{}] {}", d.code, d.message)?;
                }
                write!(f, "pass --allow-unsupported to start anyway")
            }
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            Self::RouteRegex { page, error } => write!(f, "route {page}: {error}"),
        }
    }
}

impl std::error::Error for StartError {}

/// Builds the axum application for one Next.js build.
pub async fn app(info: &BuildInfo, options: &ServerOptions) -> Result<Router, StartError> {
    let unsupported: Vec<Diagnostic> = info
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Unsupported)
        .cloned()
        .collect();
    if !unsupported.is_empty() && !options.allow_unsupported {
        return Err(StartError::Unsupported(unsupported));
    }
    let state = AppState::load(info).await?;
    let state = AppState {
        executor: options.executor.clone(),
        ..state
    };
    Ok(Router::new().fallback(handle).with_state(Arc::new(state)))
}

struct AppState {
    config: RoutingConfig,
    rsc: RscConfig,
    static_root: Option<PathBuf>,
    public_root: Option<PathBuf>,
    pages: HashMap<String, Page>,
    not_found: Option<Cached>,
    routes: Vec<CompiledRoute>,
    executor: Option<Arc<Executor>>,
    /// `(.)`-style segments: responses then vary on `Next-URL`, which the
    /// executor path does not implement.
    has_interception_routes: bool,
}

struct Page {
    status: StatusCode,
    html: Option<Cached>,
    rsc: Option<Cached>,
    segments: HashMap<String, Cached>,
}

/// A stored response, read once at startup. Phase 0 only serves builds
/// without ISR, so these never change while the server runs.
#[derive(Clone)]
struct Cached {
    body: Bytes,
    headers: Vec<(HeaderName, HeaderValue)>,
    etag: Option<HeaderValue>,
}

struct CompiledRoute {
    page: String,
    kind: RouteKind,
    rendering: Rendering,
    runtime: Runtime,
    entry: Option<PathBuf>,
    internal: bool,
    matcher: CompiledMatcher,
}

enum CompiledMatcher {
    Exact(String),
    Pattern(Regex),
}

impl CompiledRoute {
    fn matches(&self, path: &str) -> bool {
        match &self.matcher {
            CompiledMatcher::Exact(pathname) => pathname == path,
            CompiledMatcher::Pattern(regex) => regex.is_match(path),
        }
    }
}

impl AppState {
    async fn load(info: &BuildInfo) -> Result<Self, StartError> {
        let mut pages = HashMap::new();
        let mut not_found = None;
        for prerender in &info.prerenders {
            // Phase 0 does not regenerate, so revalidating pages are skipped
            // rather than served stale forever.
            if prerender.revalidate.is_some() {
                continue;
            }
            let html = load_cached(prerender.html.as_ref(), true).await?;
            if prerender.pathname == "/_not-found" {
                not_found = html;
                continue;
            }
            let mut segments = HashMap::new();
            for (path, stored) in &prerender.segments {
                if let Some(cached) = load_cached(Some(stored), false).await? {
                    segments.insert(path.clone(), cached);
                }
            }
            // next start serves the global error shell with status 500
            // although its prerender is stored as 200.
            let status = if prerender.pathname == "/_global-error" {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::from_u16(prerender.status).unwrap_or(StatusCode::OK)
            };
            pages.insert(
                prerender.pathname.clone(),
                Page {
                    status,
                    html,
                    rsc: load_cached(prerender.rsc.as_ref(), false).await?,
                    segments,
                },
            );
        }

        let mut routes = Vec::new();
        for route in &info.routes {
            let matcher = match &route.matcher {
                Matcher::Exact { pathname } => CompiledMatcher::Exact(pathname.clone()),
                Matcher::Pattern { regex, .. } => {
                    CompiledMatcher::Pattern(Regex::new(regex).map_err(|error| {
                        StartError::RouteRegex {
                            page: route.page.clone(),
                            error,
                        }
                    })?)
                }
            };
            routes.push(CompiledRoute {
                page: route.page.clone(),
                kind: route.kind,
                rendering: route.rendering.clone(),
                runtime: route.runtime,
                entry: route.entry.clone(),
                internal: route.internal,
                matcher,
            });
        }

        Ok(Self {
            config: info.config.clone(),
            rsc: info.rsc.clone(),
            static_root: canonical_dir(&info.static_dir).await,
            public_root: match &info.public_dir {
                Some(dir) => canonical_dir(dir).await,
                None => None,
            },
            pages,
            not_found,
            has_interception_routes: info.routes.iter().any(|route| route.page.contains("/(.")),
            routes,
            executor: None,
        })
    }
}

async fn canonical_dir(dir: &Path) -> Option<PathBuf> {
    tokio::fs::canonicalize(dir).await.ok()
}

async fn load_cached(
    stored: Option<&StoredResponse>,
    with_etag: bool,
) -> Result<Option<Cached>, StartError> {
    let Some(stored) = stored else {
        return Ok(None);
    };
    let body = tokio::fs::read(&stored.file)
        .await
        .map_err(|source| StartError::Io {
            path: stored.file.clone(),
            source,
        })?;
    let headers = stored
        .headers
        .iter()
        .filter(|(name, _)| !INTERNAL_HEADERS.contains(&name.as_str()))
        .filter_map(|(name, value)| {
            Some((
                HeaderName::from_bytes(name.as_bytes()).ok()?,
                HeaderValue::from_str(value).ok()?,
            ))
        })
        .collect();
    let etag = with_etag
        .then(|| HeaderValue::from_str(&etag::generate_etag(&String::from_utf8_lossy(&body))).ok())
        .flatten();
    Ok(Some(Cached {
        body: Bytes::from(body),
        headers,
        etag,
    }))
}

async fn handle(
    State(state): State<Arc<AppState>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let path = normalize_dot_segments(uri.path());
    let path = path.as_ref();
    let query = uri.query();
    let readonly = method == Method::GET || method == Method::HEAD;

    // routes-manifest always carries this redirect when trailingSlash is false.
    if !state.config.trailing_slash && path.len() > 1 && path.ends_with('/') {
        let trimmed = path.trim_end_matches('/');
        let target = match query {
            Some(q) => format!("{}?{q}", if trimmed.is_empty() { "/" } else { trimmed }),
            None => if trimmed.is_empty() { "/" } else { trimmed }.to_owned(),
        };
        return redirect(StatusCode::PERMANENT_REDIRECT, &target, true);
    }

    if let Some(rest) = path.strip_prefix("/_next/static/") {
        if !readonly {
            return method_not_allowed(&state);
        }
        return match &state.static_root {
            Some(root) => serve_file(root, rest, IMMUTABLE, &method, &headers).await,
            None => not_found_plain(),
        };
    }
    if path == "/_next/image" {
        return unsupported(
            "image-optimizer",
            "/_next/image (image optimization) is not implemented in NextRS Phase 0",
        );
    }

    if let Some(page) = state.pages.get(path) {
        if !readonly {
            if headers.contains_key("next-action") {
                return unsupported(
                    "server-actions",
                    "Server Actions require JavaScript execution, which NextRS Phase 0 does not implement",
                );
            }
            return method_not_allowed(&state);
        }
        if headers.get(&state.rsc.header).is_some_and(|v| v == "1") {
            return serve_rsc(&state, page, path, query, &headers, &method);
        }
        if let Some(html) = &page.html {
            return prerendered(
                &state,
                html,
                page.status,
                CACHE_FOREVER,
                HTML,
                true,
                &headers,
                &method,
            );
        }
    }

    if readonly
        && let Some(root) = &state.public_root
        && let Some(file) = static_files::resolve(root, path.trim_start_matches('/')).await
    {
        return file_response(&file, PUBLIC_FILE, &method, &headers).await;
    }

    // Framework routes (`/_not-found`) are not addressable; they fall
    // through to the not-found page like any unknown path.
    if let Some(route) = state
        .routes
        .iter()
        .find(|route| !route.internal && route.matches(path))
    {
        if let (Some(executor), Some(entry)) = (&state.executor, executable_entry(route)) {
            if state.has_interception_routes {
                return unsupported(
                    "interception-routes",
                    "builds with interception routes are not supported by the NextRS executor",
                );
            }
            if headers.contains_key("next-action") {
                return unsupported(
                    "server-actions",
                    "Server Actions are not implemented by the NextRS executor",
                );
            }
            if route.kind == RouteKind::AppPage
                && headers.get(&state.rsc.header).is_some_and(|v| v == "1")
                && let Some(redirect) = rsc_redirect(&state, path, query, &headers)
            {
                return redirect;
            }
            let policy = dynamic::AppPathPolicy {
                vary: &state.rsc.vary_header,
            };
            return dynamic::render(executor, entry, &policy, &method, &uri, &headers, body).await;
        }
        let reason = match &route.rendering {
            Rendering::Static => "is prerendered but its stored response is missing",
            Rendering::Revalidated { .. } => "uses ISR, which needs JavaScript to regenerate",
            Rendering::Dynamic => "is rendered on demand by the Next.js server bundle",
            Rendering::Unknown { .. } => "could not be classified from build metadata",
        };
        return unsupported(
            "javascript-required",
            &format!(
                "route {} {reason}; executing it requires JavaScript (start with --js-runtime embedded-v8)",
                route.page
            ),
        );
    }

    // next start marks the not-found shell cacheable only when it is
    // requested by its own path.
    let cache_control = if path == "/_not-found" {
        CACHE_FOREVER
    } else {
        NO_STORE
    };
    match &state.not_found {
        Some(page) => prerendered(
            &state,
            page,
            StatusCode::NOT_FOUND,
            cache_control,
            HTML,
            true,
            &headers,
            &method,
        ),
        None => not_found_plain(),
    }
}

/// Dynamic routes the executor can run: App Router pages and route handlers
/// compiled for the Node.js runtime.
fn executable_entry(route: &CompiledRoute) -> Option<&Path> {
    let kind_ok = matches!(route.kind, RouteKind::AppPage | RouteKind::AppRoute);
    let dynamic = route.rendering == Rendering::Dynamic;
    (kind_ok && dynamic && route.runtime == Runtime::Nodejs)
        .then_some(route.entry.as_deref())
        .flatten()
}

/// next start validates the `_rsc` cache-busting parameter before any App
/// Router render, dynamic ones included.
fn rsc_redirect(
    state: &AppState,
    path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
) -> Option<Response> {
    let router = RouterHeaders::from_headers(
        headers,
        &state.rsc.prefetch_header,
        &state.rsc.segment_prefetch_header,
    );
    if router.matches(rsc::query_hash(query).as_deref()) {
        return None;
    }
    let target = rsc::redirect_target(path, query, &router.expected_hash());
    Some(redirect(StatusCode::TEMPORARY_REDIRECT, &target, false))
}

fn serve_rsc(
    state: &AppState,
    page: &Page,
    path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    method: &Method,
) -> Response {
    let router = RouterHeaders::from_headers(
        headers,
        &state.rsc.prefetch_header,
        &state.rsc.segment_prefetch_header,
    );
    let actual = rsc::query_hash(query);
    if !router.matches(actual.as_deref()) {
        let target = rsc::redirect_target(path, query, &router.expected_hash());
        return redirect(StatusCode::TEMPORARY_REDIRECT, &target, false);
    }
    let content_type = state.rsc.content_type.as_str();
    let stored = match router.segment_prefetch {
        Some(segment) => page.segments.get(segment),
        None => page.rsc.as_ref(),
    };
    match stored {
        Some(cached) => prerendered(
            state,
            cached,
            page.status,
            CACHE_FOREVER,
            content_type,
            false,
            headers,
            method,
        ),
        None => unsupported(
            "javascript-required",
            "this RSC payload was not prerendered; rendering it requires JavaScript",
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn prerendered(
    state: &AppState,
    cached: &Cached,
    status: StatusCode,
    cache_control: &'static str,
    content_type: &str,
    is_html: bool,
    request: &HeaderMap,
    method: &Method,
) -> Response {
    let mut response = Response::new(Body::empty());
    let headers = response.headers_mut();
    if let Ok(vary) = HeaderValue::from_str(&state.rsc.vary_header) {
        headers.insert(header::VARY, vary);
    }
    headers.insert("x-nextjs-cache", HeaderValue::from_static("HIT"));
    for (name, value) in &cached.headers {
        headers.append(name.clone(), value.clone());
    }
    if is_html && state.config.powered_by_header {
        headers.insert("x-powered-by", HeaderValue::from_static("Next.js"));
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    let etag = cached.etag.as_ref().filter(|_| state.config.generate_etags);
    if let Some(etag) = etag {
        headers.insert(header::ETAG, etag.clone());
        let fresh = request
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|tags| tags.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
        if fresh {
            *response.status_mut() = StatusCode::NOT_MODIFIED;
            return response;
        }
    }
    if let Ok(value) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(cached.body.len()));
    *response.status_mut() = status;
    if method != Method::HEAD {
        *response.body_mut() = Body::from(cached.body.clone());
    }
    response
}

async fn serve_file(
    root: &Path,
    rest: &str,
    cache_control: &'static str,
    method: &Method,
    request: &HeaderMap,
) -> Response {
    match static_files::resolve(root, rest).await {
        Some(file) => file_response(&file, cache_control, method, request).await,
        None => not_found_plain(),
    }
}

/// Mirrors the `send` package used by `next start`: weak ETag
/// `W/"<size hex>-<mtime ms hex>"`, Last-Modified, and 304 revalidation.
/// Range requests are not implemented, so `Accept-Ranges` is not advertised.
async fn file_response(
    file: &Path,
    cache_control: &'static str,
    method: &Method,
    request: &HeaderMap,
) -> Response {
    let Ok(metadata) = tokio::fs::metadata(file).await else {
        return not_found_plain();
    };
    let modified = metadata.modified().ok();
    let etag = modified
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| {
            // Node builds `stat.mtime` with Math.round(mtimeMs).
            let millis = d.as_secs() * 1000 + (u64::from(d.subsec_nanos()) + 500_000) / 1_000_000;
            HeaderValue::from_str(&format!("W/\"{:x}-{millis:x}\"", metadata.len())).ok()
        });
    let last_modified =
        modified.and_then(|m| HeaderValue::from_str(&httpdate::fmt_http_date(m)).ok());

    let mut response = Response::new(Body::empty());
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    if let Some(etag) = &etag {
        headers.insert(header::ETAG, etag.clone());
    }
    if let Some(last_modified) = &last_modified {
        headers.insert(header::LAST_MODIFIED, last_modified.clone());
    }
    if is_fresh(request, etag.as_ref(), modified) {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        return response;
    }
    let Ok(bytes) = tokio::fs::read(file).await else {
        return not_found_plain();
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(static_files::content_type(file)),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));
    if method != Method::HEAD {
        *response.body_mut() = Body::from(bytes);
    }
    response
}

fn is_fresh(
    request: &HeaderMap,
    etag: Option<&HeaderValue>,
    modified: Option<std::time::SystemTime>,
) -> bool {
    if let Some(tags) = request
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        return etag
            .is_some_and(|etag| tags.split(',').any(|t| t.trim() == "*" || t.trim() == etag));
    }
    let since = request
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| httpdate::parse_http_date(v).ok());
    match (since, modified) {
        // HTTP dates have second precision.
        (Some(since), Some(modified)) => modified
            .duration_since(UNIX_EPOCH)
            .ok()
            .zip(since.duration_since(UNIX_EPOCH).ok())
            .is_some_and(|(m, s)| m.as_secs() <= s.as_secs()),
        _ => false,
    }
}

/// The trailing-slash redirect (308) carries the target as body and a
/// `Refresh` header; the RSC cache-busting redirect (307) is empty.
fn redirect(status: StatusCode, target: &str, with_refresh: bool) -> Response {
    let Ok(location) = HeaderValue::from_str(target) else {
        return (StatusCode::BAD_REQUEST, "Bad Request").into_response();
    };
    let body = if with_refresh {
        target.to_owned()
    } else {
        String::new()
    };
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    if with_refresh && let Ok(refresh) = HeaderValue::from_str(&format!("0;url={target}")) {
        headers.insert(header::REFRESH, refresh);
    }
    headers.insert(header::LOCATION, location);
    headers.remove(header::CONTENT_TYPE);
    response
}

fn method_not_allowed(state: &AppState) -> Response {
    let mut response = Response::new(Body::from("Method Not Allowed"));
    *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
    let headers = response.headers_mut();
    headers.insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
    if let Ok(vary) = HeaderValue::from_str(&state.rsc.vary_header) {
        headers.insert(header::VARY, vary);
    }
    response
}

fn not_found_plain() -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, NO_STORE),
        ],
        "Not Found",
    )
        .into_response()
}

/// WHATWG URL dot-segment removal (`.`, `..` and their `%2e` spellings), which
/// `next start` applies via `new URL()` before routing. Static file
/// resolution still rejects dot segments on its own.
fn normalize_dot_segments(path: &str) -> Cow<'_, str> {
    let single = |s: &str| s == "." || s.eq_ignore_ascii_case("%2e");
    let double = |s: &str| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            ".." | ".%2e" | "%2e." | "%2e%2e"
        )
    };
    if !path.split('/').any(|s| single(s) || double(s)) {
        return Cow::Borrowed(path);
    }
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let last = segments.len().saturating_sub(1);
    let mut out: Vec<&str> = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        if double(segment) {
            out.pop();
        } else if !single(segment) {
            out.push(segment);
            continue;
        }
        if i == last {
            out.push("");
        }
    }
    Cow::Owned(format!("/{}", out.join("/")))
}

/// Explicit refusal instead of a wrong answer.
fn unsupported(code: &'static str, message: &str) -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (HeaderName::from_static(UNSUPPORTED_HEADER), code),
        ],
        format!("NextRS: {message}\n"),
    )
        .into_response()
}
