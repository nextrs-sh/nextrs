use std::path::PathBuf;

use serde::Serialize;

/// Everything NextRS knows about one `next build` output.
#[derive(Debug, Clone, Serialize)]
pub struct BuildInfo {
    pub project_dir: PathBuf,
    pub dist_dir: PathBuf,
    pub build_id: String,
    pub next_version: Option<DetectedVersion>,
    pub source: MetadataSource,
    pub routers: Routers,
    pub config: RoutingConfig,
    pub rsc: RscConfig,
    pub routes: Vec<Route>,
    pub prerenders: Vec<Prerender>,
    /// `.next/static`, served under `/_next/static/`.
    pub static_dir: PathBuf,
    /// `<project>/public`, when present.
    pub public_dir: Option<PathBuf>,
    pub server_actions: ServerActionsInfo,
    pub diagnostics: Vec<Diagnostic>,
}

/// Server Actions in the build, and the `experimental.serverActions`
/// options Next's action handler enforces at runtime.
#[derive(Debug, Clone, Serialize)]
pub struct ServerActionsInfo {
    /// Node.js-runtime actions in `server-reference-manifest.json`.
    pub count: usize,
    /// Edge-runtime actions (not run by NextRS).
    pub edge_count: usize,
    /// `bodySizeLimit` in bytes, parsed as Next parses it (the `bytes`
    /// package: 1 MB = 1024 * 1024). Next's default is 1 MB.
    pub body_size_limit: u64,
    /// The configured value, when not the default.
    pub body_size_limit_config: Option<String>,
    pub allowed_origins: Vec<String>,
}

impl Default for ServerActionsInfo {
    fn default() -> Self {
        Self {
            count: 0,
            edge_count: 0,
            body_size_limit: DEFAULT_ACTION_BODY_LIMIT,
            body_size_limit_config: None,
            allowed_origins: Vec::new(),
        }
    }
}

/// Next's default `serverActions.bodySizeLimit` ("1 MB").
pub const DEFAULT_ACTION_BODY_LIMIT: u64 = 1024 * 1024;

/// Parses a size the way the `bytes` npm package does (`500kb`, `2mb`,
/// `1.5 GB`, a plain number of bytes). Returns `None` for what it rejects.
pub fn parse_byte_size(value: &str) -> Option<u64> {
    let value = value.trim();
    let split = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '+'))
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f64 = number.parse().ok()?;
    let multiplier: f64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1024.0,
        "mb" => 1024.0 * 1024.0,
        "gb" => 1024.0 * 1024.0 * 1024.0,
        "tb" => 1024f64.powi(4),
        "pb" => 1024f64.powi(5),
        _ => return None,
    };
    (number.is_finite() && number >= 0.0).then(|| (number * multiplier).floor() as u64)
}

impl BuildInfo {
    pub fn prerender(&self, pathname: &str) -> Option<&Prerender> {
        self.prerenders.iter().find(|p| p.pathname == pathname)
    }

    pub fn route(&self, page: &str) -> Option<&Route> {
        self.routes.iter().find(|r| r.page == page)
    }

    pub fn has_unsupported(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Unsupported)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DetectedVersion {
    pub version: String,
    /// Which file the version was read from.
    pub source: String,
}

/// Where route metadata came from.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MetadataSource {
    /// `.next/nextrs/build-output.json`, written by `@nextrs/next-adapter`
    /// through the public Deployment Adapter API (`NextAdapter.onBuildComplete`).
    AdapterApi,
    /// Undocumented manifests in `.next` (`routes-manifest.json`, ...).
    InternalManifests,
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
pub struct Routers {
    pub app: bool,
    pub pages: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RoutingConfig {
    pub base_path: String,
    pub trailing_slash: bool,
    pub i18n: bool,
    pub powered_by_header: bool,
    pub generate_etags: bool,
}

/// RSC request/response conventions from `routes-manifest.json#rsc`.
#[derive(Debug, Clone, Serialize)]
pub struct RscConfig {
    pub header: String,
    pub vary_header: String,
    pub prefetch_header: String,
    pub content_type: String,
    pub suffix: String,
    pub segment_prefetch_header: String,
    pub segment_suffix: String,
    pub segment_dir_suffix: String,
}

impl Default for RscConfig {
    fn default() -> Self {
        Self {
            header: "rsc".into(),
            vary_header:
                "rsc, next-router-state-tree, next-router-prefetch, next-router-segment-prefetch"
                    .into(),
            prefetch_header: "next-router-prefetch".into(),
            content_type: "text/x-component".into(),
            suffix: ".rsc".into(),
            segment_prefetch_header: "next-router-segment-prefetch".into(),
            segment_suffix: ".segment.rsc".into(),
            segment_dir_suffix: ".segments".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Route {
    /// Route as written in the app, e.g. `/books/[id]`.
    pub page: String,
    pub kind: RouteKind,
    pub rendering: Rendering,
    pub runtime: Runtime,
    /// Compiled server entry, e.g. `.next/server/app/books/[id]/page.js`.
    pub entry: Option<PathBuf>,
    pub matcher: Matcher,
    /// Framework-provided routes such as `/_not-found`.
    pub internal: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RouteKind {
    AppPage,
    AppRoute,
    PagesPage,
    PagesApi,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Rendering {
    /// Fully prerendered; no request-time compute.
    Static,
    /// Prerendered, regenerated in the background after `seconds` (ISR).
    Revalidated { seconds: u64 },
    /// Rendered per request by the server bundle.
    Dynamic,
    /// The metadata does not settle the classification.
    Unknown { reason: String },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Runtime {
    Nodejs,
    Edge,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Matcher {
    /// Matches one pathname (with or without a trailing slash).
    Exact { pathname: String },
    /// Next.js-generated regex with named groups for the route params.
    Pattern { regex: String, params: Vec<String> },
}

/// One prerendered URL and its stored responses.
#[derive(Debug, Clone, Serialize)]
pub struct Prerender {
    pub pathname: String,
    /// Route the prerender was produced from, e.g. `/`.
    pub route: String,
    pub status: u16,
    pub revalidate: Option<u64>,
    pub html: Option<StoredResponse>,
    pub rsc: Option<StoredResponse>,
    /// Segment prefetch responses keyed by segment path, e.g. `/_tree`.
    pub segments: Vec<(String, StoredResponse)>,
    /// A Partial Prerendering shell: the stored HTML is incomplete, and
    /// Next's handler resumes its dynamic parts on every request.
    #[serde(default)]
    pub postponed: bool,
}

impl Prerender {
    pub fn segment(&self, path: &str) -> Option<&StoredResponse> {
        self.segments
            .iter()
            .find(|(segment, _)| segment == path)
            .map(|(_, response)| response)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredResponse {
    pub file: PathBuf,
    /// Lowercased header names; repeated names are separate entries.
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Severity {
    Info,
    Warning,
    /// The build uses something NextRS cannot serve correctly yet.
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::parse_byte_size;

    /// The `bytes` package's parse, as Next uses it for bodySizeLimit.
    #[test]
    fn byte_sizes_parse_as_next_parses_them() {
        assert_eq!(parse_byte_size("1mb"), Some(1024 * 1024));
        assert_eq!(parse_byte_size("1 MB"), Some(1024 * 1024));
        assert_eq!(parse_byte_size("500kb"), Some(500 * 1024));
        assert_eq!(parse_byte_size("1.5gb"), Some(1024 * 1024 * 1536));
        assert_eq!(parse_byte_size("2048"), Some(2048));
        assert_eq!(parse_byte_size("10b"), Some(10));
        assert_eq!(parse_byte_size("ten mb"), None);
        assert_eq!(parse_byte_size("5 parsecs"), None);
    }
}
