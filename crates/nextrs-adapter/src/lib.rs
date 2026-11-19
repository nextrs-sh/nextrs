//! Reads the output of the official `next build` for the NextRS runtime.
//!
//! Two metadata sources produce the same [`BuildInfo`]:
//!
//! - [`MetadataSource::AdapterApi`]: `.next/nextrs/build-output.json`, written
//!   by `packages/next-adapter` through the public Deployment Adapter API.
//! - [`MetadataSource::InternalManifests`]: the undocumented manifests that
//!   `next start` reads. Always present, used when the adapter did not run.
//!
//! Nothing here executes JavaScript.

mod adapter_output;
mod error;
mod glob;
mod images;
mod internal;
mod manifests;
mod model;
mod routing;

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use error::InspectError;
pub use images::{ImagesConfig, LocalPattern, OptimizerSettings, RemotePattern};
pub use model::*;

use manifests::{
    FunctionsConfigManifest, MiddlewareManifest, RequiredServerFiles, ServerReferenceManifest,
    read_json_optional,
};

/// The Next.js major version this release serves. Builds of another major
/// are refused at startup (`unsupported-next-version`).
pub const SUPPORTED_NEXT_MAJOR: u64 = 16;

/// The release line NextRS is tested against (the compatibility suites run
/// 16.4.x patch releases). Other 16.x minors start with a warning.
pub const TESTED_NEXT_VERSION: &str = "16.4";

/// How a detected Next.js version relates to what this release supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSupport {
    /// The tested release line.
    Tested,
    /// The supported major, another minor: expected to work, not tested.
    Untested,
    /// Another major, or a version string that is not `major.minor[...]`.
    Unsupported,
}

/// Classifies a Next.js version string (`16.4.6`, `16.5.0-canary.3`,
/// `v16.4.6`).
pub fn version_support(version: &str) -> VersionSupport {
    let version = version.trim().trim_start_matches('v');
    let mut parts = version.split('.');
    let major = parts.next().and_then(|m| m.parse::<u64>().ok());
    let minor = parts
        .next()
        .map(|m| m.split(|c: char| !c.is_ascii_digit()).next().unwrap_or(""))
        .and_then(|m| m.parse::<u64>().ok());
    match (major, minor) {
        (Some(SUPPORTED_NEXT_MAJOR), Some(_))
            if version.starts_with(&format!("{TESTED_NEXT_VERSION}.")) =>
        {
            VersionSupport::Tested
        }
        (Some(SUPPORTED_NEXT_MAJOR), Some(_)) => VersionSupport::Untested,
        _ => VersionSupport::Unsupported,
    }
}

/// The diagnostic code for a build of an unsupported Next.js version.
pub const UNSUPPORTED_VERSION_CODE: &str = "unsupported-next-version";

#[derive(Debug, Clone, Default)]
pub struct InspectOptions {
    /// Ignore `.next/nextrs/build-output.json` even when it is present.
    pub ignore_adapter_output: bool,
}

/// Inspects a Next.js project directory (or its `.next` directory).
pub fn inspect(path: &Path) -> Result<BuildInfo, InspectError> {
    inspect_with(path, &InspectOptions::default())
}

pub fn inspect_with(path: &Path, options: &InspectOptions) -> Result<BuildInfo, InspectError> {
    let (project_dir, dist_dir) = locate(path)?;
    let build_id = std::fs::read_to_string(dist_dir.join("BUILD_ID"))
        .map_err(|source| InspectError::Io {
            path: dist_dir.join("BUILD_ID"),
            source,
        })?
        .trim()
        .to_owned();

    let mut diagnostics = Vec::new();
    let mut routes_manifest = internal::load_routes_manifest(&dist_dir)?;
    let prerender_manifest = internal::load_prerender_manifest(&dist_dir)?;
    let required: Option<RequiredServerFiles> =
        read_json_optional(&dist_dir.join("required-server-files.json"))?;
    // Next's i18n config: the object next start reads (routes-manifest,
    // else required-server-files), for both metadata sources.
    let i18n_value = routes_manifest.i18n.clone().or_else(|| {
        required
            .as_ref()
            .and_then(|r| r.config.get("i18n").cloned())
    });
    let (i18n, i18n_error) = match i18n_value.as_ref().map(I18nConfig::from_value) {
        Some(Ok(parsed)) => (parsed, None),
        Some(Err(error)) => (None, Some(error)),
        None => (None, None),
    };
    let internal = internal::load(
        &dist_dir,
        &routes_manifest,
        &prerender_manifest,
        i18n.as_ref().map(|i| i.locales.as_slice()),
        &mut diagnostics,
    )?;

    let adapter = if options.ignore_adapter_output {
        None
    } else {
        adapter_output::read(&dist_dir)?
    };
    // The adapter file survives rebuilds without the adapter, so only trust it
    // when it describes this exact build.
    let adapter = adapter.filter(|output| {
        let fresh = output.build_id == build_id;
        if !fresh {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code: "stale-adapter-output",
                message: format!(
                    "{} is from build {} (current {build_id}); using internal manifests",
                    adapter_output::RELATIVE_PATH,
                    output.build_id
                ),
            });
        }
        fresh
    });

    let mut next_version = None;
    let mut adapter_stored_files = Vec::new();
    let (source, mut routes, mut prerenders, rsc, mut config) = match adapter {
        Some(output) => {
            next_version = Some(DetectedVersion {
                version: output.next_version.clone(),
                source: adapter_output::RELATIVE_PATH.into(),
            });
            let config = RoutingConfig {
                base_path: output.config.base_path.clone(),
                trailing_slash: output.config.trailing_slash,
                i18n: None,
                powered_by_header: output.config.powered_by_header,
                generate_etags: output.config.generate_etags,
                skip_proxy_url_normalize: config_flag(required.as_ref(), "skipProxyUrlNormalize"),
            };
            let tables = adapter_output::tables(&project_dir, output, &mut diagnostics);
            // One model for both sources: the Pages Router's routes and
            // stored outputs come from the manifests next start reads (the
            // adapter runs after next build wrote them all), checked
            // against what the adapter reported.
            check_adapter_pages(
                &internal,
                &tables.pages,
                &build_id,
                i18n.as_ref().map(|i| i.locales.as_slice()),
                &mut diagnostics,
            );
            adapter_stored_files = tables.stored_files;
            let mut routes = tables.routes;
            let mut prerenders = tables.prerenders;
            routes.extend(
                internal
                    .routes
                    .iter()
                    .filter(|r| matches!(r.kind, RouteKind::PagesPage | RouteKind::PagesApi))
                    .cloned(),
            );
            prerenders.extend(internal.prerenders.iter().filter(|p| p.pages).cloned());
            (
                MetadataSource::AdapterApi,
                routes,
                prerenders,
                tables.rsc,
                config,
            )
        }
        None => (
            MetadataSource::InternalManifests,
            internal.routes.clone(),
            internal.prerenders.clone(),
            routes_manifest
                .rsc
                .take()
                .map(Into::into)
                .unwrap_or_default(),
            routing_config(required.as_ref(), &routes_manifest.base_path),
        ),
    };

    config.i18n = i18n;

    if source == MetadataSource::AdapterApi {
        supplement_internal_prerenders(&internal, &mut routes, &mut prerenders, &mut diagnostics);
    }

    if next_version.is_none() {
        next_version = detect_version(&project_dir, &dist_dir);
    }
    match &next_version {
        Some(v) => match version_support(&v.version) {
            VersionSupport::Tested => {}
            VersionSupport::Untested => diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code: "untested-next-version",
                message: format!(
                    "Next.js {} (from {}) has not been tested with this NextRS release, \
                     which is tested with {TESTED_NEXT_VERSION}.x",
                    v.version, v.source
                ),
            }),
            VersionSupport::Unsupported => diagnostics.push(Diagnostic {
                severity: Severity::Unsupported,
                code: UNSUPPORTED_VERSION_CODE,
                message: format!(
                    "unsupported Next.js build version {} (from {}): this NextRS release \
                     serves Next.js {SUPPORTED_NEXT_MAJOR} builds and is tested with \
                     {TESTED_NEXT_VERSION}.x",
                    v.version, v.source
                ),
            }),
        },
        None => diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "unknown-next-version",
            message: "could not determine the Next.js version".into(),
        }),
    }

    routes.sort_by(|a, b| a.page.cmp(&b.page));
    prerenders.sort_by(|a, b| a.pathname.cmp(&b.pathname));
    let public_dir = Some(project_dir.join("public")).filter(|dir| dir.is_dir());
    let mut invalid_rules = Vec::new();
    let mut routing = routing::routing_rules(&dist_dir, &mut invalid_rules)?;
    routing.proxy_timeout_ms = routing::proxy_timeout_ms(required.as_ref().map(|r| &r.config));
    let proxy = routing::proxy_info(&dist_dir, required.as_ref().map(|r| &r.config))?;
    let images = images::images_config(
        &dist_dir,
        required.as_ref().map(|r| &r.config),
        &mut diagnostics,
    )?;
    for rule in invalid_rules {
        diagnostics.push(Diagnostic {
            severity: Severity::Unsupported,
            code: "routing-rule",
            message: format!("routes-manifest rule without source or regex: {rule}"),
        });
    }
    let workspace_root = workspace_root(
        &project_dir,
        required
            .as_ref()
            .and_then(|r| r.relative_app_dir.as_deref()),
        &mut diagnostics,
    );
    let mut info = BuildInfo {
        static_dir: dist_dir.join("static"),
        workspace_root,
        project_dir,
        dist_dir,
        build_id,
        next_version,
        source,
        routers: internal.routers,
        pages_router: internal.pages_router.clone(),
        adapter_stored_files,
        config,
        rsc,
        routes,
        prerenders,
        public_dir,
        server_actions: ServerActionsInfo::default(),
        routing,
        proxy,
        images,
        diagnostics,
    };
    if let Some(error) = i18n_error {
        info.diagnostics.push(Diagnostic {
            severity: Severity::Unsupported,
            code: "i18n",
            message: format!("the i18n config could not be read: {error}"),
        });
    }
    detect_features(&mut info, &routes_manifest)?;
    info.server_actions = server_actions_info(&info, required.as_ref())?;
    Ok(info)
}

/// Accepts a project directory or a `.next` directory.
fn locate(path: &Path) -> Result<(PathBuf, PathBuf), InspectError> {
    let path = path.canonicalize().map_err(|source| InspectError::Io {
        path: path.to_owned(),
        source,
    })?;
    if path.join("BUILD_ID").is_file() && path.join("server").is_dir() {
        let project = path.parent().unwrap_or(&path).to_owned();
        return Ok((project, path));
    }
    let dist = path.join(".next");
    if !dist.join("BUILD_ID").is_file() {
        return Err(InspectError::NoBuildOutput { dist_dir: dist });
    }
    Ok((path, dist))
}

/// The workspace root of a monorepo build: `project_dir` with Next's
/// `relativeAppDir` removed from its end. Only plain relative paths are
/// accepted (no `..`, no absolute paths), and only when `project_dir`
/// really ends with them: the build may have been moved, and then the root
/// cannot be known. Never an ancestor chosen from anything but the build.
pub(crate) fn workspace_root(
    project_dir: &Path,
    relative_app_dir: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PathBuf> {
    use std::path::Component;
    let relative = relative_app_dir?.trim_matches('/');
    if relative.is_empty() {
        return None;
    }
    let relative = Path::new(relative);
    let plain = relative
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    let mut root = project_dir.to_path_buf();
    let matches = plain
        && relative
            .components()
            .rev()
            .all(|component| root.file_name() == Some(component.as_os_str()) && root.pop());
    if !matches {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "workspace-root",
            message: format!(
                "required-server-files.json gives the app directory as {:?} within its workspace, \
                 but {} does not end with it; packages are read from <project>/node_modules only",
                relative.display(),
                project_dir.display()
            ),
        });
        return None;
    }
    Some(root)
}

fn routing_config(required: Option<&RequiredServerFiles>, base_path: &str) -> RoutingConfig {
    let config = required.map(|r| &r.config);
    let flag = |key: &str, default: bool| {
        config
            .and_then(|c| c.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    };
    RoutingConfig {
        base_path: base_path.to_owned(),
        trailing_slash: flag("trailingSlash", false),
        i18n: None,
        powered_by_header: flag("poweredByHeader", true),
        generate_etags: flag("generateEtags", true),
        skip_proxy_url_normalize: flag("skipProxyUrlNormalize", false)
            || flag("skipMiddlewareUrlNormalize", false),
    }
}

fn config_flag(required: Option<&RequiredServerFiles>, key: &str) -> bool {
    required
        .and_then(|r| r.config.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn server_actions_info(
    info: &BuildInfo,
    required: Option<&RequiredServerFiles>,
) -> Result<ServerActionsInfo, InspectError> {
    let manifest: ServerReferenceManifest =
        read_json_optional(&info.dist_dir.join("server/server-reference-manifest.json"))?
            .unwrap_or_default();
    let options = required
        .and_then(|r| r.config.get("experimental"))
        .and_then(|e| e.get("serverActions"));
    let configured = options
        .and_then(|o| o.get("bodySizeLimit"))
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        });
    let body_size_limit = configured
        .as_deref()
        .and_then(parse_byte_size)
        .unwrap_or(DEFAULT_ACTION_BODY_LIMIT);
    let allowed_origins = options
        .and_then(|o| o.get("allowedOrigins"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(ServerActionsInfo {
        count: manifest.node.len(),
        edge_count: manifest.edge.len(),
        body_size_limit,
        body_size_limit_config: configured,
        allowed_origins,
    })
}

/// The adapter API does not list prerendered shells of framework routes such
/// as `/_not-found` (16.4.3), although `next start` serves them for 404s.
/// Compares the adapter's Pages Router outputs with the internal tables the
/// model uses. Every page and API route function the adapter reported must
/// be a route with the same entry (a getServerSideProps page's data path
/// one of its data routes), every route with a handler must be reported,
/// and every stored response the adapter reported must be the file the
/// model serves or seeds the cache with. A difference is a refusal: the
/// model would not describe the build the adapter saw.
fn check_adapter_pages(
    internal: &internal::InternalTables,
    adapter: &adapter_output::AdapterPages,
    build_id: &str,
    locales: Option<&[String]>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // With i18n the adapter lists each function once without a locale and
    // once per locale (`/fr/ssr`, the index page as `/index` and `/fr`);
    // the filesystem router strips the locale to find the page.
    let page_of = |pathname: &str| -> String {
        let page = match locales {
            Some(locales) => normalize_locale_path(pathname, locales).0,
            None => pathname,
        };
        if page == "/index" {
            "/".to_owned()
        } else {
            page.to_owned()
        }
    };
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    };
    let data_prefix = format!("/_next/data/{build_id}");
    // `/_next/data/<build>/x.json` -> `/x` (`/index.json` -> `/`).
    let data_page = |pathname: &str| -> Option<String> {
        let rest = pathname.strip_prefix(&data_prefix)?.strip_suffix(".json")?;
        Some(match rest {
            "/index" => "/".to_owned(),
            rest => rest.to_owned(),
        })
    };
    let mut mismatches = Vec::new();
    for (kind, pathname, file) in &adapter.functions {
        if let Some(page) = data_page(pathname).map(|p| page_of(&p)) {
            if !internal
                .pages_router
                .data_routes
                .iter()
                .any(|d| d.page == page)
            {
                mismatches.push(format!(
                    "data route {pathname} has no page with getServerSideProps"
                ));
            }
            continue;
        }
        let pathname = &page_of(pathname);
        if pathname == "/_error" {
            if !internal
                .pages_router
                .error_entry
                .as_deref()
                .is_some_and(|e| same(e, file))
            {
                mismatches.push("_error entry differs".to_owned());
            }
            continue;
        }
        match internal.routes.iter().find(|r| r.page == *pathname) {
            Some(route)
                if route.kind == *kind && route.entry.as_deref().is_some_and(|e| same(e, file)) => {
            }
            // The adapter reports the 404 page as a function (its module
            // stub) even when pages-manifest maps it to the HTML next start
            // serves; so does the model.
            Some(route)
                if pathname == "/404"
                    && route
                        .entry
                        .as_ref()
                        .is_some_and(|e| e.extension().is_some_and(|x| x == "html")) => {}
            Some(_) => mismatches.push(format!("{pathname}: kind or entry differs")),
            None => mismatches.push(format!("{pathname}: no such route")),
        }
    }
    for route in internal.routes.iter().filter(|r| {
        matches!(r.kind, RouteKind::PagesPage | RouteKind::PagesApi)
            && r.entry
                .as_ref()
                .is_some_and(|e| e.extension().is_some_and(|x| x == "js"))
    }) {
        if !adapter
            .functions
            .iter()
            .any(|(_, p, _)| page_of(p) == route.page)
        {
            mismatches.push(format!("{}: not in the adapter output", route.page));
        }
    }
    for (pathname, file, status) in &adapter.prerenders {
        let (page, data) = match data_page(pathname) {
            Some(page) => (page, true),
            None => (pathname.clone(), false),
        };
        // A fallback page (`/x/[slug]`) is Next's handler's own business;
        // a build-time notFound has no stored output of its own.
        if page.contains('[') || *status == Some(404) || file.is_none() {
            continue;
        }
        let stored = internal
            .prerenders
            .iter()
            .find(|p| p.pages && p.pathname == page)
            .and_then(|p| {
                if data {
                    p.data.as_ref()
                } else {
                    p.html.as_ref()
                }
            });
        match (stored, file) {
            (Some(stored), Some(file)) if same(&stored.file, file) => {}
            _ => mismatches.push(format!("{pathname}: stored output differs")),
        }
    }
    if !mismatches.is_empty() {
        let shown = mismatches.len().min(5);
        diagnostics.push(Diagnostic {
            severity: Severity::Unsupported,
            code: "pages-adapter-mismatch",
            message: format!(
                "the adapter output and the build's manifests disagree on {} Pages Router entries: {}{}",
                mismatches.len(),
                mismatches[..shown].join("; "),
                if mismatches.len() > shown { "; ..." } else { "" }
            ),
        });
    }
}

fn supplement_internal_prerenders(
    internal: &internal::InternalTables,
    routes: &mut [Route],
    prerenders: &mut Vec<Prerender>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for prerender in &internal.prerenders {
        if prerenders.iter().any(|p| p.pathname == prerender.pathname) {
            continue;
        }
        diagnostics.push(Diagnostic {
            severity: Severity::Info,
            code: "adapter-missing-prerender",
            message: format!(
                "adapter output omits prerendered {}; read from prerender-manifest.json",
                prerender.pathname
            ),
        });
        if let Some(route) = routes.iter_mut().find(|r| r.page == prerender.route) {
            route.rendering = internal
                .routes
                .iter()
                .find(|r| r.page == route.page)
                .map_or(Rendering::Static, |r| r.rendering.clone());
        }
        prerenders.push(prerender.clone());
    }
}

#[derive(Deserialize)]
struct PackageVersion {
    version: String,
}

fn detect_version(project_dir: &Path, dist_dir: &Path) -> Option<DetectedVersion> {
    let candidates = [
        // Written by `next build` (undocumented).
        (
            dist_dir.join("diagnostics/framework.json"),
            ".next/diagnostics/framework.json",
        ),
        (
            project_dir.join("node_modules/next/package.json"),
            "node_modules/next/package.json",
        ),
    ];
    candidates.into_iter().find_map(|(path, label)| {
        let parsed: PackageVersion = read_json_optional(&path).ok().flatten()?;
        Some(DetectedVersion {
            version: parsed.version,
            source: label.into(),
        })
    })
}

/// Flags build features Phase 0 cannot serve correctly. Serving a build that
/// relies on them without running them (e.g. auth in middleware, security
/// headers in next.config) would silently return wrong responses.
fn detect_features(
    info: &mut BuildInfo,
    routes_manifest: &manifests::RoutesManifest,
) -> Result<(), InspectError> {
    let server_dir = info.dist_dir.join("server");
    let middleware: MiddlewareManifest =
        read_json_optional(&server_dir.join("middleware-manifest.json"))?.unwrap_or_default();
    let functions: FunctionsConfigManifest =
        read_json_optional(&server_dir.join("functions-config-manifest.json"))?.unwrap_or_default();
    let actions: ServerReferenceManifest =
        read_json_optional(&server_dir.join("server-reference-manifest.json"))?.unwrap_or_default();

    let mut push = |severity, code, message: String| {
        info.diagnostics.push(Diagnostic {
            severity,
            code,
            message,
        })
    };

    if !middleware.middleware.is_empty() {
        push(
            Severity::Unsupported,
            "middleware",
            "edge-runtime middleware is present; NextRS runs only the Node.js proxy, and serving routes without it could bypass its logic"
                .into(),
        );
    }
    if functions.functions.contains_key("/_middleware") {
        push(
            Severity::Unsupported,
            "proxy",
            "proxy (middleware) requires JavaScript execution; serving routes without running it could bypass its logic"
                .into(),
        );
        if info.config.skip_proxy_url_normalize {
            push(
                Severity::Unsupported,
                "skip-proxy-url-normalize",
                "skipProxyUrlNormalize is not supported with the proxy".into(),
            );
        }
    }
    let custom_redirects = routes_manifest
        .redirects
        .iter()
        .filter(|r| !r.internal)
        .count();
    let custom_rules = info.routing.custom_count();
    if custom_rules > 0 {
        push(
            Severity::Info,
            "routing-rules",
            format!(
                "{} header rules, {custom_redirects} redirects and {} rewrites from next.config are applied",
                info.routing.headers.len(),
                routes_manifest.rewrite_count()
            ),
        );
    }
    let external: Vec<&str> = info
        .routing
        .before_files
        .iter()
        .chain(&info.routing.after_files)
        .chain(&info.routing.fallback)
        .filter_map(|r| r.destination.as_deref())
        .filter(|d| !d.starts_with('/'))
        .collect();
    if !external.is_empty() {
        push(
            Severity::Warning,
            "external-rewrites",
            format!(
                "{} rewrites to other origins are forwarded only to destinations the network policy allows (--net-allow; otherwise 502): {}",
                external.len(),
                external.join(", ")
            ),
        );
    }
    if let Some(i18n) = &info.config.i18n {
        // Next warns that i18n does not apply to app/ and routes its pages
        // only under the default locale; that combination is untested here.
        let app_routes = info
            .routes
            .iter()
            .filter(|r| matches!(r.kind, RouteKind::AppPage | RouteKind::AppRoute) && !r.internal)
            .count();
        if app_routes > 0 {
            push(
                Severity::Unsupported,
                "i18n-app-router",
                format!(
                    "Next's i18n config with {app_routes} App Router routes is not supported (Pages Router only)"
                ),
            );
        } else {
            push(
                Severity::Info,
                "i18n",
                format!(
                    "Pages Router i18n: locales {} (default {}){}{}",
                    i18n.locales.join(", "),
                    i18n.default_locale,
                    if i18n.domains.is_empty() {
                        String::new()
                    } else {
                        format!(", {} locale domains", i18n.domains.len())
                    },
                    if i18n.locale_detection {
                        ""
                    } else {
                        ", no locale detection"
                    }
                ),
            );
        }
    } else if routes_manifest.i18n.as_ref().is_some_and(|v| !v.is_null()) {
        push(Severity::Unsupported, "i18n", "i18n routing".into());
    }
    if info.routers.pages {
        push(
            Severity::Info,
            "pages-router",
            if info.source == MetadataSource::AdapterApi {
                "Pages Router (deployment-adapter build): routes and stored outputs read from the manifests next start reads and checked against the adapter output; getServerSideProps, getStaticProps, API routes and /_next/data run Next's compiled handlers".into()
            } else {
                "Pages Router: pages without data functions are served as built; getServerSideProps, getStaticProps, API routes and /_next/data run Next's compiled handlers".into()
            },
        );
    }
    if !actions.node.is_empty() {
        push(
            Severity::Unsupported,
            "server-actions",
            format!(
                "{} Server Actions require JavaScript execution",
                actions.node.len()
            ),
        );
    }
    if !actions.edge.is_empty() {
        push(
            Severity::Unsupported,
            "edge-server-actions",
            format!(
                "{} Server Actions target the edge runtime, which NextRS does not run",
                actions.edge.len()
            ),
        );
    }
    let revalidated: Vec<&str> = info
        .prerenders
        .iter()
        .filter(|p| p.revalidate.is_some())
        .map(|p| p.pathname.as_str())
        .collect();
    if !revalidated.is_empty() {
        push(
            Severity::Unsupported,
            "isr",
            format!(
                "ISR regeneration requires JavaScript: {}",
                revalidated.join(", ")
            ),
        );
    }
    for route in &info.routes {
        if let Matcher::Pattern { regex, .. } = &route.matcher
            && let Err(error) = regex::Regex::new(regex)
        {
            push(
                Severity::Unsupported,
                "route-regex",
                format!(
                    "{}: Next.js regex is not supported by the Rust regex engine: {error}",
                    route.page
                ),
            );
        }
    }
    let edge = info
        .routes
        .iter()
        .filter(|r| r.runtime == Runtime::Edge)
        .count();
    if edge > 0 {
        push(
            Severity::Info,
            "edge-runtime",
            format!("{edge} routes target the edge runtime"),
        );
    }
    let needs_js = info
        .routes
        .iter()
        .filter(|r| !r.internal && !matches!(r.rendering, Rendering::Static))
        .count();
    if needs_js > 0 {
        push(
            Severity::Info,
            "javascript-required",
            format!("{needs_js} routes require server-side JavaScript execution"),
        );
    }
    if !info.static_dir.is_dir() {
        push(
            Severity::Warning,
            "missing-static-dir",
            format!(
                "{} does not exist; /_next/static requests will 404 (standalone output does not copy it)",
                info.static_dir.display()
            ),
        );
    }
    if info.dist_dir.join("standalone").is_dir() {
        push(
            Severity::Info,
            "standalone",
            "output: 'standalone' bundle present".into(),
        );
    }
    match &info.images {
        None => push(
            Severity::Info,
            "image-optimizer",
            "no images config (images-manifest.json or required-server-files.json): /_next/image answers 404".into(),
        ),
        Some(images) => {
            if images.disabled() {
                push(
                    Severity::Info,
                    "image-optimizer",
                    "images.unoptimized or a custom loader: /_next/image answers 404, as in next start".into(),
                );
            }
            if images.custom_cache_handler {
                push(
                    Severity::Warning,
                    "image-cache-handler",
                    "images.customCacheHandler is not supported: optimized images use NextRS's own cache".into(),
                );
            }
            if !images.remote_patterns.is_empty() || !images.domains.is_empty() {
                push(
                    Severity::Info,
                    "remote-images",
                    "images.remotePatterns: remote images are fetched only from origins --net-allow also allows".into(),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod adapter_pages_tests {
    //! Negative controls for the adapter cross-check: an adapter entry the
    //! model ignores, or a route the adapter did not report, must refuse.
    use super::*;
    use std::path::PathBuf;

    fn route(page: &str, kind: RouteKind, entry: &str) -> Route {
        Route {
            page: page.into(),
            kind,
            rendering: Rendering::Dynamic,
            runtime: Runtime::Nodejs,
            entry: Some(PathBuf::from(entry)),
            matcher: Matcher::Exact {
                pathname: page.into(),
            },
            internal: false,
        }
    }

    fn tables() -> (internal::InternalTables, adapter_output::AdapterPages) {
        let internal = internal::InternalTables {
            routes: vec![
                route("/ssr", RouteKind::PagesPage, "/b/server/pages/ssr.js"),
                route("/api/x", RouteKind::PagesApi, "/b/server/pages/api/x.js"),
                route("/about", RouteKind::PagesPage, "/b/server/pages/about.html"),
            ],
            prerenders: Vec::new(),
            routers: Routers {
                app: false,
                pages: true,
            },
            pages_router: PagesRouterInfo {
                data_routes: vec![DataRoute {
                    page: "/ssr".into(),
                    regex: "^/_next/data/B/ssr\\.json$".into(),
                    dynamic: false,
                }],
                ..PagesRouterInfo::default()
            },
        };
        let adapter = adapter_output::AdapterPages {
            functions: vec![
                (
                    RouteKind::PagesPage,
                    "/ssr".into(),
                    "/b/server/pages/ssr.js".into(),
                ),
                (
                    RouteKind::PagesPage,
                    "/_next/data/B/ssr.json".into(),
                    "/b/server/pages/ssr.js".into(),
                ),
                (
                    RouteKind::PagesApi,
                    "/api/x".into(),
                    "/b/server/pages/api/x.js".into(),
                ),
            ],
            prerenders: Vec::new(),
        };
        (internal, adapter)
    }

    fn refused(
        internal: &internal::InternalTables,
        adapter: &adapter_output::AdapterPages,
    ) -> Option<String> {
        let mut diagnostics = Vec::new();
        check_adapter_pages(internal, adapter, "B", None, &mut diagnostics);
        diagnostics
            .into_iter()
            .find(|d| d.code == "pages-adapter-mismatch" && d.severity == Severity::Unsupported)
            .map(|d| d.message)
    }

    #[test]
    fn consistent_tables_pass() {
        let (internal, adapter) = tables();
        assert_eq!(refused(&internal, &adapter), None);
    }

    #[test]
    fn an_adapter_route_the_model_lacks_refuses() {
        let (mut internal, adapter) = tables();
        internal.routes.retain(|r| r.page != "/api/x");
        assert!(
            refused(&internal, &adapter)
                .unwrap()
                .contains("/api/x: no such route")
        );
    }

    #[test]
    fn a_route_the_adapter_did_not_report_refuses() {
        let (internal, mut adapter) = tables();
        adapter.functions.retain(|(_, p, _)| p != "/ssr");
        assert!(
            refused(&internal, &adapter)
                .unwrap()
                .contains("/ssr: not in the adapter output")
        );
    }

    #[test]
    fn a_different_entry_or_data_route_refuses() {
        let (internal, mut adapter) = tables();
        adapter.functions[2].2 = "/b/server/pages/api/other.js".into();
        assert!(
            refused(&internal, &adapter)
                .unwrap()
                .contains("/api/x: kind or entry differs")
        );
        let (internal, mut adapter) = tables();
        adapter.functions.push((
            RouteKind::PagesPage,
            "/_next/data/B/gone.json".into(),
            "/b/server/pages/gone.js".into(),
        ));
        assert!(
            refused(&internal, &adapter)
                .unwrap()
                .contains("no page with getServerSideProps")
        );
    }

    #[test]
    fn a_stored_output_elsewhere_refuses() {
        let (mut internal, mut adapter) = tables();
        internal.prerenders.push(Prerender {
            pathname: "/ssg".into(),
            route: "/ssg".into(),
            status: 200,
            revalidate: None,
            html: Some(StoredResponse {
                file: "/b/server/pages/ssg.html".into(),
                headers: Vec::new(),
            }),
            rsc: None,
            segments: Vec::new(),
            postponed: false,
            pages: true,
            data: None,
            locale: None,
        });
        adapter.prerenders.push((
            "/ssg".into(),
            Some("/b/server/route-cache/PAGES/x/$/ssg.html".into()),
            None,
        ));
        assert!(
            refused(&internal, &adapter)
                .unwrap()
                .contains("/ssg: stored output differs")
        );
    }
}

#[cfg(test)]
mod workspace_root_tests {
    //! `relativeAppDir` from required-server-files.json gives the monorepo
    //! root only when the served directory really ends with it.
    use super::*;

    fn root(project: &str, relative: Option<&str>) -> (Option<PathBuf>, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let root = workspace_root(Path::new(project), relative, &mut diagnostics);
        (root, diagnostics)
    }

    #[test]
    fn single_package_projects_have_no_workspace_root() {
        assert_eq!(root("/srv/app", None), (None, vec![]));
        assert_eq!(root("/srv/app", Some("")), (None, vec![]));
    }

    #[test]
    fn monorepo_and_standalone_layouts() {
        assert_eq!(
            root("/w/repo/apps/web", Some("apps/web")).0,
            Some(PathBuf::from("/w/repo"))
        );
        // output: 'standalone' reproduces the layout under .next/standalone.
        assert_eq!(
            root(
                "/w/repo/apps/web/.next/standalone/apps/web",
                Some("apps/web")
            )
            .0,
            Some(PathBuf::from("/w/repo/apps/web/.next/standalone"))
        );
        assert_eq!(
            root("/w/repo/web", Some("web/")).0,
            Some(PathBuf::from("/w/repo"))
        );
    }

    #[test]
    fn mismatched_or_unsafe_relative_dirs_are_refused_with_a_warning() {
        for (project, relative) in [
            ("/w/repo/apps/site", "apps/web"),
            ("/w/repo/apps/web", "../apps/web"),
            ("/w/repo/apps/web", "apps/../web"),
            ("/web", "x/web"),
        ] {
            let (found, diagnostics) = root(project, Some(relative));
            assert_eq!(found, None, "{project} {relative}");
            assert_eq!(diagnostics.len(), 1, "{project} {relative}");
            assert_eq!(diagnostics[0].code, "workspace-root");
        }
    }
}

#[cfg(test)]
mod version_support_tests {
    use super::*;

    #[test]
    fn the_tested_line_is_tested() {
        for v in ["16.4.0", "16.4.3", "16.4.6", "v16.4.6", "16.4.7-canary.2"] {
            assert_eq!(version_support(v), VersionSupport::Tested, "{v}");
        }
    }

    #[test]
    fn other_minors_of_the_supported_major_are_untested_not_refused() {
        for v in ["16.0.0", "16.3.9", "16.5.0", "16.5.0-canary.3", "16.10.1"] {
            assert_eq!(version_support(v), VersionSupport::Untested, "{v}");
        }
        // "16.40" is not the 16.4 line.
        assert_eq!(version_support("16.40.0"), VersionSupport::Untested);
    }

    #[test]
    fn other_majors_and_garbage_are_unsupported() {
        for v in ["15.5.12", "14.2.0", "17.0.0", "", "latest", "16", "x.4.6"] {
            assert_eq!(version_support(v), VersionSupport::Unsupported, "{v}");
        }
    }
}
