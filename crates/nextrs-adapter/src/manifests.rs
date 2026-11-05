//! Serde views of the undocumented manifests in `.next`.
//!
//! Only fields NextRS reads are declared; unknown fields are ignored so that
//! additive changes in Next.js do not break parsing. Shapes were taken from
//! Next.js 16.4.3 build output.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::InspectError;

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, InspectError> {
    let bytes = std::fs::read(path).map_err(|source| InspectError::Io {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| InspectError::Json {
        path: path.to_owned(),
        source,
    })
}

pub(crate) fn read_json_optional<T: DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, InspectError> {
    if path.is_file() {
        read_json(path).map(Some)
    } else {
        Ok(None)
    }
}

/// `routes-manifest.json`
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoutesManifest {
    pub version: u32,
    #[serde(default)]
    pub base_path: String,
    #[serde(default)]
    pub redirects: Vec<RedirectEntry>,
    /// Either an array (legacy) or `{ beforeFiles, afterFiles, fallback }`.
    #[serde(default)]
    pub rewrites: Value,
    #[serde(default)]
    pub static_routes: Vec<RouteRegex>,
    #[serde(default)]
    pub dynamic_routes: Vec<RouteRegex>,
    #[serde(default)]
    pub i18n: Option<Value>,
    pub rsc: Option<RscManifest>,
}

impl RoutesManifest {
    pub fn rewrite_count(&self) -> usize {
        match &self.rewrites {
            Value::Array(items) => items.len(),
            Value::Object(groups) => groups
                .values()
                .map(|group| group.as_array().map_or(0, Vec::len))
                .sum(),
            _ => 0,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RedirectEntry {
    #[serde(default)]
    pub internal: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RouteRegex {
    pub page: String,
    pub regex: String,
    pub named_regex: Option<String>,
    #[serde(default)]
    pub route_keys: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RscManifest {
    pub header: String,
    pub vary_header: String,
    pub prefetch_header: String,
    pub content_type_header: String,
    pub suffix: String,
    pub prefetch_segment_header: Option<String>,
    pub prefetch_segment_suffix: Option<String>,
    pub prefetch_segment_dir_suffix: Option<String>,
}

impl From<RscManifest> for crate::RscConfig {
    fn from(rsc: RscManifest) -> Self {
        let defaults = crate::RscConfig::default();
        Self {
            header: rsc.header,
            vary_header: rsc.vary_header,
            prefetch_header: rsc.prefetch_header,
            content_type: rsc.content_type_header,
            suffix: rsc.suffix,
            segment_prefetch_header: rsc
                .prefetch_segment_header
                .unwrap_or(defaults.segment_prefetch_header),
            segment_suffix: rsc
                .prefetch_segment_suffix
                .unwrap_or(defaults.segment_suffix),
            segment_dir_suffix: rsc
                .prefetch_segment_dir_suffix
                .unwrap_or(defaults.segment_dir_suffix),
        }
    }
}

/// `prerender-manifest.json`
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrerenderManifest {
    pub version: u32,
    #[serde(default)]
    pub routes: BTreeMap<String, PrerenderRoute>,
    #[serde(default)]
    pub dynamic_routes: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrerenderRoute {
    pub initial_status: Option<u16>,
    /// `false` (never) or seconds.
    #[serde(default)]
    pub initial_revalidate_seconds: Value,
    pub src_route: Option<String>,
    pub data_route: Option<String>,
}

/// `false`/absent means "never revalidate".
pub(crate) fn revalidate_seconds(value: &Value) -> Option<u64> {
    value.as_u64()
}

/// `<name>.meta`, written next to each prerendered app page.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrerenderMeta {
    pub status: Option<u16>,
    #[serde(default)]
    pub headers: BTreeMap<String, Value>,
    #[serde(default)]
    pub segment_paths: Vec<String>,
    /// Partial Prerendering: the state Next resumes the dynamic parts from.
    #[serde(default)]
    pub postponed: Option<Value>,
}

/// `server/middleware-manifest.json`
#[derive(Debug, Default, Deserialize)]
pub(crate) struct MiddlewareManifest {
    #[serde(default)]
    pub middleware: BTreeMap<String, Value>,
    /// Edge-runtime route functions keyed by app path (e.g. `/edge/page`).
    #[serde(default)]
    pub functions: BTreeMap<String, Value>,
}

/// `server/functions-config-manifest.json`
#[derive(Debug, Default, Deserialize)]
pub(crate) struct FunctionsConfigManifest {
    #[serde(default)]
    pub functions: BTreeMap<String, Value>,
}

/// `server/server-reference-manifest.json` (Server Actions)
#[derive(Debug, Default, Deserialize)]
pub(crate) struct ServerReferenceManifest {
    #[serde(default)]
    pub node: BTreeMap<String, Value>,
    #[serde(default)]
    pub edge: BTreeMap<String, Value>,
}

/// `required-server-files.json`. `config` is the resolved next.config.
#[derive(Debug, Deserialize)]
pub(crate) struct RequiredServerFiles {
    pub config: Value,
}

/// Header maps in `.meta` files and adapter outputs hold strings or arrays.
pub(crate) fn flatten_headers(headers: &BTreeMap<String, Value>) -> Vec<(String, String)> {
    let mut flat = Vec::new();
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        match value {
            Value::String(v) => flat.push((name, v.clone())),
            Value::Array(items) => flat.extend(
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|v| (name.clone(), v.to_owned())),
            ),
            Value::Number(n) => flat.push((name, n.to_string())),
            _ => {}
        }
    }
    flat
}

/// Port of Next.js `normalizePagePath`: `/` -> `/index`, `/index/x` -> `/index/index/x`.
pub(crate) fn normalize_page_path(pathname: &str) -> String {
    if pathname == "/" {
        return "/index".to_owned();
    }
    if pathname == "/index" || pathname.starts_with("/index/") {
        return format!("/index{pathname}");
    }
    if pathname.starts_with('/') {
        pathname.to_owned()
    } else {
        format!("/{pathname}")
    }
}

pub(crate) fn with_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut path = base.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}
