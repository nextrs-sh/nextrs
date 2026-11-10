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

/// Next.js release line the Phase 0 parsers were written against.
pub const TESTED_NEXT_VERSION: &str = "16.4";

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
            (
                MetadataSource::AdapterApi,
                tables.routes,
                tables.prerenders,
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
        Some(v) if !v.version.starts_with(&format!("{TESTED_NEXT_VERSION}.")) => {
            diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code: "untested-next-version",
                message: format!(
                    "Next.js {} has not been tested; Phase 0 targets {TESTED_NEXT_VERSION}.x",
                    v.version
                ),
            });
        }
        None => diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "unknown-next-version",
            message: "could not determine the Next.js version".into(),
        }),
        _ => {}
    }

    routes.sort_by(|a, b| a.page.cmp(&b.page));
    prerenders.sort_by(|a, b| a.pathname.cmp(&b.pathname));
    let public_dir = Some(project_dir.join("public")).filter(|dir| dir.is_dir());
    let mut invalid_rules = Vec::new();
    let mut routing = routing::routing_rules(&dist_dir, &mut invalid_rules)?;
    routing.proxy_timeout_ms = routing::proxy_timeout_ms(required.as_ref().map(|r| &r.config));
    let proxy = routing::proxy_info(&dist_dir, required.as_ref().map(|r| &r.config))?;
    let images = images::images_config(&dist_dir, required.as_ref().map(|r| &r.config))?;
    for rule in invalid_rules {
        diagnostics.push(Diagnostic {
            severity: Severity::Unsupported,
            code: "routing-rule",
            message: format!("routes-manifest rule without source or regex: {rule}"),
        });
    }
    let mut info = BuildInfo {
        static_dir: dist_dir.join("static"),
        project_dir,
        dist_dir,
        build_id,
        next_version,
        source,
        routers: internal.routers,
        pages_router: internal.pages_router.clone(),
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
        } else if info.source == MetadataSource::AdapterApi {
            push(
                Severity::Unsupported,
                "i18n-adapter-output",
                "Next's i18n config is not supported with deployment-adapter output; build without NEXT_ADAPTER_PATH".into(),
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
        if info.source == MetadataSource::AdapterApi {
            // The adapter's prerender outputs carry the HTML but not the
            // page data NextRS serves for /_next/data requests.
            push(
                Severity::Unsupported,
                "pages-router-adapter-output",
                "Pages Router routes from deployment-adapter output are not supported; build without NEXT_ADAPTER_PATH".into(),
            );
        } else {
            push(
                Severity::Info,
                "pages-router",
                "Pages Router: pages without data functions are served as built; getServerSideProps, getStaticProps, API routes and /_next/data run Next's compiled handlers".into(),
            );
        }
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
            "no images-manifest.json: /_next/image answers 404".into(),
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
