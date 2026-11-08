//! Reads `.next/nextrs/build-output.json`, the file `@nextrs/next-adapter`
//! writes from the public `NextAdapter.onBuildComplete` context.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::manifests::{RscManifest, flatten_headers, read_json, revalidate_seconds};
use crate::{
    Diagnostic, InspectError, Matcher, Prerender, Rendering, Route, RouteKind, RscConfig, Runtime,
    Severity, StoredResponse,
};

pub(crate) const RELATIVE_PATH: &str = "nextrs/build-output.json";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildOutput {
    pub schema_version: u32,
    pub next_version: String,
    pub build_id: String,
    pub config: AdapterConfig,
    pub routing: Routing,
    pub outputs: Outputs,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdapterConfig {
    #[serde(default)]
    pub base_path: String,
    #[serde(default)]
    pub trailing_slash: bool,
    #[serde(default)]
    pub i18n: Option<Value>,
    #[serde(default = "yes")]
    pub powered_by_header: bool,
    #[serde(default = "yes")]
    pub generate_etags: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Routing {
    #[serde(default)]
    pub before_middleware: Vec<RoutingRule>,
    #[serde(default)]
    pub before_files: Vec<RoutingRule>,
    #[serde(default)]
    pub after_files: Vec<RoutingRule>,
    #[serde(default)]
    pub dynamic_routes: Vec<RoutingRule>,
    #[serde(default)]
    pub fallback: Vec<RoutingRule>,
    pub rsc: RscManifest,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoutingRule {
    pub source: Option<String>,
    pub source_regex: String,
    pub status: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Outputs {
    #[serde(default)]
    pub pages: Vec<FunctionOutput>,
    #[serde(default)]
    pub pages_api: Vec<FunctionOutput>,
    #[serde(default)]
    pub app_pages: Vec<FunctionOutput>,
    #[serde(default)]
    pub app_routes: Vec<FunctionOutput>,
    #[serde(default)]
    pub prerenders: Vec<PrerenderOutput>,
    pub middleware: Option<FunctionOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FunctionOutput {
    pub pathname: String,
    pub runtime: String,
    pub file_path: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrerenderOutput {
    pub pathname: String,
    pub parent_output_id: String,
    pub route: Option<String>,
    pub fallback: Option<Fallback>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Fallback {
    pub file_path: Option<PathBuf>,
    pub initial_status: Option<u16>,
    #[serde(default)]
    pub initial_headers: BTreeMap<String, Value>,
    #[serde(default)]
    pub initial_revalidate: Value,
}

pub(crate) struct AdapterTables {
    pub routes: Vec<Route>,
    pub prerenders: Vec<Prerender>,
    pub rsc: RscConfig,
}

pub(crate) fn read(dist_dir: &Path) -> Result<Option<BuildOutput>, InspectError> {
    let path = dist_dir.join(RELATIVE_PATH);
    if !path.is_file() {
        return Ok(None);
    }
    let output: BuildOutput = read_json(&path)?;
    if output.schema_version != SCHEMA_VERSION {
        return Err(InspectError::Invalid {
            path,
            message: format!("unsupported schema version {}", output.schema_version),
        });
    }
    Ok(Some(output))
}

pub(crate) fn tables(
    project_dir: &Path,
    output: BuildOutput,
    diagnostics: &mut Vec<Diagnostic>,
) -> AdapterTables {
    let rsc: RscConfig = output.routing.rsc.into();
    let prerenders = group_prerenders(project_dir, &output.outputs.prerenders, &rsc);

    let function_outputs = [
        (RouteKind::AppPage, &output.outputs.app_pages),
        (RouteKind::AppRoute, &output.outputs.app_routes),
        (RouteKind::PagesPage, &output.outputs.pages),
        (RouteKind::PagesApi, &output.outputs.pages_api),
    ];
    let mut routes = Vec::new();
    for (kind, outputs) in function_outputs {
        // Each page is also emitted as `<pathname>.rsc`; the RSC variant is
        // the same entry and is handled by the page's own route.
        for function in outputs
            .iter()
            .filter(|f| !f.pathname.ends_with(&rsc.suffix))
        {
            let prerender = prerenders.iter().find(|p| p.route == function.pathname);
            let rendering = match prerender {
                Some(p) => match p.revalidate {
                    Some(seconds) => Rendering::Revalidated { seconds },
                    None => Rendering::Static,
                },
                None => Rendering::Dynamic,
            };
            let matcher = output
                .routing
                .dynamic_routes
                .iter()
                .find(|rule| rule.source.as_deref() == Some(function.pathname.as_str()))
                .map(|rule| Matcher::Pattern {
                    regex: rule.source_regex.clone(),
                    params: named_groups(&rule.source_regex),
                })
                .unwrap_or_else(|| Matcher::Exact {
                    pathname: function.pathname.clone(),
                });
            routes.push(Route {
                page: function.pathname.clone(),
                kind,
                rendering,
                runtime: if function.runtime == "edge" {
                    Runtime::Edge
                } else {
                    Runtime::Nodejs
                },
                entry: Some(project_dir.join(&function.file_path)),
                matcher,
                internal: function.pathname.starts_with("/_"),
            });
        }
    }

    // Routing rules and the proxy are read from routes-manifest.json and
    // functions-config-manifest.json whatever the metadata source (the
    // manifests next start applies); the adapter's routing view is only
    // compared with them.
    let before_middleware = output
        .routing
        .before_middleware
        .iter()
        // The trailing-slash redirect Next.js always adds.
        .filter(|rule| !(rule.source.as_deref() == Some("/:path+/") && rule.status == Some(308)))
        .count();
    let adapter_rules = before_middleware
        + output.routing.before_files.len()
        + output.routing.after_files.len()
        + output.routing.fallback.len();
    if adapter_rules > 0 {
        diagnostics.push(Diagnostic {
            severity: Severity::Info,
            code: "adapter-routing-rules",
            message: format!(
                "adapter reports {adapter_rules} routing rules; NextRS applies the routes-manifest.json copies"
            ),
        });
    }
    if let Some(proxy) = &output.outputs.middleware {
        diagnostics.push(Diagnostic {
            severity: Severity::Info,
            code: "adapter-proxy-output",
            message: format!(
                "adapter reports the proxy at {} ({}); NextRS runs server/middleware.js as next start does",
                proxy.file_path.display(),
                proxy.runtime
            ),
        });
    }

    AdapterTables {
        routes,
        prerenders,
        rsc,
    }
}

/// Prerender outputs come as one entry per stored response (`/`,
/// `/index.rsc`, `/index.segments/_tree.segment.rsc`, ...); group them by the
/// page output they belong to.
fn group_prerenders(
    project_dir: &Path,
    outputs: &[PrerenderOutput],
    rsc: &RscConfig,
) -> Vec<Prerender> {
    let mut groups: BTreeMap<&str, Prerender> = BTreeMap::new();
    for output in outputs {
        let Some(fallback) = &output.fallback else {
            continue;
        };
        let Some(file) = &fallback.file_path else {
            continue;
        };
        let group = groups
            .entry(output.parent_output_id.as_str())
            .or_insert_with(|| Prerender {
                pathname: output.parent_output_id.clone(),
                route: output
                    .route
                    .clone()
                    .unwrap_or_else(|| output.parent_output_id.clone()),
                status: 200,
                revalidate: None,
                html: None,
                rsc: None,
                segments: Vec::new(),
                postponed: false,
                pages: false,
                data: None,
            });
        let stored = StoredResponse {
            file: project_dir.join(file),
            headers: flatten_headers(&fallback.initial_headers)
                .into_iter()
                .filter(|(name, _)| name != "content-type" && name != "vary")
                .collect(),
        };
        let segment_marker = format!("{}/", rsc.segment_dir_suffix);
        if let Some(start) = output.pathname.find(&segment_marker)
            && let Some(segment) = output.pathname[start + segment_marker.len() - 1..]
                .strip_suffix(&rsc.segment_suffix)
        {
            // initialHeaders for segments include x-nextjs-stale-time, but
            // `next start` 16.4.3 does not send it on segment responses.
            let stored = StoredResponse {
                headers: stored
                    .headers
                    .into_iter()
                    .filter(|(name, _)| name != "x-nextjs-stale-time")
                    .collect(),
                ..stored
            };
            group.segments.push((segment.to_owned(), stored));
        } else if output.pathname.ends_with(&rsc.suffix) {
            group.rsc = Some(stored);
        } else {
            group.status = fallback.initial_status.unwrap_or(200);
            group.revalidate = revalidate_seconds(&fallback.initial_revalidate);
            group.html = Some(stored);
        }
    }
    groups.into_values().collect()
}

fn named_groups(regex: &str) -> Vec<String> {
    regex::Regex::new(regex)
        .map(|re| re.capture_names().flatten().map(str::to_owned).collect())
        .unwrap_or_default()
}
