//! Builds the route table from the manifests `next start` itself reads.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::manifests::{
    MiddlewareManifest, PrerenderManifest, PrerenderMeta, RoutesManifest, flatten_headers,
    normalize_page_path, read_json, read_json_optional, revalidate_seconds, with_suffix,
};
use crate::{
    Diagnostic, InspectError, Matcher, Prerender, Rendering, Route, RouteKind, Routers, Runtime,
    Severity, StoredResponse,
};

/// Pages Router entries that are framework plumbing, not routes.
const PAGES_INTERNALS: &[&str] = &["/_app", "/_document", "/_error", "/404", "/500"];

pub(crate) struct InternalTables {
    pub routes: Vec<Route>,
    pub prerenders: Vec<Prerender>,
    pub routers: Routers,
}

pub(crate) fn load(
    dist_dir: &Path,
    routes_manifest: &RoutesManifest,
    prerender_manifest: &PrerenderManifest,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<InternalTables, InspectError> {
    let server_dir = dist_dir.join("server");
    let app_paths: BTreeMap<String, String> =
        read_json_optional(&server_dir.join("app-paths-manifest.json"))?.unwrap_or_default();
    let app_path_routes: BTreeMap<String, String> =
        read_json_optional(&dist_dir.join("app-path-routes-manifest.json"))?.unwrap_or_default();
    let pages: BTreeMap<String, String> =
        read_json_optional(&server_dir.join("pages-manifest.json"))?.unwrap_or_default();
    let middleware: MiddlewareManifest =
        read_json_optional(&server_dir.join("middleware-manifest.json"))?.unwrap_or_default();

    let mut routes = Vec::new();
    for (app_path, route) in &app_path_routes {
        let kind = if app_path.ends_with("/route") {
            RouteKind::AppRoute
        } else {
            RouteKind::AppPage
        };
        let rendering = classify(route, prerender_manifest);
        routes.push(Route {
            page: route.clone(),
            kind,
            rendering,
            runtime: if middleware.functions.contains_key(app_path) {
                Runtime::Edge
            } else {
                Runtime::Nodejs
            },
            entry: app_paths.get(app_path).map(|file| server_dir.join(file)),
            matcher: matcher_for(route, routes_manifest, diagnostics),
            internal: route.starts_with("/_"),
        });
    }

    let mut has_pages_routes = false;
    for (page, file) in &pages {
        if PAGES_INTERNALS.contains(&page.as_str()) {
            continue;
        }
        has_pages_routes = true;
        let kind = if page == "/api" || page.starts_with("/api/") {
            RouteKind::PagesApi
        } else {
            RouteKind::PagesPage
        };
        let rendering = if file.ends_with(".html") {
            Rendering::Static
        } else {
            classify(page, prerender_manifest)
        };
        routes.push(Route {
            page: page.clone(),
            kind,
            rendering,
            runtime: Runtime::Nodejs,
            entry: Some(server_dir.join(file)),
            matcher: matcher_for(page, routes_manifest, diagnostics),
            internal: false,
        });
    }

    // Reverse map: route -> app path, needed to locate route-cache files.
    let app_path_for: BTreeMap<&str, &str> = app_path_routes
        .iter()
        .map(|(app_path, route)| (route.as_str(), app_path.as_str()))
        .collect();

    let mut prerenders = Vec::new();
    for (pathname, entry) in &prerender_manifest.routes {
        let is_app = entry
            .data_route
            .as_deref()
            .is_none_or(|data| data.ends_with(".rsc"));
        if !is_app {
            diagnostics.push(Diagnostic {
                severity: Severity::Unsupported,
                code: "pages-prerender",
                message: format!("Pages Router prerender {pathname} is not served in Phase 0"),
            });
            continue;
        }
        let route = entry.src_route.clone().unwrap_or_else(|| pathname.clone());
        let app_path = app_path_for.get(route.as_str()).copied();
        match resolve_app_prerender(dist_dir, pathname, app_path)? {
            Some(mut prerender) => {
                prerender.route = route;
                prerender.status = entry.initial_status.unwrap_or(prerender.status);
                prerender.revalidate = revalidate_seconds(&entry.initial_revalidate_seconds);
                prerenders.push(prerender);
            }
            None => diagnostics.push(Diagnostic {
                severity: Severity::Warning,
                code: "prerender-missing",
                message: format!(
                    "prerender-manifest lists {pathname} but no stored files were found"
                ),
            }),
        }
    }

    Ok(InternalTables {
        routes,
        prerenders,
        routers: Routers {
            app: !app_paths.is_empty(),
            pages: has_pages_routes,
        },
    })
}

fn classify(route: &str, prerender_manifest: &PrerenderManifest) -> Rendering {
    if prerender_manifest.dynamic_routes.contains_key(route) {
        return Rendering::Unknown {
            reason: "dynamic route with prerendered params (generateStaticParams); fallback behaviour is not modelled yet".into(),
        };
    }
    match prerender_manifest.routes.get(route) {
        Some(entry) => match revalidate_seconds(&entry.initial_revalidate_seconds) {
            Some(seconds) => Rendering::Revalidated { seconds },
            None => Rendering::Static,
        },
        None => Rendering::Dynamic,
    }
}

fn matcher_for(
    page: &str,
    routes_manifest: &RoutesManifest,
    diagnostics: &mut Vec<Diagnostic>,
) -> Matcher {
    if let Some(dynamic) = routes_manifest
        .dynamic_routes
        .iter()
        .find(|r| r.page == page)
    {
        return Matcher::Pattern {
            regex: dynamic
                .named_regex
                .clone()
                .unwrap_or_else(|| dynamic.regex.clone()),
            params: dynamic.route_keys.keys().cloned().collect(),
        };
    }
    if !routes_manifest.static_routes.iter().any(|r| r.page == page) {
        diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            code: "route-not-in-routes-manifest",
            message: format!("{page} is not listed in routes-manifest.json; matching it exactly"),
        });
    }
    Matcher::Exact {
        pathname: page.to_owned(),
    }
}

/// Locates the stored HTML/RSC/segment files for a prerendered app page.
///
/// Next.js 16.4 writes them to `server/app/<name>.*` by default, and to
/// `server/route-cache/APP_PAGE/<sha256(app path)>/$<name>.*` when a
/// deployment adapter is configured (`getRouteCacheKey` in
/// `next/dist/server/lib/route-cache-key.js`).
pub(crate) fn resolve_app_prerender(
    dist_dir: &Path,
    pathname: &str,
    app_path: Option<&str>,
) -> Result<Option<Prerender>, InspectError> {
    let name = normalize_page_path(pathname);
    let mut candidates = vec![dist_dir.join("server/app").join(&name[1..])];
    if let Some(app_path) = app_path {
        let owner = hex(&Sha256::digest(app_path.as_bytes()));
        candidates.push(
            dist_dir
                .join("server/route-cache/APP_PAGE")
                .join(owner)
                .join(format!("${name}")),
        );
    }
    let Some(base) = candidates
        .into_iter()
        .find(|base| with_suffix(base, ".html").is_file() || with_suffix(base, ".meta").is_file())
    else {
        return Ok(None);
    };

    let meta: PrerenderMeta = read_json_optional(&with_suffix(&base, ".meta"))?.unwrap_or_default();
    let headers = flatten_headers(&meta.headers);
    let stored = |suffix: &str| -> Option<StoredResponse> {
        let file = with_suffix(&base, suffix);
        file.is_file().then(|| StoredResponse {
            file,
            headers: headers.clone(),
        })
    };
    // `next start` 16.4.3 sends segment prefetches without the stale-time
    // header and with `x-nextjs-postponed: 2` (the adapter API's
    // `initialHeaders` for segments agree); the `.meta` file only stores the
    // page-level headers.
    let segment_headers: Vec<(String, String)> = headers
        .iter()
        .filter(|(name, _)| name != "x-nextjs-stale-time")
        .cloned()
        .chain([("x-nextjs-postponed".to_owned(), "2".to_owned())])
        .collect();
    let segments = meta
        .segment_paths
        .iter()
        .filter_map(|segment| {
            let file = with_suffix(&base, &format!(".segments{segment}.segment.rsc"));
            file.is_file().then(|| {
                let response = StoredResponse {
                    file,
                    headers: segment_headers.clone(),
                };
                (segment.clone(), response)
            })
        })
        .collect();

    Ok(Some(Prerender {
        pathname: pathname.to_owned(),
        route: pathname.to_owned(),
        status: meta.status.unwrap_or(200),
        revalidate: None,
        html: stored(".html"),
        rsc: stored(".rsc"),
        segments,
    }))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn load_routes_manifest(dist_dir: &Path) -> Result<RoutesManifest, InspectError> {
    let path = dist_dir.join("routes-manifest.json");
    let manifest: RoutesManifest = read_json(&path)?;
    if manifest.version != 3 {
        return Err(InspectError::Invalid {
            path,
            message: format!("unsupported routes-manifest version {}", manifest.version),
        });
    }
    Ok(manifest)
}

pub(crate) fn load_prerender_manifest(dist_dir: &Path) -> Result<PrerenderManifest, InspectError> {
    let path: PathBuf = dist_dir.join("prerender-manifest.json");
    let manifest: PrerenderManifest = read_json(&path)?;
    if manifest.version != 4 {
        return Err(InspectError::Invalid {
            path,
            message: format!(
                "unsupported prerender-manifest version {}",
                manifest.version
            ),
        });
    }
    Ok(manifest)
}
