//! `next.config` `images`, as the build wrote it to `images-manifest.json`
//! (patterns already compiled to regular expressions by picomatch, exactly
//! as next start compiles them per request), plus the optimizer settings
//! next start reads from `experimental` (`required-server-files.json`).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::InspectError;
use crate::manifests::read_json_optional;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesConfig {
    #[serde(default)]
    pub device_sizes: Vec<u32>,
    #[serde(default)]
    pub image_sizes: Vec<u32>,
    #[serde(default = "default_loader")]
    pub loader: String,
    /// Deprecated exact-hostname allowlist.
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default = "default_ttl", rename = "minimumCacheTTL")]
    pub minimum_cache_ttl: u64,
    #[serde(default = "default_formats")]
    pub formats: Vec<String>,
    #[serde(default = "default_redirects")]
    pub maximum_redirects: u32,
    #[serde(default = "default_body")]
    pub maximum_response_body: u64,
    #[serde(default)]
    pub maximum_disk_cache_size: Option<u64>,
    #[serde(default, rename = "dangerouslyAllowLocalIP")]
    pub dangerously_allow_local_ip: bool,
    #[serde(default, rename = "dangerouslyAllowSVG")]
    pub dangerously_allow_svg: bool,
    #[serde(default = "default_csp")]
    pub content_security_policy: String,
    #[serde(default = "default_disposition")]
    pub content_disposition_type: String,
    /// `None`: every local path is allowed (no `localPatterns` configured).
    #[serde(default)]
    pub local_patterns: Option<Vec<LocalPattern>>,
    #[serde(default)]
    pub remote_patterns: Vec<RemotePattern>,
    /// `None`: every quality from 1 to 100.
    #[serde(default)]
    pub qualities: Option<Vec<u32>>,
    #[serde(default)]
    pub unoptimized: bool,
    #[serde(default)]
    pub custom_cache_handler: bool,
    /// From `required-server-files.json`; not part of the manifest.
    #[serde(skip_deserializing)]
    pub optimizer: OptimizerSettings,
}

/// `{ pathname, search }`; `pathname` is a JavaScript regular expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPattern {
    pub pathname: String,
    #[serde(default)]
    pub search: Option<String>,
}

/// `hostname` and `pathname` are JavaScript regular expressions;
/// `protocol` has no trailing colon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePattern {
    #[serde(default)]
    pub protocol: Option<String>,
    pub hostname: String,
    #[serde(default)]
    pub port: Option<String>,
    pub pathname: String,
    #[serde(default)]
    pub search: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OptimizerSettings {
    /// `experimental.imgOptTimeoutInSeconds` (sharp's timeout; 7).
    pub timeout_seconds: u64,
    /// `experimental.imgOptMaxInputPixels` (sharp's `limitInputPixels`).
    pub max_input_pixels: u64,
    /// `experimental.isrFlushToDisk`: next start's disk cache for images.
    pub flush_to_disk: bool,
    /// `output: 'export'`: the endpoint answers 400.
    pub export: bool,
}

impl Default for OptimizerSettings {
    fn default() -> Self {
        Self {
            timeout_seconds: 7,
            max_input_pixels: 268_402_689,
            flush_to_disk: true,
            export: false,
        }
    }
}

impl ImagesConfig {
    /// `deviceSizes` then `imageSizes`: the widths the endpoint accepts.
    pub fn sizes(&self) -> impl Iterator<Item = u32> + '_ {
        self.device_sizes.iter().chain(&self.image_sizes).copied()
    }

    /// next start renders its 404 for every image request.
    pub fn disabled(&self) -> bool {
        self.loader != "default" || self.unoptimized
    }
}

fn default_loader() -> String {
    "default".into()
}
fn default_ttl() -> u64 {
    14_400
}
fn default_formats() -> Vec<String> {
    vec!["image/webp".into()]
}
fn default_redirects() -> u32 {
    3
}
fn default_body() -> u64 {
    50_000_000
}
fn default_csp() -> String {
    "script-src 'none'; frame-src 'none'; sandbox;".into()
}
fn default_disposition() -> String {
    "attachment".into()
}

#[derive(Deserialize)]
struct ImagesManifest {
    images: ImagesConfig,
}

pub(crate) fn images_config(
    dist_dir: &Path,
    config: Option<&Value>,
) -> Result<Option<ImagesConfig>, InspectError> {
    let Some(manifest) =
        read_json_optional::<ImagesManifest>(&dist_dir.join("images-manifest.json"))?
    else {
        return Ok(None);
    };
    let mut images = manifest.images;
    let experimental = config.and_then(|c| c.get("experimental"));
    let number = |key: &str| {
        experimental
            .and_then(|e| e.get(key))
            .and_then(Value::as_u64)
    };
    let defaults = OptimizerSettings::default();
    images.optimizer = OptimizerSettings {
        timeout_seconds: number("imgOptTimeoutInSeconds").unwrap_or(defaults.timeout_seconds),
        max_input_pixels: number("imgOptMaxInputPixels").unwrap_or(defaults.max_input_pixels),
        flush_to_disk: experimental
            .and_then(|e| e.get("isrFlushToDisk"))
            .and_then(Value::as_bool)
            .unwrap_or(defaults.flush_to_disk),
        export: config.and_then(|c| c.get("output")).and_then(Value::as_str) == Some("export"),
    };
    Ok(Some(images))
}
