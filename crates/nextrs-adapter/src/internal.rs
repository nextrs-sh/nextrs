//! Builds the route table from the manifests `next start` itself reads.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::manifests::{
    MiddlewareManifest, PrerenderManifest, PrerenderMeta, RoutesManifest, flatten_headers,
    normalize_page_path, read_json, read_json_optional, revalidate_seconds, with_suffix,
};
use crate::{
    DataRoute, Diagnostic, InspectError, Matcher, PagesRouterInfo, Prerender, Rendering, Route,
    RouteKind, Routers, Runtime, Severity, StoredResponse,
};

/// Pages Router entries that are framework plumbing, not routes
/// (next start's BLOCKED_PAGES: requesting one renders the 404 page).
const PAGES_INTERNALS: &[&str] = &["/_app", "/_document", "/_error"];
/// Status pages: routes only in a build with Pages Router routes (an App
/// Router build carries Next's default `404.html`/`500.html` too).
const PAGES_STATUS: &[&str] = &["/404", "/500"];

pub(crate) struct InternalTables {
    pub routes: Vec<Route>,
    pub prerenders: Vec<Prerender>,
    pub routers: Routers,
    pub pages_router: PagesRouterInfo,
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

    let has_pages_routes = pages.keys().any(|page| {
        !PAGES_INTERNALS.contains(&page.as_str()) && !PAGES_STATUS.contains(&page.as_str())
    });
    let mut pages_router = PagesRouterInfo::default();
    let mut pages_prerenders = Vec::new();
    if has_pages_routes {
        for (page, file) in &pages {
            let entry = server_dir.join(file);
            if page == "/_error" {
                pages_router.error_entry = Some(entry);
                continue;
            }
            if PAGES_INTERNALS.contains(&page.as_str()) {
                continue;
            }
            let kind = if page == "/api" || page.starts_with("/api/") {
                RouteKind::PagesApi
            } else {
                RouteKind::PagesPage
            };
            let is_html = file.ends_with(".html");
            let rendering = if is_html {
                Rendering::Static
            } else {
                classify(page, prerender_manifest)
            };
            if is_html {
                // A page without data functions: Next wrote its HTML at
                // build time and next start sends that file as it is.
                pages_prerenders.push(Prerender {
                    pathname: page.clone(),
                    route: page.clone(),
                    status: match page.as_str() {
                        "/404" => 404,
                        "/500" => 500,
                        _ => 200,
                    },
                    revalidate: None,
                    html: Some(StoredResponse {
                        file: entry.clone(),
                        headers: Vec::new(),
                    }),
                    rsc: None,
                    segments: Vec::new(),
                    postponed: false,
                    pages: true,
                    data: None,
                });
            }
            routes.push(Route {
                page: page.clone(),
                kind,
                rendering,
                runtime: Runtime::Nodejs,
                entry: Some(entry),
                matcher: matcher_for(page, routes_manifest, diagnostics),
                internal: false,
            });
        }
        for data in &routes_manifest.data_routes {
            pages_router.data_routes.push(DataRoute {
                page: data.page.clone(),
                regex: data.data_route_regex.clone(),
                dynamic: data.page.contains('['),
            });
        }
        let mut ssg: std::collections::BTreeSet<String> = prerender_manifest
            .routes
            .iter()
            .filter(|(_, entry)| {
                entry
                    .data_route
                    .as_deref()
                    .is_some_and(|d| d.ends_with(".json"))
            })
            .map(|(path, entry)| entry.src_route.clone().unwrap_or_else(|| path.clone()))
            .collect();
        for (page, entry) in &prerender_manifest.dynamic_routes {
            if entry
                .get("dataRoute")
                .and_then(|v| v.as_str())
                .is_some_and(|d| d.ends_with(".json"))
            {
                ssg.insert(page.clone());
            }
        }
        // getStaticProps pages that returned notFound at build time.
        ssg.extend(prerender_manifest.not_found_routes.iter().cloned());
        pages_router.ssg_pages = ssg.into_iter().collect();
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
            match resolve_pages_prerender(dist_dir, pathname)? {
                Some(mut prerender) => {
                    prerender.route = entry.src_route.clone().unwrap_or_else(|| pathname.clone());
                    prerender.status = entry.initial_status.unwrap_or(prerender.status);
                    prerender.revalidate = revalidate_seconds(&entry.initial_revalidate_seconds);
                    pages_prerenders.push(prerender);
                }
                None => diagnostics.push(Diagnostic {
                    severity: Severity::Warning,
                    code: "prerender-missing",
                    message: format!(
                        "prerender-manifest lists {pathname} but no stored files were found"
                    ),
                }),
            }
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

    prerenders.extend(pages_prerenders);
    Ok(InternalTables {
        routes,
        prerenders,
        routers: Routers {
            app: !app_paths.is_empty(),
            pages: has_pages_routes,
        },
        pages_router,
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
        postponed: meta.postponed.is_some_and(|p| !p.is_null()),
        pages: false,
        data: None,
    }))
}

/// The stored HTML and page data of a Pages Router `getStaticProps`
/// prerender: `server/pages/<name>.html`, `.json` and `.meta`.
fn resolve_pages_prerender(
    dist_dir: &Path,
    pathname: &str,
) -> Result<Option<Prerender>, InspectError> {
    let name = normalize_page_path(pathname);
    let base = dist_dir.join("server/pages").join(&name[1..]);
    let html = with_suffix(&base, ".html");
    if !html.is_file() {
        return Ok(None);
    }
    let meta: PrerenderMeta = read_json_optional(&with_suffix(&base, ".meta"))?.unwrap_or_default();
    let headers = flatten_headers(&meta.headers);
    let data = with_suffix(&base, ".json");
    Ok(Some(Prerender {
        pathname: pathname.to_owned(),
        route: pathname.to_owned(),
        status: meta.status.unwrap_or(200),
        revalidate: None,
        html: Some(StoredResponse {
            file: html,
            headers: headers.clone(),
        }),
        rsc: None,
        segments: Vec::new(),
        postponed: false,
        pages: true,
        data: data.is_file().then_some(StoredResponse {
            file: data,
            headers,
        }),
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
