//! HTTP server for official Next.js build output. Serves what `next build`
//! already produced (`/_next/static`, `public/`, prerendered HTML/RSC/segment
//! responses) from Rust. Dynamic App Router routes are rendered by the
//! embedded V8 executor when one is configured (`ServerOptions::executor`);
//! everything else that needs JavaScript gets an explicit
//! `501 Not Implemented`.

mod compress;
mod compress_cache;
mod dynamic;
mod etag;
mod external;
pub mod http_metrics;
mod i18n;
mod image;
mod pages;
#[cfg(feature = "negative-controls")]
mod pages_controls;
mod range;
pub mod reclaim;
mod resolve;
#[doc(hidden)]
pub mod routing;
mod rsc;
mod serve;
mod static_files;

pub use crate::compress_cache::CompressionCache;
pub use crate::dynamic::DEFAULT_MAX_REQUEST_BODY;
pub use crate::http_metrics::HttpMetrics;
pub use crate::image::{ImageCacheOptions, ImageFault, ImageFaults, ImageOptions};
pub use crate::reclaim::{ReclaimPolicy, ReclaimStats};
pub use crate::serve::{ClientAddr, ServeOptions, serve};

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
// Build features that are unsupported without JavaScript but that the
// embedded executor runs (ISR: Next's handler regenerates pages and keeps
// them in the shared cache).
use nextrs_adapter::EXECUTOR_FEATURES;
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

/// Which requests may carry `X-Forwarded-*` / `Forwarded` headers to Next
/// (which trusts `X-Forwarded-Host` and `-Proto` for Server Action origin
/// checks, redirects and absolute URLs).
#[derive(Debug, Clone, Default)]
pub enum ForwardedHeaders {
    /// Passed through from every client, as `next start` does: the reverse
    /// proxy in front must overwrite them.
    #[default]
    Pass,
    /// Kept only on connections from these addresses (the reverse
    /// proxies); removed from every other request before Next sees it.
    TrustedProxies(Vec<nextrs_runtime::AddrRange>),
    /// Removed from every request (no proxy in front).
    Strip,
}

/// Headers a client could use to claim another host, scheme, port or
/// address.
const FORWARDED_HEADERS: &[&str] = &[
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-forwarded-port",
    "x-forwarded-for",
    "x-forwarded-ssl",
    "x-real-ip",
    "forwarded",
];

impl ForwardedHeaders {
    /// Whether a request from `peer` keeps its forwarded headers. Requests
    /// Next sends to itself (internal dispatch) carry what it was given.
    fn trusts(&self, peer: Option<std::net::SocketAddr>, internal: bool) -> bool {
        match self {
            Self::Pass => true,
            Self::Strip => internal,
            Self::TrustedProxies(ranges) => {
                internal || peer.is_some_and(|peer| ranges.iter().any(|r| r.contains(peer.ip())))
            }
        }
    }
}

/// Marks a request Next made to its own server (see `SelfDispatch`).
#[derive(Clone, Copy)]
struct SelfDispatched;

/// Marks the request for an image's source (`fetchInternalImage`): the
/// image endpoint does not answer it, so a rewrite cannot loop it back.
#[derive(Clone, Copy)]
struct ImageSubrequest;

#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// Start even when the build uses features NextRS cannot serve.
    pub allow_unsupported: bool,
    /// Renders dynamic App Router routes. Without it they get a 501.
    pub executor: Option<Arc<Executor>>,
    /// Request bodies for dynamic routes above this many bytes get a 413,
    /// whether they declare a length or are chunked.
    pub max_request_body: u64,
    /// Forwarded-header policy (reverse proxies).
    pub forwarded: ForwardedHeaders,
    /// The address the server listens on. The proxy sees request URLs on
    /// `localhost:<port>` and its redirects are made relative against
    /// `<host>:<port>`, as next start does with its hostname and port.
    pub listen: SocketAddr,
    /// `/_next/image` (transformations, remote sources, cache).
    pub images: ImageOptions,
    /// Gzip copies of immutable responses (`/_next/static` files and
    /// prerendered pages), shared with the metrics endpoint.
    pub compression_cache: Arc<CompressionCache>,
    /// Request counters and serving-stage timings, shared with the
    /// listener (`ServeOptions::metrics`) and the metrics endpoint.
    pub metrics: Arc<HttpMetrics>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            allow_unsupported: false,
            executor: None,
            max_request_body: DEFAULT_MAX_REQUEST_BODY,
            forwarded: ForwardedHeaders::Pass,
            listen: SocketAddr::from(([127, 0, 0, 1], 3000)),
            images: ImageOptions::default(),
            compression_cache: Arc::new(CompressionCache::default()),
            metrics: HttpMetrics::new(),
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
    /// A routing rule or proxy matcher NextRS cannot apply exactly. It is
    /// refused rather than skipped: skipping one could drop a security
    /// header or an access check.
    Routing {
        rule: String,
        error: String,
    },
    /// The image optimizer could not start (a pattern, the cache directory
    /// or the network client).
    Images(String),
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
            Self::Routing { rule, error } => write!(f, "routing rule {rule}: {error}"),
            Self::Images(error) => write!(f, "image optimizer: {error}"),
        }
    }
}

impl std::error::Error for StartError {}

/// Builds the axum application for one Next.js build.
pub async fn app(info: &BuildInfo, options: &ServerOptions) -> Result<Router, StartError> {
    app_with_stats(info, options)
        .await
        .map(|(router, _)| router)
}

/// Image optimizer counters and cache sizes, for `metrics_app`.
#[derive(Clone)]
pub struct ImageStats(Arc<image::Optimizer>);

impl ImageStats {
    pub fn snapshot(&self) -> Vec<(&'static str, u64)> {
        self.0.snapshot()
    }
}

/// [`app`], plus the image optimizer's statistics (when the build has an
/// image config).
pub async fn app_with_stats(
    info: &BuildInfo,
    options: &ServerOptions,
) -> Result<(Router, Option<ImageStats>), StartError> {
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
    let images = match &info.images {
        Some(images) => Some(Arc::new(
            image::Optimizer::new(
                images,
                &info.config.base_path,
                &info.build_id,
                &options.images,
            )
            .map_err(StartError::Images)?,
        )),
        None => None,
    };
    let state = Arc::new(AppState {
        executor: options.executor.clone(),
        max_request_body: options.max_request_body,
        forwarded: options.forwarded.clone(),
        listen: Listen::from(options.listen),
        images,
        compression_cache: options.compression_cache.clone(),
        metrics: options.metrics.clone(),
        ..state
    });
    if let Some(images) = &state.images {
        images.attach(Arc::downgrade(&state));
    }
    let stats = state.images.clone().map(ImageStats);
    warn_external_rewrites(info, options);
    if let Some(executor) = &options.executor {
        // Weak: the executor must not keep the app (and itself) alive.
        executor.set_internal_dispatch(Arc::new(SelfDispatch(Arc::downgrade(&state))));
    }
    Ok((Router::new().fallback(handle).with_state(state), stats))
}

/// Config rewrites to a fixed other origin that the network policy refuses
/// would answer 502 on every request: say so at startup. (Destinations
/// with parameters in the host are checked per request only.)
fn warn_external_rewrites(info: &BuildInfo, options: &ServerOptions) {
    let upstream = options.executor.as_ref().and_then(|e| e.upstream());
    for rule in info
        .routing
        .before_files
        .iter()
        .chain(&info.routing.after_files)
        .chain(&info.routing.fallback)
    {
        let Some(destination) = rule.destination.as_deref() else {
            continue;
        };
        if destination.starts_with('/') {
            continue;
        }
        let origin = destination
            .split_once("://")
            .map(|(scheme, rest)| (scheme, rest.split('/').next().unwrap_or(rest)));
        let Some((scheme, authority)) = origin else {
            continue;
        };
        if authority.contains(':')
            && authority
                .rsplit(':')
                .next()
                .is_some_and(|p| p.parse::<u16>().is_err())
        {
            continue;
        }
        let verdict = match (
            &upstream,
            url::Url::parse(&format!("{scheme}://{authority}/")),
        ) {
            (None, _) => Some("no network policy (--net-allow) is configured".to_owned()),
            (Some(upstream), Ok(url)) => upstream.check(&url).err().map(|d| d.to_string()),
            (Some(_), Err(_)) => None,
        };
        if let Some(reason) = verdict {
            tracing::warn!(
                source = %rule.source,
                destination,
                %reason,
                "external rewrite will answer 502: its destination is not allowed"
            );
        }
    }
}

/// The source of a local image (`fetchInternalImage`): a GET without
/// headers through the server's own routing, as next start sends it to its
/// router (public files and `/_next/static` without JavaScript; routes and
/// the proxy as for any request).
pub(crate) fn image_subrequest(
    state: Arc<AppState>,
    mut request: Request,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    request.extensions_mut().insert(SelfDispatched);
    request.extensions_mut().insert(ImageSubrequest);
    Box::pin(async move { handle(State(state), request).await })
}

/// Serves Next.js's requests to its own server (Server Action redirects
/// and forwarding; see nextrs_runtime::InternalDispatch) through the same
/// routing as a client request, without a socket.
struct SelfDispatch(std::sync::Weak<AppState>);

impl nextrs_runtime::InternalDispatch for SelfDispatch {
    /// The origin of the `initURL` requests get (`dynamic::render`):
    /// `http(s)://<listen host>:<port>`.
    fn serves(&self, url: &url::Url) -> bool {
        let Some(state) = self.0.upgrade() else {
            return false;
        };
        matches!(url.scheme(), "http" | "https")
            && url.host_str() == Some(state.listen.host.as_str())
            && url.port_or_known_default() == Some(state.listen.port)
    }

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
            let mut builder = builder.extension(SelfDispatched);
            if request.revalidation {
                builder = builder.extension(Revalidation);
            }
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
    /// Prerendered pages that never revalidate on a timer, served from
    /// memory until a revalidation touches one of their tags.
    pages: HashMap<String, Page>,
    not_found: Option<Cached>,
    routes: Vec<CompiledRoute>,
    executor: Option<Arc<Executor>>,
    max_request_body: u64,
    /// `experimental.proxyTimeout` for external rewrites.
    proxy_timeout: Option<std::time::Duration>,
    forwarded: ForwardedHeaders,
    /// `(.)`-style segments: responses then vary on `Next-URL`, which the
    /// executor path does not implement.
    has_interception_routes: bool,
    rules: CompiledRules,
    proxy: Option<ProxyRuntime>,
    /// A proxy or a `has`/`missing` rule reads the request's headers.
    needs_node_headers: bool,
    /// next.config `compress` (next start's router-server compresses
    /// unless it is `false`).
    compress: bool,
    compression_cache: Arc<CompressionCache>,
    /// Serving-stage timings (`HttpMetrics::stages`).
    metrics: Arc<HttpMetrics>,
    /// Files in `public/` (relative, decoded), listed at startup as next
    /// start's `publicFolderItems`.
    public_files: std::collections::HashSet<String>,
    listen: Listen,
    build_id: String,
    images: Option<Arc<image::Optimizer>>,
    pages_router: pages::PagesState,
    /// Next's i18n config (Pages Router locale routing).
    i18n: Option<i18n::I18n>,
}

/// next.config rules, compiled once at startup.
#[derive(Default)]
struct CompiledRules {
    headers: Vec<routing::Rule>,
    /// routes-manifest `onMatchHeaders`: header rules applied only when
    /// routing found an output (Next writes them for a deployment ID).
    on_match_headers: Vec<routing::Rule>,
    redirects: Vec<routing::Rule>,
    before_files: Vec<routing::Rule>,
    after_files: Vec<routing::Rule>,
    fallback: Vec<routing::Rule>,
}

struct ProxyRuntime {
    entry: PathBuf,
    matchers: routing::ProxyMatchers,
    /// `proxyClientMaxBodySize`.
    body_limit: u64,
}

/// next start's `hostname` (formatted) and `port`.
struct Listen {
    host: String,
    port: u16,
}

impl From<SocketAddr> for Listen {
    fn from(addr: SocketAddr) -> Self {
        // next start uses its hostname for both the proxy's request URL
        // and the base it relativizes proxy rewrites against. NextURL turns
        // loopback hosts into `localhost` in the proxy's URLs, so a
        // loopback bind is named `localhost` here: with `127.0.0.1` the two
        // origins would differ, and next start (bound with -H 127.0.0.1)
        // then proxies its own rewrites over HTTP to itself.
        let host = match addr.ip() {
            ip if ip.is_loopback() => "localhost".to_owned(),
            std::net::IpAddr::V6(ip) => format!("[{ip}]"),
            std::net::IpAddr::V4(ip) => ip.to_string(),
        };
        Self {
            host,
            port: addr.port(),
        }
    }
}

fn compile_rules(info: &BuildInfo) -> Result<CompiledRules, StartError> {
    use routing::RuleKind;
    let sensitive = info.routing.case_sensitive;
    let list = |kind: RuleKind,
                specs: &[nextrs_adapter::RuleSpec]|
     -> Result<Vec<routing::Rule>, StartError> {
        specs
            .iter()
            .map(|spec| {
                routing::Rule::compile(kind, spec, sensitive).map_err(|error| StartError::Routing {
                    rule: spec.source.clone(),
                    error,
                })
            })
            .collect()
    };
    Ok(CompiledRules {
        headers: list(RuleKind::Header, &info.routing.headers)?,
        // resolve-routes runs them through the same handleRoute as the
        // headers list (filesystem.js builds both with
        // buildCustomRoute('header')).
        on_match_headers: list(RuleKind::Header, &info.routing.on_match_headers)?,
        redirects: list(RuleKind::Redirect, &info.routing.redirects)?,
        before_files: list(RuleKind::BeforeFiles, &info.routing.before_files)?,
        after_files: list(RuleKind::AfterFiles, &info.routing.after_files)?,
        fallback: list(RuleKind::Fallback, &info.routing.fallback)?,
    })
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
    /// Prerendered from a dynamic route (`generateStaticParams`): next
    /// start's handler serves it from the cache for any method, where a
    /// static route's prerender answers other methods with 405.
    dynamic_route: bool,
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
    /// Names this body for the compression cache (unique per load).
    id: u64,
    body: Bytes,
    headers: Vec<(HeaderName, HeaderValue)>,
    etag: Option<HeaderValue>,
}

/// `Cached::id`s.
static NEXT_CACHED_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

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
            // Pages Router output: static pages are kept by PagesState;
            // getStaticProps pages are served by Next's handler from the
            // shared cache, which it seeds from these files.
            if prerender.pages {
                continue;
            }
            // ISR pages are served by Next's handler (from the shared cache,
            // seeded from these files), which decides when to regenerate.
            if prerender.revalidate.is_some() {
                continue;
            }
            // Partial Prerendering shells are completed per request by
            // Next's handler (it resumes the postponed state).
            if prerender.postponed {
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
                    dynamic_route: prerender.route.contains('['),
                },
            );
        }

        let mut routes = Vec::new();
        let mut ordered: Vec<&nextrs_adapter::Route> = info.routes.iter().collect();
        ordered.sort_by(|a, b| route_specificity(&a.page).cmp(&route_specificity(&b.page)));
        for route in ordered {
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

        let proxy = match &info.proxy {
            Some(proxy) => Some(ProxyRuntime {
                entry: proxy.entry.clone(),
                matchers: routing::ProxyMatchers::compile(&proxy.matchers).map_err(|error| {
                    StartError::Routing {
                        rule: "proxy matcher".into(),
                        error,
                    }
                })?,
                body_limit: proxy.client_max_body_size,
            }),
            None => None,
        };
        let rules = compile_rules(info)?;
        let public_root = match &info.public_dir {
            Some(dir) => canonical_dir(dir).await,
            None => None,
        };
        let public_files = match &public_root {
            Some(root) => list_files(root),
            None => Default::default(),
        };
        Ok(Self {
            compress: info.config.compress,
            compression_cache: Arc::new(CompressionCache::disabled()),
            metrics: HttpMetrics::new(),
            needs_node_headers: proxy.is_some()
                || [
                    &rules.headers,
                    &rules.on_match_headers,
                    &rules.redirects,
                    &rules.before_files,
                    &rules.after_files,
                    &rules.fallback,
                ]
                .iter()
                .any(|list| list.iter().any(routing::Rule::has_conditions)),
            rules,
            public_files,
            build_id: info.build_id.clone(),
            proxy,
            listen: Listen::from(SocketAddr::from(([127, 0, 0, 1], 3000))),
            config: info.config.clone(),
            i18n: info.config.i18n.clone().map(i18n::I18n::new),
            rsc: info.rsc.clone(),
            static_root: canonical_dir(&info.static_dir).await,
            public_root,
            pages,
            not_found,
            has_interception_routes: info.has_interception_routes(),
            routes,
            executor: None,
            max_request_body: DEFAULT_MAX_REQUEST_BODY,
            proxy_timeout: info
                .routing
                .proxy_timeout_ms
                .map(std::time::Duration::from_millis),
            forwarded: ForwardedHeaders::Pass,
            images: None,
            pages_router: pages::PagesState::load(info).await?,
        })
    }
}

/// Every regular file below `root`, as `/`-separated relative paths
/// (`recursiveReadDir` in next start; symlinks are followed by the
/// per-request resolution, which also re-checks containment).
fn list_files(root: &Path) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file()
                && let Ok(rel) = path.strip_prefix(root)
                && let Some(rel) = rel.to_str()
            {
                out.insert(rel.replace(std::path::MAIN_SEPARATOR, "/"));
            }
        }
    }
    out
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
        id: NEXT_CACHED_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        body: Bytes::from(body),
        headers,
        etag,
    }))
}

/// next-server's `revalidate()` request (`res.revalidate`): routed to its
/// page directly, as next start hands it to the page handler without
/// router-server (no next.config rules, no proxy). Set only by the
/// runtime's revalidation op, never from a client request.
#[derive(Clone, Copy)]
struct Revalidation;

async fn handle(State(state): State<Arc<AppState>>, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let client = parts.extensions.get::<ClientAddr>().map(|c| c.0);
    let internal = parts.extensions.get::<SelfDispatched>().is_some();
    let revalidation = internal && parts.extensions.get::<Revalidation>().is_some();
    let image_source = parts.extensions.get::<ImageSubrequest>().is_some();
    let axum::http::request::Parts {
        method,
        uri,
        version,
        mut headers,
        ..
    } = parts;
    if !state.forwarded.trusts(client, internal) {
        for name in FORWARDED_HEADERS {
            headers.remove(*name);
        }
    }
    // router-server deletes Next's internal headers from every request
    // (filterInternalHeaders), its own self-requests included: a client
    // must not be able to claim a proxy result or a matched path.
    for name in resolve::NEXT_INTERNAL_HEADERS {
        headers.remove(*name);
    }

    let started = std::time::Instant::now();
    let request = resolve::RouteRequest {
        method: &method,
        uri: &uri,
        version,
        headers: &headers,
        body,
        client,
    };
    let (outcome, resolved, body) = if revalidation {
        resolve::direct(&state, request).await
    } else {
        resolve::resolve(&state, request).await
    };
    // Next's requests to its own server (image sources, revalidations,
    // forwarded actions) are not client requests: not counted.
    if !internal {
        state.metrics.stages.routed(started.elapsed());
    }
    let via = via_of(&outcome);
    let res_headers = resolved.res_headers.clone();
    let response = serve_resolved(
        &state,
        outcome,
        &resolved,
        body,
        &method,
        &uri,
        &headers,
        version,
        client,
        image_source,
    )
    .await;
    if !internal {
        let via = if response.extensions().get::<dynamic::Executed>().is_some() {
            http_metrics::Via::Executor
        } else {
            via
        };
        state.metrics.stages.served(via, started.elapsed());
    }
    // HTTP/1.0 framing for every response, the proxy's direct responses
    // included (a body without a length then closes the connection).
    let response = dynamic::frame_for(version, with_routing_headers(response, &res_headers));
    // router-server's compression, for client requests only: Next's
    // requests to its own server (image sources, Server Action
    // forwarding) read the bytes themselves.
    if state.compress && !internal {
        compress::Negotiated::of(&method, version, &headers)
            .apply(response, &state.compression_cache)
            .await
    } else {
        response
    }
}

/// What answers a routed request when Next's handler does not (a response
/// it produced is counted as `Via::Executor` instead).
fn via_of(outcome: &resolve::Outcome) -> http_metrics::Via {
    use http_metrics::Via;
    use resolve::{Outcome, Output};
    match outcome {
        Outcome::Redirect { .. } => Via::Redirect,
        Outcome::NotFound => Via::NotFound,
        Outcome::Output(Output::Static(_)) => Via::Static,
        Outcome::Output(Output::Public(_)) => Via::Public,
        Outcome::Output(Output::Image) => Via::Image,
        Outcome::Output(Output::App { .. } | Output::EncodedApp) => Via::Prerendered,
        Outcome::ProxyFailed | Outcome::External(_) | Outcome::Respond(_) => Via::Other,
    }
}

/// Marks a response that must not get routing's response headers (config
/// redirects: resolve-routes returns `resHeaders: null` for them).
#[derive(Clone, Copy)]
struct NoRoutingHeaders;

/// router-server sets `resHeaders` on the response before the route
/// writes it; the route's own headers win.
fn with_routing_headers(mut response: Response, res_headers: &routing::query::JsMap) -> Response {
    if response.extensions().get::<NoRoutingHeaders>().is_some() || res_headers.is_empty() {
        return response;
    }
    let headers = response.headers_mut();
    for (name, value) in res_headers.iter() {
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        if headers.contains_key(&name) {
            continue;
        }
        for item in value.values() {
            if let Ok(value) = HeaderValue::from_bytes(&resolve::latin1_bytes(item)) {
                headers.append(name.clone(), value);
            }
        }
    }
    response
}

/// The executor invocation for a routed request: next start's request
/// meta after a rewrite or after the proxy set cookies.
fn invocation_for(
    state: &AppState,
    resolved: &resolve::Resolved,
    page: &str,
    item_path: &str,
) -> nextrs_runtime::Invocation {
    let rewrite = resolved.did_rewrite.then(|| nextrs_runtime::Rewrite {
        page: page.to_owned(),
        pathname: if state.config.base_path.is_empty() {
            item_path.to_owned()
        } else {
            resolve::strip_base_path(item_path, &state.config.base_path)
                .unwrap_or(item_path)
                .to_owned()
        },
        query_json: query_json(&resolved.query),
    });
    let locale = resolved.locale.as_ref().map(locale_meta);
    if rewrite.is_none() && resolved.middleware_cookies.is_empty() && locale.is_none() {
        return nextrs_runtime::Invocation::Handler;
    }
    nextrs_runtime::Invocation::Routed(nextrs_runtime::RouteMeta {
        middleware_cookies: resolved.middleware_cookies.clone(),
        rewrite,
        locale,
    })
}

/// Routing's locale meta, as the executor passes it to Next's handler.
pub(crate) fn locale_meta(locale: &resolve::RouteLocale) -> nextrs_runtime::LocaleMeta {
    nextrs_runtime::LocaleMeta {
        locale: locale.locale.clone(),
        default_locale: locale.default_locale.clone(),
        locale_domain: locale.locale_domain,
    }
}

/// A query object as JSON (JavaScript reads it back with its own key order).
fn query_json(query: &routing::query::JsMap) -> String {
    use routing::query::Value;
    let mut out = String::from("{");
    for (i, (key, value)) in query.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&serde_json_string(key));
        out.push(':');
        match value {
            Value::Str(s) => out.push_str(&serde_json_string(s)),
            Value::Arr(items) => {
                out.push('[');
                out.push_str(
                    &items
                        .iter()
                        .map(|s| serde_json_string(s))
                        .collect::<Vec<_>>()
                        .join(","),
                );
                out.push(']');
            }
        }
    }
    out.push('}');
    out
}

fn serde_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// next start's 500 for an exception in the proxy (or in routing).
pub(crate) fn proxy_failed() -> Response {
    let mut response = (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    response.extensions_mut().insert(NoRoutingHeaders);
    response
}

#[allow(clippy::too_many_arguments)]
async fn serve_resolved(
    state: &AppState,
    outcome: resolve::Outcome,
    resolved: &resolve::Resolved,
    body: Body,
    method: &Method,
    uri: &axum::http::Uri,
    original_headers: &HeaderMap,
    version: axum::http::Version,
    client: Option<SocketAddr>,
    image_source: bool,
) -> Response {
    use resolve::{Outcome, Output};
    let readonly = method == Method::GET || method == Method::HEAD;
    // The route sees the headers as the proxy left them.
    let routed_headers;
    let headers: &HeaderMap = if resolved.headers_modified {
        routed_headers = resolve::header_map(&resolved.headers);
        &routed_headers
    } else {
        original_headers
    };
    let request_path = normalize_dot_segments(uri.path()).into_owned();
    let query = uri.query();
    let initial: Vec<(String, String)> = resolve::pairs(&resolved.res_headers);

    let output = match outcome {
        Outcome::Redirect {
            status,
            location,
            with_res_headers,
        } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::TEMPORARY_REDIRECT);
            let mut response = redirect(
                status,
                &location,
                status == StatusCode::PERMANENT_REDIRECT,
                true,
            );
            if !with_res_headers {
                response.extensions_mut().insert(NoRoutingHeaders);
            }
            return response;
        }
        Outcome::ProxyFailed => return proxy_failed(),
        Outcome::External(url) => {
            // Config rewrites to another origin carry no routing headers
            // (resolve-routes returns `resHeaders: null`); a proxy rewrite
            // keeps the proxy's (router-server sets them before proxying,
            // and the upstream's own headers replace them).
            let upstream = state.executor.as_ref().and_then(|e| e.upstream());
            let mut response = external::forward(
                upstream,
                external::Forward {
                    url: &url,
                    method,
                    headers,
                    client_host: original_headers.get(header::HOST),
                    body,
                    max_body: state.max_request_body,
                    timeout: state.proxy_timeout,
                },
            )
            .await;
            if resolved.from_proxy {
                // router-server sets the proxy's cookies before proxying and
                // the upstream's are added to them, not put in their place.
                if let Some(cookies) = resolved.res_headers.get("set-cookie") {
                    let upstream: Vec<HeaderValue> = response
                        .headers()
                        .get_all(header::SET_COOKIE)
                        .iter()
                        .cloned()
                        .collect();
                    let out = response.headers_mut();
                    out.remove(header::SET_COOKIE);
                    for cookie in cookies.values() {
                        if let Ok(value) = HeaderValue::from_bytes(&resolve::latin1_bytes(cookie)) {
                            out.append(header::SET_COOKIE, value);
                        }
                    }
                    for value in upstream {
                        out.append(header::SET_COOKIE, value);
                    }
                }
            } else {
                response.extensions_mut().insert(NoRoutingHeaders);
            }
            return response;
        }
        Outcome::Respond(response) => return response,
        Outcome::NotFound => None,
        Outcome::Output(output) => Some(output),
    };
    let resolved_ref = resolved;
    let incoming = |body: Body| dynamic::Incoming {
        method,
        uri,
        headers,
        body,
        client,
        max_body: state.max_request_body,
        listen: &state.listen,
    };

    let Some(output) = output else {
        return not_found(
            state,
            method,
            headers,
            &request_path,
            &initial,
            incoming(body),
            version,
            resolved_ref,
        )
        .await;
    };
    match output {
        Output::Static(file) => {
            if !readonly {
                return method_not_allowed(state);
            }
            // Build output under /_next/static: its gzip copy may be cached
            // (keyed by the file's identity as stat'ed per request).
            file_response(state, &file, IMMUTABLE, method, headers, true).await
        }
        Output::EncodedApp => {
            // next start's page handler does not match the encoded path and
            // the not-found route answers instead: for a possible Server
            // Action or an RSC request that is the not-found handler's own
            // answer (as for any unmatched path), not the 404 document.
            // router-server's no-store header is not set: routing matched
            // the (decoded) file, so only the handler's own headers apply.
            if is_possible_server_action(method, headers)
                || rsc::is_rsc_request(headers, &state.rsc.header)
            {
                return not_found_render(
                    state,
                    method,
                    headers,
                    &request_path,
                    &initial,
                    incoming(body),
                    version,
                    resolved_ref,
                    false,
                )
                .await;
            }
            // base server: a data request no page handler matched, in a build
            // with a proxy, gets an empty JSON object naming the path.
            if state.proxy.is_some() && headers.contains_key("x-nextjs-data") {
                return pages::data_unmatched(&pages::matched_path(state, &resolved_ref.pathname));
            }
            let Some(page) = &state.not_found else {
                // A Pages Router build: next start's page handler renders
                // the 404 page, for the request's locale with i18n (no
                // router-server Cache-Control: routing matched the decoded
                // file).
                if state.pages_router_404() {
                    let ctx = pages::Ctx {
                        method,
                        uri,
                        headers,
                        client,
                        version,
                        initial: &[],
                        max_body: state.max_request_body,
                        listen: &state.listen,
                        locale: resolved_ref.locale.as_ref(),
                    };
                    if let Some(response) = pages::not_found(state, ctx, Vec::new(), None).await {
                        return response;
                    }
                }
                return not_found_plain();
            };
            prerendered(
                state,
                page,
                StatusCode::NOT_FOUND,
                CACHE_FOREVER,
                HTML,
                true,
                headers,
                method,
            )
        }
        Output::Image => {
            let optimizer = state.images.as_ref().filter(|o| !o.images().disabled());
            match optimizer {
                // A source request that routed back to the endpoint.
                Some(_) if image_source => image::loop_detected(),
                Some(optimizer) => optimizer.serve(method, headers, &resolved.query).await,
                // `images.unoptimized`, a custom loader, or no manifest:
                // next start renders its 404.
                None => {
                    not_found(
                        state,
                        method,
                        headers,
                        &request_path,
                        &initial,
                        incoming(body),
                        version,
                        resolved_ref,
                    )
                    .await
                }
            }
        }
        Output::Public(file) => {
            if !readonly {
                // next start answers 405 for other methods on public files;
                // NextRS keeps its earlier behaviour: not found.
                return not_found(
                    state,
                    method,
                    headers,
                    &request_path,
                    &initial,
                    incoming(body),
                    version,
                    resolved_ref,
                )
                .await;
            }
            // Public files can be replaced while the server runs: compressed
            // per request, never cached.
            file_response(state, &file, PUBLIC_FILE, method, headers, false).await
        }
        Output::App {
            route,
            item_path,
            data,
        } => {
            let route = &state.routes[route];
            let path = item_path.as_str();
            let invocation = invocation_for(state, resolved_ref, &route.page, path);
            if matches!(route.kind, RouteKind::PagesPage | RouteKind::PagesApi) {
                let ctx = pages::Ctx {
                    method,
                    uri,
                    headers,
                    client,
                    version,
                    initial: &initial,
                    max_body: state.max_request_body,
                    listen: &state.listen,
                    locale: resolved_ref.locale.as_ref(),
                };
                // base server's handleNextDataRequest with i18n and no proxy:
                // a data path without a locale renders the 404 page.
                if data.is_some()
                    && state.proxy.is_none()
                    && pages::data_path_without_locale(state, &resolved_ref.pathname)
                {
                    let ctx = pages::Ctx {
                        initial: &[],
                        ..ctx
                    };
                    let keep = initial
                        .iter()
                        .filter_map(|(n, v)| {
                            Some((
                                HeaderName::from_bytes(n.as_bytes()).ok()?,
                                HeaderValue::from_str(v).ok()?,
                            ))
                        })
                        .collect();
                    if let Some(response) = pages::not_found(state, ctx, keep, None).await {
                        return response;
                    }
                }
                return match pages::serve(state, route, path, data, ctx, body, invocation).await {
                    pages::Next::Done(response) => response,
                    pages::Next::NotFound => {
                        // router-server routes on after the NoFallbackError;
                        // its second pass names the path without basePath
                        // (the Pages Router's /404 only, as above).
                        if state.not_found.is_none()
                            && state.proxy.is_some()
                            && headers.contains_key("x-nextjs-data")
                        {
                            let base = state.config.base_path.as_str();
                            let routed = resolved_ref.pathname.as_str();
                            let path = if base.is_empty() {
                                routed
                            } else {
                                resolve::strip_base_path(routed, base).unwrap_or(routed)
                            };
                            return pages::data_not_found(path);
                        }
                        not_found_render(
                            state,
                            method,
                            headers,
                            &request_path,
                            &initial,
                            incoming(Body::empty()),
                            version,
                            resolved_ref,
                            true,
                        )
                        .await
                    }
                };
            }
            let run = |entry: &Path,
                       body: Body,
                       status: Option<u16>,
                       invocation: nextrs_runtime::Invocation| {
                let policy = dynamic::AppPathPolicy {
                    headers: &initial,
                    vary: Some(&state.rsc.vary_header),
                    status,
                    invocation,
                };
                let entry = entry.to_path_buf();
                let request_path = &request_path;
                let initial = &initial;
                async move {
                    let executor = state.executor.as_deref().expect("checked by caller");
                    let response = dynamic::render(executor, &entry, &policy, incoming(body)).await;
                    // NoFallbackError (params the page has no entry for):
                    // router-server routes on without this output, which
                    // ends at the not-found route.
                    // NoFallbackError, or the handler asked for the 404
                    // page (routerServerContext.render404).
                    if response
                        .headers()
                        .get("x-nextrs-error")
                        .is_some_and(|v| v == "no-fallback" || v == "render-404")
                    {
                        return not_found_render(
                            state,
                            method,
                            headers,
                            request_path,
                            initial,
                            incoming(Body::empty()),
                            version,
                            resolved_ref,
                            true,
                        )
                        .await;
                    }
                    dynamic::frame_for(version, response)
                }
            };
            if let Some(page) = state.pages.get(path) {
                // Revalidated since the build: Next's handler serves it from
                // the shared cache (regenerating it first if needed), as next
                // start would, instead of the stale prerender.
                if readonly
                    && page.is_revalidated(state.executor.as_deref())
                    && state.executor.is_some()
                    && let Some(entry) = state.action_entry(path)
                {
                    if rsc::is_rsc_request(headers, &state.rsc.header)
                        && let Some(redirect) = rsc_redirect(state, &request_path, query, headers)
                    {
                        return redirect;
                    }
                    return run(entry, body, None, invocation).await;
                }
                // Draft Mode (or a stale or forged preview cookie): Next's
                // handler decides, rendering on demand for a valid cookie
                // and clearing an invalid one; the prerender is not served.
                if readonly
                    && has_preview_cookie(headers)
                    && state.executor.is_some()
                    && let Some(entry) = state.action_entry(path)
                {
                    return run(entry, body, None, invocation).await;
                }
                if !readonly {
                    // next start hands a POST that may carry a Server Action
                    // to the page's compiled handler even when the page is
                    // prerendered; Next's action handler decides.
                    if is_possible_server_action(method, headers) {
                        if state.executor.is_some()
                            && let Some(entry) = state.action_entry(path)
                        {
                            return run(entry, body, None, invocation).await;
                        }
                        if headers.contains_key("next-action") || state.executor.is_none() {
                            return unsupported(
                                "server-actions",
                                "Server Actions require JavaScript execution (start with --js-runtime embedded-v8)",
                            );
                        }
                    }
                    // A static route's prerender: 405. A dynamic route's
                    // (generateStaticParams) is served as for GET, as next
                    // start's handler serves it from the cache.
                    if !page.dynamic_route {
                        return method_not_allowed(state);
                    }
                }
                if rsc::is_rsc_request(headers, &state.rsc.header) {
                    return serve_rsc(state, page, &request_path, query, headers, method);
                }
                if let Some(html) = &page.html {
                    let mut response = prerendered(
                        state,
                        html,
                        page.status,
                        CACHE_FOREVER,
                        HTML,
                        true,
                        headers,
                        method,
                    );
                    // Next's handler names the route a data request matched.
                    if headers.contains_key("x-nextjs-data")
                        && let Ok(value) = HeaderValue::from_str(path)
                    {
                        response
                            .headers_mut()
                            .insert("x-nextjs-matched-path", value);
                    }
                    return response;
                }
            }
            if let (Some(_), Some(entry)) = (&state.executor, executable_entry(route)) {
                if state.has_interception_routes {
                    return unsupported(
                        "interception-routes",
                        "builds with interception routes are not supported by the NextRS executor",
                    );
                }
                // base-server validates the cache-busting parameter of an
                // RSC request before rendering any route module it serves:
                // App Router pages and route handlers alike, for every
                // method (Pages API routes and edge functions are handled
                // before that check, and are not affected).
                if matches!(route.kind, RouteKind::AppPage | RouteKind::AppRoute)
                    && rsc::is_rsc_request(headers, &state.rsc.header)
                    && let Some(redirect) = rsc_redirect(state, &request_path, query, headers)
                {
                    return redirect;
                }
                return run(entry, body, None, invocation).await;
            }
            let reason = match &route.rendering {
                Rendering::Static => "is prerendered but its stored response is missing",
                Rendering::Revalidated { .. } => "uses ISR, which needs JavaScript to regenerate",
                Rendering::Dynamic => "is rendered on demand by the Next.js server bundle",
                Rendering::Unknown { .. } => "could not be classified from build metadata",
            };
            unsupported(
                "javascript-required",
                &format!(
                    "route {} {reason}; executing it requires JavaScript (start with --js-runtime embedded-v8)",
                    route.page
                ),
            )
        }
    }
}

/// The request carries Draft Mode's (or the legacy Preview Mode's) cookie,
/// valid or not (`__prerender_bypass`, `__next_preview_data`).
fn has_preview_cookie(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.split_once('=').map(|(name, _)| name.trim()))
        .any(|name| name == "__prerender_bypass" || name == "__next_preview_data")
}

/// Next's `isNonHtmlSecFetchDest`: subresource requests get a plain 404.
const NON_HTML_SEC_FETCH_DESTS: &[&str] = &[
    "audio",
    "audioworklet",
    "font",
    "image",
    "json",
    "manifest",
    "paintworklet",
    "report",
    "script",
    "serviceworker",
    "sharedworker",
    "style",
    "track",
    "video",
    "webidentity",
    "worker",
    "xslt",
];

#[allow(clippy::too_many_arguments)]
async fn not_found(
    state: &AppState,
    method: &Method,
    headers: &HeaderMap,
    request_path: &str,
    initial: &[(String, String)],
    incoming: dynamic::Incoming<'_>,
    version: axum::http::Version,
    resolved: &resolve::Resolved,
) -> Response {
    not_found_render(
        state,
        method,
        headers,
        request_path,
        initial,
        incoming,
        version,
        resolved,
        true,
    )
    .await
}

/// `unmatched`: routing found nothing (router-server then sets its no-store
/// Cache-Control before rendering the not-found route).
#[allow(clippy::too_many_arguments)]
async fn not_found_render(
    state: &AppState,
    method: &Method,
    headers: &HeaderMap,
    request_path: &str,
    initial: &[(String, String)],
    incoming: dynamic::Incoming<'_>,
    version: axum::http::Version,
    resolved: &resolve::Resolved,
    unmatched: bool,
) -> Response {
    let readonly = method == Method::GET || method == Method::HEAD;
    let routed = resolved.pathname.as_str();
    let real = if state.config.base_path.is_empty() {
        routed
    } else {
        resolve::strip_base_path(routed, &state.config.base_path).unwrap_or(routed)
    };
    if real.starts_with("/_next/static/") {
        return not_found_plain();
    }
    if readonly
        && headers
            .get("sec-fetch-dest")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|dest| NON_HTML_SEC_FETCH_DESTS.contains(&dest))
    {
        return not_found_plain();
    }
    // next start renders an unmatched possible-action POST with the
    // not-found page's handler, whose action handler answers it (an
    // unknown action is 409, a known one is forwarded to its page); an
    // RSC request for an unknown path gets the not-found page's payload
    // from the same handler. So does a document request when the not-found
    // page has no static shell to serve: Cache Components store it as ISR
    // (or as a Partial Prerendering shell), which only its handler serves.
    let rsc = rsc::is_rsc_request(headers, &state.rsc.header);
    if (is_possible_server_action(method, headers)
        || (readonly && rsc)
        || (readonly && state.not_found.is_none()))
        && let Some(executor) = &state.executor
        && !state.has_interception_routes
        && let Some(entry) = state
            .routes
            .iter()
            .find(|route| route.internal && route.page == "/_not-found")
            .and_then(|route| route.entry.as_deref())
    {
        // router-server sets the no-store Cache-Control before it renders
        // the not-found route (after routing's own headers).
        let mut headers_404 = initial.to_vec();
        if unmatched {
            headers_404.push(("cache-control".to_owned(), NO_STORE.to_owned()));
        }
        let policy = dynamic::AppPathPolicy {
            headers: &headers_404,
            vary: Some(&state.rsc.vary_header),
            status: Some(404),
            invocation: invocation_for(state, resolved, "/_not-found", real),
        };
        let response = dynamic::render(executor, entry, &policy, incoming).await;
        return dynamic::frame_for(version, response);
    }
    // router-server answers a data request routing did not match with an
    // empty JSON object when the build has a proxy.
    // Only for the Pages Router's /404: with an App Router not-found page,
    // router-server renders /_not-found instead.
    if unmatched
        && state.not_found.is_none()
        && state.proxy.is_some()
        && headers.contains_key("x-nextjs-data")
    {
        // The routed path as middleware_next_data left it (basePath and
        // trailing slash included).
        return pages::data_not_found(routed);
    }
    // A Pages Router build without an App Router not-found page.
    if state.not_found.is_none() && state.pages_router_404() {
        // The routing headers go to the page as `keep` (not also as the
        // initial headers, which would send them twice).
        let ctx = pages::Ctx {
            method,
            uri: incoming.uri,
            headers,
            client: incoming.client,
            version,
            initial: &[],
            max_body: state.max_request_body,
            listen: &state.listen,
            locale: resolved.locale.as_ref(),
        };
        let keep = initial
            .iter()
            .filter_map(|(n, v)| {
                Some((
                    HeaderName::from_bytes(n.as_bytes()).ok()?,
                    HeaderValue::from_str(v).ok()?,
                ))
            })
            .collect();
        if let Some(response) =
            pages::not_found(state, ctx, keep, unmatched.then_some(NO_STORE)).await
        {
            return response;
        }
    }
    // next start marks the not-found shell cacheable only when it is
    // requested by its own path.
    let cache_control = if request_path == "/_not-found" {
        CACHE_FOREVER
    } else {
        NO_STORE
    };
    match &state.not_found {
        Some(page) => prerendered(
            state,
            page,
            StatusCode::NOT_FOUND,
            cache_control,
            HTML,
            true,
            headers,
            method,
        ),
        None => not_found_plain(),
    }
}

impl AppState {
    /// The build renders its 404 through the Pages Router.
    fn pages_router_404(&self) -> bool {
        self.pages_router.not_found.is_some()
            || self.pages_router.not_found_entry.is_some()
            || self.pages_router.error_entry.is_some()
    }
}

/// The not-found page a Pages Router handler asked for
/// (`routerServerContext.render404`), keeping the headers it set: the App
/// Router's not-found page when the build has one (next start prefers it),
/// else the Pages Router's.
pub(crate) async fn render_not_found_for_handler(
    state: &AppState,
    ctx: pages::Ctx<'_>,
    keep: Vec<(HeaderName, HeaderValue)>,
) -> Option<Response> {
    if let Some(page) = &state.not_found {
        // The App Router not-found page, sent as its prerender is, with the
        // Cache-Control and other headers the handler set.
        let mut response = prerendered(
            state,
            page,
            StatusCode::NOT_FOUND,
            NO_STORE,
            HTML,
            true,
            ctx.headers,
            ctx.method,
        );
        let headers = response.headers_mut();
        for (name, value) in keep {
            if name == header::CACHE_CONTROL {
                headers.insert(name, value);
            } else {
                headers.append(name, value);
            }
        }
        return Some(response);
    }
    pages::not_found(state, ctx, keep, None).await
}

/// What the metrics listener reports.
#[derive(Clone)]
pub struct MetricsSources {
    /// The JavaScript executor (isolates, queue, heaps, shared cache).
    pub executor: Option<Arc<Executor>>,
    /// The image optimizer (when the build has an image config).
    pub images: Option<ImageStats>,
    /// The gzip cache of immutable responses.
    pub compression: Option<Arc<CompressionCache>>,
    /// The main listener's request, response and connection counters.
    pub http: Arc<HttpMetrics>,
    /// `/readyz` answers 200 only with at least this many ready isolates.
    pub ready_min_isolates: usize,
    /// The memory reclaimer, when enabled.
    pub reclaim: Option<Arc<ReclaimStats>>,
}

impl MetricsSources {
    /// Ready for traffic: not shutting down, and enough isolates ready.
    fn ready(&self) -> Result<(), String> {
        if self
            .http
            .draining
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err("shutting down".into());
        }
        if let Some(executor) = &self.executor {
            let ready = executor.metrics().ready_executors();
            if ready < self.ready_min_isolates.max(1) {
                return Err(format!(
                    "{ready} isolate(s) ready, {} required",
                    self.ready_min_isolates.max(1)
                ));
            }
        }
        Ok(())
    }
}

/// Metrics and health for a separate (loopback) listener:
///
/// * `GET /metrics`: Prometheus text format (executor, shared cache, image
///   optimizer, HTTP, process);
/// * `GET /metrics.json`: the same as JSON;
/// * `GET /healthz`: liveness (the server runs and at least one isolate is
///   ready);
/// * `GET /readyz`: readiness. 503 once graceful shutdown begins, or with
///   fewer ready isolates than `ready_min_isolates`.
///
/// Labels are bounded: no path, host or header value is ever a label.
pub fn metrics_app(sources: MetricsSources) -> Router {
    use axum::routing::get;
    let text = sources.clone();
    let json = sources.clone();
    let health = sources.clone();
    let ready = sources;
    Router::new()
        .route(
            "/metrics",
            get(move || async move {
                let mut out = String::new();
                if let Some(executor) = &text.executor {
                    out.push_str(&executor.metrics().to_prometheus());
                }
                if let Some(images) = &text.images {
                    out.push_str("# HELP nextrs_image_* Image optimizer counters (_total-less names are counters unless named *_bytes, queued or in_flight).\n");
                    for (name, value) in images.snapshot() {
                        out.push_str(&format!("nextrs_image_{name} {value}\n"));
                    }
                }
                if let Some(cache) = &text.compression {
                    out.push_str("# HELP nextrs_compression_cache_* Gzip cache of immutable responses (hits, misses, coalesced, bypassed and evictions are counters).\n");
                    for (name, value) in cache.snapshot() {
                        out.push_str(&format!("nextrs_compression_cache_{name} {value}\n"));
                    }
                }
                out.push_str(&text.http.to_prometheus());
                if let Some(reclaim) = &text.reclaim {
                    out.push_str(&reclaim.to_prometheus());
                }
                let malloc = text
                    .executor
                    .as_ref()
                    .and_then(|e| e.metrics().memory.malloc.clone());
                out.push_str(&http_metrics::process_prometheus(
                    text.http.uptime_seconds(),
                    malloc.as_ref(),
                ));
                ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], out)
            }),
        )
        .route(
            "/metrics.json",
            get(move || async move {
                let mut value = match &json.executor {
                    Some(executor) => serde_json::to_value(executor.metrics()).unwrap_or_default(),
                    None => serde_json::json!({}),
                };
                if let Some(object) = value.as_object_mut() {
                    if let Some(images) = &json.images {
                        let map: serde_json::Map<String, serde_json::Value> = images
                            .snapshot()
                            .into_iter()
                            .map(|(k, v)| (k.to_owned(), v.into()))
                            .collect();
                        object.insert("images".into(), map.into());
                    }
                    if let Some(cache) = &json.compression {
                        let map: serde_json::Map<String, serde_json::Value> = cache
                            .snapshot()
                            .into_iter()
                            .map(|(k, v)| (k.to_owned(), v.into()))
                            .collect();
                        object.insert("compression_cache".into(), map.into());
                    }
                    object.insert("http".into(), json.http.to_json());
                    if let Some(reclaim) = &json.reclaim {
                        object.insert("reclaim".into(), reclaim.to_json());
                    }
                    object.insert(
                        "process".into(),
                        http_metrics::process_json(json.http.uptime_seconds()),
                    );
                }
                axum::Json(value)
            }),
        )
        .route(
            "/healthz",
            get(move || async move {
                let alive = health
                    .executor
                    .as_ref()
                    .is_none_or(|e| e.metrics().ready_executors() > 0);
                if alive {
                    (StatusCode::OK, "ok\n".to_owned())
                } else {
                    (StatusCode::SERVICE_UNAVAILABLE, "no ready isolate\n".to_owned())
                }
            }),
        )
        .route(
            "/readyz",
            get(move || async move {
                match ready.ready() {
                    Ok(()) => (StatusCode::OK, "ready\n".to_owned()),
                    Err(reason) => (StatusCode::SERVICE_UNAVAILABLE, format!("not ready: {reason}\n")),
                }
            }),
        )
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
    /// `fsChecker`'s app files: a route without parameters (framework
    /// routes only where their output is prerendered, as `/_global-error`).
    fn exact_route(&self, path: &str) -> Option<usize> {
        self.routes.iter().position(|route| {
            matches!(route.matcher, CompiledMatcher::Exact(_))
                && route.matches(path)
                && (!route.internal || self.pages.contains_key(path))
        })
    }

    /// The first dynamic route matching `path`, in Next's order.
    fn dynamic_route(&self, path: &str) -> Option<usize> {
        self.routes.iter().position(|route| {
            !route.internal
                && matches!(route.matcher, CompiledMatcher::Pattern(_))
                && route.matches(path)
        })
    }

    /// The addressable route for `path`, as Next's router picks it: a
    /// route without parameters first, then dynamic routes in
    /// routes-manifest order (which Next sorts by specificity).
    fn route_for(&self, path: &str) -> Option<&CompiledRoute> {
        let addressable = || self.routes.iter().filter(|route| !route.internal);
        addressable()
            .find(|route| matches!(route.matcher, CompiledMatcher::Exact(_)) && route.matches(path))
            .or_else(|| addressable().find(|route| route.matches(path)))
    }
}

/// Next's ordering of dynamic routes (`getSortedRoutes`): segment by
/// segment, a static segment before `[param]`, before `[...rest]`, before
/// `[[...rest]]`.
fn route_specificity(page: &str) -> Vec<(u8, &str)> {
    page.split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let rank = if segment.starts_with("[[...") {
                3
            } else if segment.starts_with("[...") {
                2
            } else if segment.starts_with('[') {
                1
            } else {
                0
            };
            (rank, segment)
        })
        .collect()
}

impl AppState {
    /// The compiled page that handles a Server Action POST to `path`: the
    /// App Router page serving it (prerendered, ISR or dynamic) on the
    /// Node.js runtime.
    fn action_entry(&self, path: &str) -> Option<&Path> {
        if self.has_interception_routes {
            return None;
        }
        let route = self.route_for(path)?;
        (route.kind == RouteKind::AppPage && route.runtime == Runtime::Nodejs)
            .then_some(route.entry.as_deref())
            .flatten()
    }
}

/// Routes the executor runs with Next's compiled handler: App Router pages
/// and route handlers for the Node.js runtime. Next's handler decides what
/// to render and what to cache: dynamic routes; ISR routes (served from
/// the shared cache or the build's prerender, regenerated when stale);
/// generateStaticParams routes for params that were not prerendered; and
/// prerendered routes whose output Rust does not hold (route handlers).
fn executable_entry(route: &CompiledRoute) -> Option<&Path> {
    let kind_ok = matches!(
        route.kind,
        RouteKind::AppPage | RouteKind::AppRoute | RouteKind::PagesPage | RouteKind::PagesApi
    );
    // A Pages Router page without data functions is compiled to plain HTML
    // (`pages/about.html`), which has no handler.
    (kind_ok && route.runtime == Runtime::Nodejs)
        .then_some(route.entry.as_deref())
        .flatten()
        .filter(|entry| entry.extension().is_some_and(|ext| ext == "js"))
}

/// next start validates the `_rsc` cache-busting parameter before any App
/// Router render, dynamic ones included.
fn rsc_redirect(
    state: &AppState,
    path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
) -> Option<Response> {
    if state.config.skip_rsc_header_validation {
        return None;
    }
    let router = RouterHeaders::from_headers(
        headers,
        &state.rsc.prefetch_header,
        &state.rsc.segment_prefetch_header,
    );
    if router.matches(rsc::query_hash(query).as_deref()) {
        return None;
    }
    let target = rsc::redirect_target(path, query, &router.expected_hash());
    Some(redirect(
        StatusCode::TEMPORARY_REDIRECT,
        &target,
        false,
        false,
    ))
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
    if !state.config.skip_rsc_header_validation && !router.matches(actual.as_deref()) {
        let target = rsc::redirect_target(path, query, &router.expected_hash());
        return redirect(StatusCode::TEMPORARY_REDIRECT, &target, false, false);
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
    }
    if etag::fresh(request, etag.and_then(|e| e.to_str().ok()), None) {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        return response;
    }
    if let Ok(value) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(cached.body.len()));
    *response.status_mut() = status;
    if method != Method::HEAD {
        *response.body_mut() = Body::from(cached.body.clone());
        // Build output loaded at startup: the same bytes for every request
        // that reaches here (ISR, PPR, Draft Mode and revalidated pages are
        // rendered by Next's handler instead).
        if may_cache_gzip(state, request) {
            response.extensions_mut().insert(compress_cache::Cacheable {
                key: compress_cache::Key::Stored(cached.id),
                identity: cached.body.clone(),
            });
        }
    }
    response
}

/// A response for this request may take its body from the compression
/// cache: compression is on and the request negotiates gzip (the cache is
/// read only then; the final decision stays with `compress::apply`).
fn may_cache_gzip(state: &AppState, request: &HeaderMap) -> bool {
    state.compress && state.compression_cache.enabled() && compress::wants_gzip(request)
}

/// Mirrors the `send` package used by `next start`: weak ETag
/// `W/"<size hex>-<mtime ms hex>"`, Last-Modified, 412 for failed
/// `If-Match` / `If-Unmodified-Since` (`etag::precondition_failed`; next
/// start answers those with a 500 error page), 304 revalidation and
/// single byte ranges (`range.rs`: 206 over the identity bytes, 416, and
/// where NextRS follows RFC 9110 instead of `send`). `immutable`: build
/// output, whose gzip copy the compression cache may hold.
async fn file_response(
    state: &AppState,
    file: &Path,
    cache_control: &'static str,
    method: &Method,
    request: &HeaderMap,
    immutable: bool,
) -> Response {
    // One blocking task opens the file, takes its metadata from the open
    // handle, decides freshness and the range, and reads only the bytes it
    // sends (Phase 3A: separate `tokio::fs` calls each hopped to the
    // blocking pool). It gets only the headers it reads.
    let mut conditional = HeaderMap::new();
    for name in [
        header::IF_MATCH,
        header::IF_UNMODIFIED_SINCE,
        header::IF_MODIFIED_SINCE,
        header::IF_NONE_MATCH,
        header::CACHE_CONTROL,
        header::RANGE,
        header::IF_RANGE,
    ] {
        for value in request.get_all(&name) {
            conditional.append(name.clone(), value.clone());
        }
    }
    let content_type = static_files::content_type(file);
    // The full response would be compressed (given its size): partial
    // responses are identity bytes, so an If-Range is not trusted then.
    let gzip =
        state.compress && compress::wants_gzip(request) && compress::compressible(content_type);
    let options = FileRead {
        body: method != Method::HEAD,
        // RFC 9110 section 14.2: GET is the only method with ranges.
        ranges: method == Method::GET,
        gzip,
        cache_key: immutable && gzip && state.compression_cache.enabled(),
    };
    let path = file.to_path_buf();
    let loaded = tokio::task::spawn_blocking(move || load_file(&path, &conditional, options)).await;
    let Ok(Some(loaded)) = loaded else {
        return not_found_plain();
    };

    let mut response = Response::new(Body::empty());
    let headers = response.headers_mut();
    // An error is not the file: never `immutable` (a cache would keep it).
    if loaded.selection != range::Selection::Unsatisfiable && !loaded.precondition_failed {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(cache_control),
        );
    }
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Some(etag) = loaded.etag {
        headers.insert(header::ETAG, etag);
    }
    if let Some(last_modified) = loaded.last_modified {
        headers.insert(header::LAST_MODIFIED, last_modified);
    }
    if loaded.precondition_failed {
        *response.status_mut() = StatusCode::PRECONDITION_FAILED;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        );
        *response.body_mut() = Body::from("Precondition Failed");
        return response;
    }
    if loaded.fresh {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        return response;
    }
    match loaded.selection {
        range::Selection::Unsatisfiable => {
            *response.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
            let headers = response.headers_mut();
            if let Ok(value) = HeaderValue::from_str(&range::unsatisfied_range(loaded.len)) {
                headers.insert(header::CONTENT_RANGE, value);
            }
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            );
            *response.body_mut() = Body::from("Range Not Satisfiable");
            return response;
        }
        range::Selection::Partial { start, end } => {
            *response.status_mut() = StatusCode::PARTIAL_CONTENT;
            if let Ok(value) = HeaderValue::from_str(&range::content_range(start, end, loaded.len))
            {
                response.headers_mut().insert(header::CONTENT_RANGE, value);
            }
        }
        range::Selection::Full => {}
    }
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    let length = match (&loaded.body, loaded.selection) {
        (Some(body), _) => body.len() as u64,
        (None, range::Selection::Partial { start, end }) => end - start + 1,
        (None, _) => loaded.len,
    };
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    if let Some(body) = loaded.body {
        let body = Bytes::from(body);
        *response.body_mut() = Body::from(body.clone());
        if let Some(key) = loaded.key {
            response.extensions_mut().insert(compress_cache::Cacheable {
                key: compress_cache::Key::File(key),
                identity: body,
            });
        }
    }
    response
}

/// What `load_file` reads besides the validators.
#[derive(Clone, Copy)]
struct FileRead {
    /// The bytes to send (not for HEAD).
    body: bool,
    /// The request may select a byte range (GET).
    ranges: bool,
    /// The full response would be gzip if large enough.
    gzip: bool,
    /// A compression cache key is wanted for a full body.
    cache_key: bool,
}

/// A file's validators, freshness, selected range and (when it will be
/// sent) the bytes.
struct LoadedFile {
    etag: Option<HeaderValue>,
    last_modified: Option<HeaderValue>,
    /// `If-Match` / `If-Unmodified-Since` failed: 412, nothing read.
    precondition_failed: bool,
    fresh: bool,
    len: u64,
    selection: range::Selection,
    /// The selected bytes: the whole file, or the range.
    body: Option<Vec<u8>>,
    /// The compression cache key for a whole-file `body` (asked for, and
    /// the file did not change while it was read).
    key: Option<compress_cache::FileKey>,
}

/// Blocking: open, stat the handle, check the conditional headers, select
/// the range, read. `None` when the file cannot be opened, stat'ed or read
/// (a 404, as before).
fn load_file(path: &Path, conditional: &HeaderMap, read: FileRead) -> Option<LoadedFile> {
    use std::io::{Read, Seek, SeekFrom};
    let mut handle = std::fs::File::open(path).ok()?;
    let metadata = handle.metadata().ok()?;
    let len = metadata.len();
    let modified = metadata.modified().ok();
    let etag = modified
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| {
            // Node builds `stat.mtime` with Math.round(mtimeMs).
            let millis = d.as_secs() * 1000 + (u64::from(d.subsec_nanos()) + 500_000) / 1_000_000;
            HeaderValue::from_str(&format!("W/\"{len:x}-{millis:x}\"")).ok()
        });
    let last_modified =
        modified.and_then(|m| HeaderValue::from_str(&httpdate::fmt_http_date(m)).ok());
    // RFC 9110 section 13.2.2: If-Match / If-Unmodified-Since first (a 412
    // is never turned into a 304 or a range), then If-None-Match /
    // If-Modified-Since before Range (a 304 whatever the range).
    if etag::precondition_failed(
        conditional,
        etag.as_ref().and_then(|e| e.to_str().ok()),
        modified,
    ) {
        return Some(LoadedFile {
            etag,
            last_modified,
            precondition_failed: true,
            fresh: false,
            len,
            selection: range::Selection::Full,
            body: None,
            key: None,
        });
    }
    let fresh = etag::fresh(
        conditional,
        etag.as_ref().and_then(|e| e.to_str().ok()),
        last_modified.as_ref().and_then(|l| l.to_str().ok()),
    );
    let mut ranges = conditional.get_all(header::RANGE).iter();
    let selection = match (read.ranges && !fresh, ranges.next(), ranges.next()) {
        (true, Some(value), None) => match value.to_str() {
            Ok(value) => range::select(value, len),
            Err(_) => range::Selection::Full,
        },
        // No Range, HEAD, or a repeated header (not one ranges-specifier).
        _ => range::Selection::Full,
    };
    let if_range = if read.gzip && len >= compress::THRESHOLD {
        !conditional.contains_key(header::IF_RANGE)
    } else {
        range::if_range_holds(
            conditional,
            etag.as_ref().and_then(|e| e.to_str().ok()),
            modified,
        )
    };
    let selection = if if_range {
        selection
    } else {
        range::Selection::Full
    };
    let body = match selection {
        _ if fresh || !read.body => None,
        range::Selection::Unsatisfiable => None,
        range::Selection::Partial { start, end } => {
            let wanted = end - start + 1;
            handle.seek(SeekFrom::Start(start)).ok()?;
            let mut bytes = Vec::with_capacity(usize::try_from(wanted).ok()?);
            (&mut handle).take(wanted).read_to_end(&mut bytes).ok()?;
            // Truncated since the stat: the range no longer exists as
            // described.
            if bytes.len() as u64 != wanted {
                return None;
            }
            Some(bytes)
        }
        range::Selection::Full => {
            let mut bytes = Vec::with_capacity(len as usize);
            handle.read_to_end(&mut bytes).ok()?;
            Some(bytes)
        }
    };
    // The key describes the bytes read only if the file is the same after
    // the read as before it (a write in between changes its size or times).
    let key = match &body {
        Some(bytes)
            if read.cache_key
                && selection == range::Selection::Full
                && bytes.len() as u64 == len =>
        {
            let before = compress_cache::FileKey::of(path, &metadata);
            let after = handle
                .metadata()
                .ok()
                .and_then(|m| compress_cache::FileKey::of(path, &m));
            before.filter(|b| after.as_ref() == Some(b))
        }
        _ => None,
    };
    Some(LoadedFile {
        etag,
        last_modified,
        precondition_failed: false,
        fresh,
        len,
        selection,
        body,
        key,
    })
}

/// Routing redirects (router-server) carry the target as body, with a
/// `Refresh` header for 308; the RSC cache-busting redirect (307) is empty.
fn redirect(status: StatusCode, target: &str, with_refresh: bool, with_body: bool) -> Response {
    let Ok(location) = HeaderValue::from_bytes(&resolve::latin1_bytes(target)) else {
        return (StatusCode::BAD_REQUEST, "Bad Request").into_response();
    };
    let body = if with_body {
        target.to_owned()
    } else {
        String::new()
    };
    let mut response = (status, body).into_response();
    let headers = response.headers_mut();
    if with_refresh
        && let Ok(refresh) =
            HeaderValue::from_bytes(&resolve::latin1_bytes(&format!("0;url={target}")))
    {
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
