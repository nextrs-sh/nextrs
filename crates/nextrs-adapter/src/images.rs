//! `next.config` `images`, as the build wrote it to `images-manifest.json`
//! (patterns already compiled to regular expressions by picomatch, exactly
//! as next start compiles them per request), plus the optimizer settings
//! next start reads from `experimental` (`required-server-files.json`).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifests::read_json_optional;
use crate::{Diagnostic, InspectError, Severity};

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
    /// Absent in a `next.config` pattern (then `**`); always in the manifest.
    #[serde(default)]
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
    #[serde(default)]
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

/// `next.config` `images` as next start's config loader leaves it (in
/// `required-server-files.json`): the manifest's fields, with
/// `localPatterns[].pathname` and `remotePatterns[].hostname`/`.pathname`
/// still globs. Compiled as the build compiles them for the manifest:
/// `makeRe(pathname ?? '**', { dot: true })`, `makeRe(hostname)`.
fn from_config(value: &Value) -> Result<ImagesConfig, String> {
    let mut images: ImagesConfig =
        serde_json::from_value(value.clone()).map_err(|e| format!("images: {e}"))?;
    let raw = |key: &str| value.get(key).and_then(Value::as_array);
    if let Some(patterns) = &mut images.local_patterns {
        let globs = raw("localPatterns").ok_or("localPatterns")?;
        for (pattern, glob) in patterns.iter_mut().zip(globs) {
            let glob = glob.get("pathname").and_then(Value::as_str).unwrap_or("**");
            pattern.pathname = crate::glob::make_re(glob, true)?;
        }
    }
    let globs = raw("remotePatterns").map(Vec::as_slice).unwrap_or_default();
    for (pattern, glob) in images.remote_patterns.iter_mut().zip(globs) {
        let hostname = glob
            .get("hostname")
            .and_then(Value::as_str)
            .ok_or("remotePatterns[].hostname")?;
        pattern.hostname = crate::glob::make_re(hostname, false)?;
        let pathname = glob.get("pathname").and_then(Value::as_str).unwrap_or("**");
        pattern.pathname = crate::glob::make_re(pathname, true)?;
    }
    Ok(images)
}

pub(crate) fn images_config(
    dist_dir: &Path,
    config: Option<&Value>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<ImagesConfig>, InspectError> {
    let mut images = match read_json_optional::<ImagesManifest>(
        &dist_dir.join("images-manifest.json"),
    )? {
        Some(manifest) => manifest.images,
        // `output: 'standalone'` copies required-server-files.json but not
        // the manifest; next start's standalone server.js reads the same
        // config from it, with patterns still as globs.
        None => match config.and_then(|c| c.get("images")) {
            Some(value) => match from_config(value) {
                Ok(images) => images,
                Err(why) => {
                    diagnostics.push(Diagnostic {
                        severity: Severity::Unsupported,
                        code: "images-config",
                        message: format!(
                            "no images-manifest.json, and required-server-files.json's images config \
                             cannot be compiled: {why}"
                        ),
                    });
                    return Ok(None);
                }
            },
            None => return Ok(None),
        },
    };
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
