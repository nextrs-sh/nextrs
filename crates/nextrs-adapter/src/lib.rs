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
mod internal;
mod manifests;
mod model;

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use error::InspectError;
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
    let internal = internal::load(
        &dist_dir,
        &routes_manifest,
        &prerender_manifest,
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

    let required: Option<RequiredServerFiles> =
        read_json_optional(&dist_dir.join("required-server-files.json"))?;
    let mut next_version = None;
    let (source, mut routes, mut prerenders, rsc, config) = match adapter {
        Some(output) => {
            next_version = Some(DetectedVersion {
                version: output.next_version.clone(),
                source: adapter_output::RELATIVE_PATH.into(),
            });
            let config = RoutingConfig {
                base_path: output.config.base_path.clone(),
                trailing_slash: output.config.trailing_slash,
                i18n: output.config.i18n.as_ref().is_some_and(|v| !v.is_null()),
                powered_by_header: output.config.powered_by_header,
                generate_etags: output.config.generate_etags,
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
    let mut info = BuildInfo {
        static_dir: dist_dir.join("static"),
        project_dir,
        dist_dir,
        build_id,
        next_version,
        source,
        routers: internal.routers,
        config,
        rsc,
        routes,
        prerenders,
        public_dir,
        diagnostics,
    };
    detect_features(&mut info, &routes_manifest)?;
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
        i18n: config
            .and_then(|c| c.get("i18n"))
            .is_some_and(|v| !v.is_null()),
        powered_by_header: flag("poweredByHeader", true),
        generate_etags: flag("generateEtags", true),
    }
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

    if !middleware.middleware.is_empty() || functions.functions.contains_key("/_middleware") {
        push(
            Severity::Unsupported,
            "middleware",
            "middleware/proxy is present; serving routes without running it could bypass its logic"
                .into(),
        );
    }
    let custom_redirects = routes_manifest
        .redirects
        .iter()
        .filter(|r| !r.internal)
        .count();
    if custom_redirects > 0 {
        push(
            Severity::Unsupported,
            "custom-redirects",
            format!("{custom_redirects} next.config redirects are not applied"),
        );
    }
    if !routes_manifest.headers.is_empty() {
        push(
            Severity::Unsupported,
            "custom-headers",
            format!(
                "{} next.config header rules (often security headers) are not applied",
                routes_manifest.headers.len()
            ),
        );
    }
    if routes_manifest.rewrite_count() > 0 {
        push(
            Severity::Unsupported,
            "rewrites",
            format!(
                "{} next.config rewrites are not applied",
                routes_manifest.rewrite_count()
            ),
        );
    }
    if !info.config.base_path.is_empty() {
        push(
            Severity::Unsupported,
            "base-path",
            format!("basePath {:?}", info.config.base_path),
        );
    }
    if info.config.i18n || routes_manifest.i18n.is_some() {
        push(Severity::Unsupported, "i18n", "i18n routing".into());
    }
    if info.config.trailing_slash {
        push(
            Severity::Unsupported,
            "trailing-slash",
            "trailingSlash: true".into(),
        );
    }
    if info.routers.pages {
        push(
            Severity::Unsupported,
            "pages-router",
            "Pages Router routes are classified but not served in Phase 0".into(),
        );
    }
    let action_count = actions.node.len() + actions.edge.len();
    if action_count > 0 {
        push(
            Severity::Unsupported,
            "server-actions",
            format!("{action_count} Server Actions require JavaScript execution"),
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
    push(
        Severity::Info,
        "image-optimizer",
        "/_next/image is not implemented".into(),
    );
    Ok(())
}
