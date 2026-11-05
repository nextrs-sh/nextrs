//! HTTP server for official Next.js build output. Serves what `next build`
//! already produced (`/_next/static`, `public/`, prerendered HTML/RSC/segment
//! responses) from Rust. Dynamic App Router routes are rendered by the
//! embedded V8 executor when one is configured (`ServerOptions::executor`);
//! everything else that needs JavaScript gets an explicit
//! `501 Not Implemented`.

mod dynamic;
mod etag;
mod rsc;
mod serve;
mod static_files;

pub use crate::dynamic::DEFAULT_MAX_REQUEST_BODY;
pub use crate::serve::{ClientAddr, ServeOptions, serve};

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
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
/// Build features that are unsupported without JavaScript but that the
/// embedded executor runs.
const EXECUTOR_FEATURES: &[&str] = &["server-actions"];
/// Headers in stored prerenders that `next start` does not send to clients.
const INTERNAL_HEADERS: &[&str] = &["x-next-cache-tags"];

#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// Start even when the build uses features NextRS cannot serve.
    pub allow_unsupported: bool,
    /// Renders dynamic App Router routes. Without it they get a 501.
    pub executor: Option<Arc<Executor>>,
    /// Request bodies for dynamic routes above this many bytes get a 413,
    /// whether they declare a length or are chunked.
    pub max_request_body: u64,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            allow_unsupported: false,
            executor: None,
            max_request_body: DEFAULT_MAX_REQUEST_BODY,
        }
    }
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
    // Server Actions run in the executor; without one they stay a refusal.
    let unsupported: Vec<Diagnostic> = unsupported
        .into_iter()
        .filter(|d| !(options.executor.is_some() && EXECUTOR_FEATURES.contains(&d.code)))
        .collect();
    if !unsupported.is_empty() && !options.allow_unsupported {
        return Err(StartError::Unsupported(unsupported));
    }
    let state = AppState::load(info).await?;
    let state = Arc::new(AppState {
        executor: options.executor.clone(),
        max_request_body: options.max_request_body,
        ..state
    });
    if let Some(executor) = &options.executor {
        // Weak: the executor must not keep the app (and itself) alive.
        executor.set_internal_dispatch(Arc::new(SelfDispatch(Arc::downgrade(&state))));
    }
    Ok(Router::new().fallback(handle).with_state(state))
}

/// Serves Next.js's requests to its own server (Server Action redirects
/// and forwarding; see nextrs_runtime::InternalDispatch) through the same
/// routing as a client request, without a socket.
struct SelfDispatch(std::sync::Weak<AppState>);

impl nextrs_runtime::InternalDispatch for SelfDispatch {
    fn dispatch(
        &self,
        request: nextrs_runtime::InternalRequest,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = nextrs_runtime::InternalResponse> + Send + 'static>,
    > {
        let state = self.0.upgrade();
        Box::pin(async move {
            let Some(state) = state else {
                return internal_failure(StatusCode::SERVICE_UNAVAILABLE);
            };
            let mut builder = Request::builder()
                .method(request.method.as_str())
                .uri(request.path_and_query.as_str());
            for (name, value) in &request.headers {
                let bytes: Vec<u8> = value.chars().map(|c| c as u32 as u8).collect();
                if let (Ok(name), Ok(value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_bytes(&bytes),
                ) {
                    builder = builder.header(name, value);
                }
            }
            let body = match request.body {
                nextrs_runtime::InternalBody::Empty => Body::empty(),
                nextrs_runtime::InternalBody::Bytes(bytes) => Body::from(bytes),
                nextrs_runtime::InternalBody::Stream(rx) => {
                    Body::from_stream(futures_util::stream::unfold(rx, |mut rx| async move {
                        rx.recv()
                            .await
                            .map(|chunk| (Ok::<_, std::io::Error>(chunk), rx))
                    }))
                }
            };
            let Ok(request) = builder.body(body) else {
                return internal_failure(StatusCode::BAD_REQUEST);
            };
            let response = handle(State(state), request).await;
            let (parts, body) = response.into_parts();
            use futures_util::StreamExt;
            nextrs_runtime::InternalResponse {
                status: parts.status.as_u16(),
                headers: parts
                    .headers
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.as_str().to_owned(),
                            value.as_bytes().iter().map(|&b| b as char).collect(),
                        )
                    })
                    .collect(),
                body: body
                    .into_data_stream()
                    .map(|item| item.map_err(|error| error.to_string()))
                    .boxed(),
            }
        })
    }
}

fn internal_failure(status: StatusCode) -> nextrs_runtime::InternalResponse {
    nextrs_runtime::InternalResponse {
        status: status.as_u16(),
        headers: Vec::new(),
        body: Box::pin(futures_util::stream::empty()),
    }
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
    max_request_body: u64,
    /// `(.)`-style segments: responses then vary on `Next-URL`, which the
    /// executor path does not implement.
    has_interception_routes: bool,
}

struct Page {
    status: StatusCode,
    html: Option<Cached>,
    rsc: Option<Cached>,
    segments: HashMap<String, Cached>,
    /// The cache tags Next stored with the prerender (`x-next-cache-tags`:
    /// its implicit path tags and the tags of fetches it made).
    tags: Vec<String>,
    /// When the prerender was written (Unix ms): a revalidation of one of
    /// its tags after this makes it stale.
    written_ms: u64,
}

impl Page {
    /// A Server Action (or route handler) revalidated this page's path or
    /// one of its tags since it was prerendered. next start would render it
    /// again; serving the stored copy would be stale.
    fn is_revalidated(&self, executor: Option<&Executor>) -> bool {
        !self.tags.is_empty()
            && executor.is_some_and(|e| e.tags_revalidated_since(&self.tags, self.written_ms))
    }
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
            let stored = prerender.html.as_ref().or(prerender.rsc.as_ref());
            let tags = stored
                .and_then(|s| {
                    s.headers
                        .iter()
                        .find(|(name, _)| name == "x-next-cache-tags")
                })
                .map(|(_, value)| {
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let written_ms = match stored {
                Some(stored) => tokio::fs::metadata(&stored.file)
                    .await
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_millis() as u64),
                None => 0,
            };
            pages.insert(
                prerender.pathname.clone(),
                Page {
                    status,
                    html,
                    rsc: load_cached(prerender.rsc.as_ref(), false).await?,
                    segments,
                    tags,
                    written_ms,
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
            max_request_body: DEFAULT_MAX_REQUEST_BODY,
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

async fn handle(State(state): State<Arc<AppState>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let client = parts.extensions.get::<ClientAddr>().map(|c| c.0);
    let axum::http::request::Parts {
        method,
        uri,
        version,
        headers,
        ..
    } = parts;
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
        // Revalidated since the build: render it on demand with Next's
        // handler instead of serving the stale prerender (NextRS does not
        // store regenerated pages yet).
        if readonly
            && page.is_revalidated(state.executor.as_deref())
            && let (Some(executor), Some(entry)) = (&state.executor, state.action_entry(path))
        {
            if headers.get(&state.rsc.header).is_some_and(|v| v == "1")
                && let Some(redirect) = rsc_redirect(&state, path, query, &headers)
            {
                return redirect;
            }
            let incoming = dynamic::Incoming {
                method: &method,
                uri: &uri,
                headers: &headers,
                body,
                client,
                max_body: state.max_request_body,
            };
            return execute(&state, executor, entry, incoming, version).await;
        }
        if !readonly {
            // next start hands a POST that may carry a Server Action to the
            // page's compiled handler even when the page is prerendered;
            // Next's action handler decides whether it is one.
            if is_possible_server_action(&method, &headers) {
                if let Some(executor) = &state.executor
                    && let Some(entry) = state.action_entry(path)
                {
                    let incoming = dynamic::Incoming {
                        method: &method,
                        uri: &uri,
                        headers: &headers,
                        body,
                        client,
                        max_body: state.max_request_body,
                    };
                    return execute(&state, executor, entry, incoming, version).await;
                }
                if headers.contains_key("next-action") || state.executor.is_none() {
                    return unsupported(
                        "server-actions",
                        "Server Actions require JavaScript execution (start with --js-runtime embedded-v8)",
                    );
                }
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
            if route.kind == RouteKind::AppPage
                && headers.get(&state.rsc.header).is_some_and(|v| v == "1")
                && let Some(redirect) = rsc_redirect(&state, path, query, &headers)
            {
                return redirect;
            }
            let incoming = dynamic::Incoming {
                method: &method,
                uri: &uri,
                headers: &headers,
                body,
                client,
                max_body: state.max_request_body,
            };
            return execute(&state, executor, entry, incoming, version).await;
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

    // next start renders an unmatched possible-action POST with the
    // not-found page's handler, whose action handler answers it (an
    // unknown action is 409, a known one is forwarded to its page).
    if is_possible_server_action(&method, &headers)
        && let Some(executor) = &state.executor
        && !state.has_interception_routes
        && let Some(entry) = state
            .routes
            .iter()
            .find(|route| route.internal && route.page == "/_not-found")
            .and_then(|route| route.entry.as_deref())
    {
        let incoming = dynamic::Incoming {
            method: &method,
            uri: &uri,
            headers: &headers,
            body,
            client,
            max_body: state.max_request_body,
        };
        return execute_with_status(&state, executor, entry, incoming, version, Some(404)).await;
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

/// Executor metrics for a separate (loopback) listener: `GET /metrics` in
/// the Prometheus text format, `GET /metrics.json`, and `GET /healthz`
/// (200 while at least one isolate is ready, 503 otherwise).
pub fn metrics_app(executor: Arc<Executor>) -> Router {
    use axum::routing::get;
    let text = executor.clone();
    let json = executor.clone();
    Router::new()
        .route(
            "/metrics",
            get(move || async move {
                (
                    [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                    text.metrics().to_prometheus(),
                )
            }),
        )
        .route(
            "/metrics.json",
            get(move || async move { axum::Json(json.metrics()) }),
        )
        .route(
            "/healthz",
            get(move || async move {
                if executor.metrics().ready_executors() > 0 {
                    (StatusCode::OK, "ok\n")
                } else {
                    (StatusCode::SERVICE_UNAVAILABLE, "no ready isolate\n")
                }
            }),
        )
}

async fn execute(
    state: &AppState,
    executor: &Executor,
    entry: &Path,
    incoming: dynamic::Incoming<'_>,
    version: axum::http::Version,
) -> Response {
    execute_with_status(state, executor, entry, incoming, version, None).await
}

async fn execute_with_status(
    state: &AppState,
    executor: &Executor,
    entry: &Path,
    incoming: dynamic::Incoming<'_>,
    version: axum::http::Version,
    status: Option<u16>,
) -> Response {
    let policy = dynamic::AppPathPolicy {
        vary: &state.rsc.vary_header,
        status,
    };
    let response = dynamic::render(executor, entry, &policy, incoming).await;
    dynamic::frame_for(version, response)
}

/// Next's `getIsPossibleServerAction` (server-action-request-meta.ts): a POST
/// with a `Next-Action` header, or a multipart or URL-encoded form POST
/// (a progressively enhanced form submission). The comparisons are Next's:
/// exact for URL-encoded, a prefix for multipart.
fn is_possible_server_action(method: &Method, headers: &HeaderMap) -> bool {
    if method != Method::POST {
        return false;
    }
    if headers.contains_key("next-action") {
        return true;
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .map(HeaderValue::as_bytes)
        .unwrap_or_default();
    content_type.starts_with(b"multipart/form-data")
        || content_type == b"application/x-www-form-urlencoded"
}

impl AppState {
    /// The compiled page that handles a Server Action POST to `path`: the
    /// App Router page serving it, prerendered or dynamic, on the Node.js
    /// runtime. ISR pages are excluded until NextRS can regenerate them.
    fn action_entry(&self, path: &str) -> Option<&Path> {
        if self.has_interception_routes {
            return None;
        }
        let route = self
            .routes
            .iter()
            .find(|route| !route.internal && route.matches(path))?;
        let rendering_ok = matches!(route.rendering, Rendering::Static | Rendering::Dynamic);
        (route.kind == RouteKind::AppPage && rendering_ok && route.runtime == Runtime::Nodejs)
            .then_some(route.entry.as_deref())
            .flatten()
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
