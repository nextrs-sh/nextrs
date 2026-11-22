//! Runtime configuration: what the build needs from `nextrs start`'s flags,
//! and whether the flags given to Doctor (the same ones, parsed by the same
//! code) would work. Read-only: nothing is created, no socket is opened and
//! no environment variable value is reported.

use std::path::{Path, PathBuf};

use nextrs_adapter::{BuildInfo, DEPLOYMENT_ID_ENV, Rendering};
use nextrs_runtime::NetworkConfig;

use super::compat::listed;
use super::report::{Category, Finding, Level, Support};
use super::{DOCS_CONFIG, DOCS_SECURITY, DoctorArgs, needs_javascript};
use crate::JsRuntime;

const DOCS_NETWORK: &str =
    "https://github.com/nextrs-sh/nextrs/blob/main/engineering/configuration.md#outbound-network";
const DOCS_CACHE: &str =
    "https://github.com/nextrs-sh/nextrs/blob/main/engineering/configuration.md#cache";
const DOCS_PROXIES: &str =
    "https://github.com/nextrs-sh/nextrs/blob/main/engineering/configuration.md#reverse-proxies";
const DOCS_IMAGES: &str =
    "https://github.com/nextrs-sh/nextrs/blob/main/engineering/configuration.md#image-optimization";

/// `access(2)` for writing into a directory, without creating anything.
fn writable_dir(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: a valid NUL-terminated path; access only reads it.
    unsafe { libc::access(c_path.as_ptr(), libc::W_OK | libc::X_OK) == 0 }
}

fn nearest_existing(path: &Path) -> Option<PathBuf> {
    let absolute = std::path::absolute(path).ok()?;
    let mut current = absolute.as_path();
    while !current.exists() {
        current = current.parent()?;
    }
    Some(current.to_path_buf())
}

/// A directory `nextrs start` would use (or create, mode 0700): refused
/// inside `forbidden` by the same check `start` runs (`checked_dir`), and
/// it, or the ancestor it would be created in, must be writable.
fn directory_finding(
    code: &'static str,
    flag: &str,
    dir: &Path,
    forbidden: &[PathBuf],
    docs: &'static str,
) -> Finding {
    let shown = dir.display().to_string();
    let category = Category::RuntimeConfig;
    let resolved = match crate::checked_dir(dir, forbidden, flag) {
        Ok(resolved) => resolved,
        Err(error) => {
            return Finding::new(
                code,
                Level::Error,
                category,
                format!("{flag} is not usable"),
                format!("nextrs start refuses it: {error}"),
            )
            .component(format!("{flag} {shown}"))
            .suggest(format!(
                "Choose a {flag} outside the build and node_modules, for example under /var/cache."
            ))
            .docs(docs);
        }
    };
    if resolved.exists() {
        if !resolved.is_dir() {
            return Finding::new(
                code,
                Level::Error,
                category,
                format!("{flag} is not a directory"),
                format!("{shown} exists and is not a directory."),
            )
            .component(format!("{flag} {shown}"))
            .docs(docs);
        }
        if !writable_dir(&resolved) {
            return Finding::new(
                code,
                Level::Warning,
                category,
                format!("{flag} is not writable by this user"),
                format!(
                    "{shown} exists but this user cannot write to it; check that the user running \
                     nextrs start can."
                ),
            )
            .component(format!("{flag} {shown}"))
            .suggest("Give the service user ownership of the directory (mode 0700).")
            .docs(docs);
        }
        return Finding::new(
            code,
            Level::Pass,
            category,
            format!("{flag} usable"),
            format!("{shown} is outside the build and writable by this user."),
        )
        .component(format!("{flag} {shown}"));
    }
    match nearest_existing(&resolved) {
        Some(parent) if writable_dir(&parent) => Finding::new(
            code,
            Level::Pass,
            category,
            format!("{flag} can be created"),
            format!(
                "{shown} does not exist yet; nextrs start creates it (mode 0700). Doctor did not."
            ),
        )
        .component(format!("{flag} {shown}")),
        _ => Finding::new(
            code,
            Level::Warning,
            category,
            format!("{flag} cannot be created by this user"),
            format!(
                "{shown} does not exist, and this user cannot create it in its nearest existing \
                 parent directory."
            ),
        )
        .component(format!("{flag} {shown}"))
        .suggest("Create the directory for the service user beforehand (mode 0700).")
        .docs(docs),
    }
}

pub struct Inputs<'a> {
    pub args: &'a DoctorArgs,
    pub info: Option<&'a BuildInfo>,
    pub project_dir: &'a Path,
    /// `NetArgs::config()`, the parser `nextrs start` uses.
    pub network: &'a Result<Option<NetworkConfig>, String>,
    /// Whether an environment variable is set where Doctor runs. Only
    /// presence is reported, never a value.
    pub env_is_set: &'a dyn Fn(&str) -> bool,
}

pub fn findings(inputs: &Inputs) -> Vec<Finding> {
    let Inputs {
        args,
        info,
        project_dir,
        network,
        env_is_set,
    } = *inputs;
    let runtime = &args.runtime;
    let r = Category::RuntimeConfig;
    let mut out = Vec::new();

    // JavaScript runtime.
    if let Some(info) = info {
        let js_routes = info
            .routes
            .iter()
            .filter(|r| !r.internal && r.rendering != Rendering::Static)
            .count();
        let executor_features: Vec<&str> = info
            .diagnostics
            .iter()
            .filter(|d| nextrs_adapter::EXECUTOR_FEATURES.contains(&d.code))
            .map(|d| match d.code {
                "server-actions" => "Server Actions",
                "isr" => "ISR",
                "proxy" => "proxy",
                other => other,
            })
            .collect();
        out.push(match runtime.js_runtime {
            _ if !needs_javascript(info) => Finding::new(
                "NEXTRS-DOC-401",
                Level::Pass,
                r,
                "No JavaScript execution required",
                "Every route is prerendered: nextrs start serves the build from Rust without \
                 embedded V8.",
            )
            .support(Support::Supported),
            Some(JsRuntime::EmbeddedV8) => Finding::new(
                "NEXTRS-DOC-401",
                Level::Pass,
                r,
                "--js-runtime embedded-v8",
                format!(
                    "Dynamic routes ({js_routes}) and the build's server features run in embedded \
                     V8."
                ),
            )
            .support(Support::Experimental),
            Some(JsRuntime::None) if !executor_features.is_empty() => Finding::new(
                "NEXTRS-DOC-401",
                Level::Error,
                r,
                "--js-runtime none refuses this build",
                format!(
                    "The build uses {}, which need embedded V8: nextrs start refuses to start \
                     without --js-runtime embedded-v8.",
                    join_and(&executor_features)
                ),
            )
            .component("--js-runtime")
            .suggest("Run nextrs start with --js-runtime embedded-v8.")
            .docs(DOCS_CONFIG),
            Some(JsRuntime::None) => Finding::new(
                "NEXTRS-DOC-401",
                Level::Warning,
                r,
                format!("{js_routes} dynamic route(s) answer 501 with --js-runtime none"),
                "Without embedded V8, dynamic routes answer 501 (x-nextrs-unsupported: \
                 javascript-required).",
            )
            .component("--js-runtime")
            .suggest("Run nextrs start with --js-runtime embedded-v8.")
            .docs(DOCS_CONFIG),
            None => Finding::new(
                "NEXTRS-DOC-401",
                Level::Info,
                r,
                "Dynamic routes require --js-runtime embedded-v8",
                if executor_features.is_empty() {
                    format!(
                        "{js_routes} route(s) need JavaScript execution; nextrs start's default \
                         (--js-runtime none) answers them 501."
                    )
                } else {
                    format!(
                        "{js_routes} route(s) need JavaScript execution, and so do the build's {}; \
                         nextrs start's default (--js-runtime none) refuses this build.",
                        join_and(&executor_features)
                    )
                },
            )
            .support(Support::Experimental)
            .component("--js-runtime")
            .suggest("Run nextrs start with --js-runtime embedded-v8.")
            .docs(DOCS_CONFIG),
        });
    }

    // The JavaScript runtime will be used: explicitly, or assumed.
    let executor = runtime.js_runtime != Some(JsRuntime::None);
    let explicit = runtime.js_runtime.is_some();

    // Outbound network.
    match network {
        Err(error) => out.push(
            Finding::new(
                "NEXTRS-DOC-402",
                Level::Error,
                r,
                "Network flags are invalid",
                format!("nextrs start refuses them: {error}"),
            )
            .component("--net-allow")
            .docs(DOCS_NETWORK),
        ),
        Ok(config) => {
            if let Some(info) = info {
                out.extend(network_findings(info, config.as_ref(), executor));
            } else if let Some(config) = config {
                out.push(
                    Finding::new(
                        "NEXTRS-DOC-402",
                        Level::Pass,
                        r,
                        "Network flags are valid",
                        format!("{} --net-allow rules parsed.", config.policy.allow.len()),
                    )
                    .component("--net-allow"),
                );
            }
        }
    }

    // Data cache.
    if let Some(info) = info {
        let isr = info.prerenders.iter().any(|p| p.revalidate.is_some());
        let actions = info.server_actions.count > 0;
        let isolates = runtime.isolates.unwrap_or(1).max(1);
        if runtime.no_shared_cache && executor && (isr || actions) && isolates > 1 {
            // The refusal nextrs start applies.
            out.push(
                Finding::new(
                    "NEXTRS-DOC-403",
                    Level::Error,
                    r,
                    "--no-shared-cache with more than one isolate",
                    "ISR pages and revalidations by Server Actions would differ between isolates; \
                     nextrs start refuses this combination.",
                )
                .component("--no-shared-cache")
                .suggest("Drop --no-shared-cache, or use --isolates 1.")
                .docs(DOCS_CACHE),
            );
        } else if runtime.no_shared_cache && executor && isr {
            out.push(
                Finding::new(
                    "NEXTRS-DOC-403",
                    Level::Warning,
                    r,
                    "--no-shared-cache keeps ISR pages in memory only",
                    "Regenerated pages are lost on restart, and revalidations by Server Actions \
                     stay in one isolate.",
                )
                .component("--no-shared-cache")
                .docs(DOCS_CACHE),
            );
        }
        // nextrs start reads --cache-dir only for the embedded executor.
        if let Some(dir) = runtime.cache_dir.as_ref().filter(|_| executor) {
            let mut config = nextrs_runtime::ExecutorConfig::new(
                info.project_dir.clone(),
                info.dist_dir.clone(),
            );
            config.workspace_dir = info.workspace_root.clone();
            let roots = config.fs_policy().read_roots().to_vec();
            out.push(directory_finding(
                "NEXTRS-DOC-404",
                "--cache-dir",
                dir,
                &roots,
                DOCS_CACHE,
            ));
        } else if executor && (isr || actions) && !runtime.no_shared_cache {
            out.push(
                Finding::new(
                    "NEXTRS-DOC-404",
                    Level::Info,
                    r,
                    "Data cache in memory only",
                    "ISR pages and cached fetch results are shared by all isolates but lost on \
                     restart.",
                )
                .suggest("Add --cache-dir <dir outside the project> to keep the data cache across restarts.")
                .docs(DOCS_CACHE),
            );
        }
    } else if let Some(dir) = &runtime.cache_dir {
        // No build: the read roots nextrs start would forbid.
        let roots: Vec<PathBuf> = [project_dir.join(".next"), project_dir.join("node_modules")]
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();
        out.push(directory_finding(
            "NEXTRS-DOC-404",
            "--cache-dir",
            dir,
            &roots,
            DOCS_CACHE,
        ));
    }
    if let Some(dir) = &runtime.image_cache_dir {
        out.push(directory_finding(
            "NEXTRS-DOC-405",
            "--image-cache-dir",
            dir,
            &[project_dir.to_path_buf()],
            DOCS_IMAGES,
        ));
    }

    // Reverse proxies and forwarded headers.
    let host_public = runtime.host.is_some_and(|h| !h.is_loopback());
    let actions = info.is_some_and(|i| i.server_actions.count > 0);
    out.push(match args.proxy.policy() {
        Err(error) => Finding::new(
            "NEXTRS-DOC-406",
            Level::Error,
            r,
            "--trusted-proxy is invalid",
            format!("nextrs start refuses it: {error}"),
        )
        .component("--trusted-proxy")
        .docs(DOCS_PROXIES),
        Ok(nextrs_server::ForwardedHeaders::Pass) if host_public => Finding::new(
            "NEXTRS-DOC-406",
            Level::Warning,
            r,
            "Forwarded headers trusted from every client",
            format!(
                "--host is not loopback and no forwarded-header policy is set: any client can \
                 claim another host or scheme in X-Forwarded-*, which Next.js uses for redirects, \
                 absolute URLs{}.",
                if actions {
                    " and the Server Action origin check"
                } else {
                    ""
                }
            ),
        )
        .component("--trusted-proxy")
        .suggest(
            "Behind a reverse proxy, pass --trusted-proxy <its address or CIDR>; with clients \
             connecting directly, pass --strip-forwarded-headers.",
        )
        .docs(DOCS_SECURITY),
        Ok(nextrs_server::ForwardedHeaders::Pass) => Finding::new(
            "NEXTRS-DOC-406",
            Level::Info,
            r,
            "Forwarded headers passed through (as next start)",
            "X-Forwarded-* headers reach Next.js from every client; the reverse proxy in front \
             must overwrite them.",
        )
        .component("--trusted-proxy")
        .suggest("In production behind a proxy, pass --trusted-proxy <its address or CIDR>.")
        .docs(DOCS_PROXIES),
        Ok(nextrs_server::ForwardedHeaders::TrustedProxies(ranges)) => Finding::new(
            "NEXTRS-DOC-406",
            Level::Pass,
            r,
            "Forwarded headers kept only from trusted proxies",
            format!("{} trusted proxy ranges.", ranges.len()),
        )
        .component("--trusted-proxy"),
        Ok(nextrs_server::ForwardedHeaders::Strip) => Finding::new(
            "NEXTRS-DOC-406",
            Level::Pass,
            r,
            "Forwarded headers removed from every request",
            "--strip-forwarded-headers: for clients that connect directly.",
        )
        .component("--strip-forwarded-headers"),
    });

    // Environment variables (names only).
    let (files, names) = super::environment::env_file_names(project_dir);
    if executor && info.is_none_or(needs_javascript) && !names.is_empty() {
        let missing: Vec<String> = names
            .iter()
            .filter(|n| !runtime.js_env.contains(n))
            .cloned()
            .collect();
        out.push(if missing.is_empty() {
            Finding::new(
                "NEXTRS-DOC-407",
                Level::Pass,
                r,
                ".env variables passed with --js-env",
                format!(
                    "Every variable {} defines is passed with --js-env.",
                    files.join(", ")
                ),
            )
        } else {
            Finding::new(
                "NEXTRS-DOC-407",
                Level::Warning,
                r,
                ".env files are not loaded by NextRS",
                format!(
                    "{} define {}. next start loads these files at runtime; NextRS gives \
                     JavaScript only NODE_ENV and the --js-env names, so server code reading them \
                     at request time gets undefined. (Doctor read the names only.)",
                    files.join(", "),
                    listed(&missing, 10)
                ),
            )
            .component(files.join(", "))
            .suggest(format!(
                "Set them in the service environment and pass each with --js-env NAME (for \
                 example --js-env {}).",
                missing[0]
            ))
            .docs(DOCS_CONFIG)
        });
    }
    let unset: Vec<String> = runtime
        .js_env
        .iter()
        .filter(|n| !env_is_set(n))
        .cloned()
        .collect();
    if !unset.is_empty() {
        out.push(
            Finding::new(
                "NEXTRS-DOC-408",
                Level::Warning,
                r,
                "--js-env variables not set here",
                format!(
                    "{} are not set in the environment Doctor runs in; nextrs start warns and \
                     leaves them undefined.",
                    listed(&unset, 10)
                ),
            )
            .component("--js-env")
            .suggest("Run Doctor with the environment the service will have."),
        );
    }

    if let Some(info) = info.filter(|_| executor) {
        // Deployment ID: the check nextrs start runs before its isolates.
        let passed =
            runtime.js_env.iter().any(|n| n == DEPLOYMENT_ID_ENV) && env_is_set(DEPLOYMENT_ID_ENV);
        // Presence only: a placeholder stands in for the value.
        if info
            .config
            .deployment
            .server_id(passed.then_some("set"))
            .is_err()
        {
            out.push(
                Finding::new(
                    "NEXTRS-DOC-409",
                    if explicit {
                        Level::Error
                    } else {
                        Level::Warning
                    },
                    r,
                    format!("{DEPLOYMENT_ID_ENV} required at runtime"),
                    format!(
                        "The build enables experimental.runtimeServerDeploymentId: nextrs start \
                         refuses to start without --js-env {DEPLOYMENT_ID_ENV} set."
                    ),
                )
                .component("--js-env")
                .suggest(format!(
                    "Set {DEPLOYMENT_ID_ENV} and pass --js-env {DEPLOYMENT_ID_ENV}."
                ))
                .docs(DOCS_CONFIG),
            );
        }
        if info.server_actions.count > 0 {
            const ACTION_KEY: &str = "NEXT_SERVER_ACTIONS_ENCRYPTION_KEY";
            out.push(Finding::new(
                "NEXTRS-DOC-410",
                Level::Info,
                r,
                format!(
                    "Server Actions key: {ACTION_KEY} {}",
                    if env_is_set(ACTION_KEY) {
                        "set"
                    } else {
                        "not set"
                    }
                ),
                if env_is_set(ACTION_KEY) {
                    format!("nextrs start passes {ACTION_KEY} to Next.js, as next start reads it.")
                } else {
                    "The build's own key is used; instances serving the same build share it."
                        .to_owned()
                },
            ));
            // The two body limits.
            let max_request_body = runtime
                .max_request_body
                .map_or(nextrs_server::DEFAULT_MAX_REQUEST_BODY, |m| {
                    m.saturating_mul(1024 * 1024)
                });
            if info.server_actions.body_size_limit > max_request_body {
                out.push(
                    Finding::new(
                        "NEXTRS-DOC-411",
                        Level::Warning,
                        r,
                        "serverActions.bodySizeLimit above --max-request-body",
                        format!(
                            "Next accepts action bodies up to {} bytes, NextRS answers 413 above \
                             {max_request_body} bytes.",
                            info.server_actions.body_size_limit
                        ),
                    )
                    .component("--max-request-body")
                    .suggest("Raise --max-request-body to accept what Next accepts.")
                    .docs(DOCS_CONFIG),
                );
            }
        }
    }
    if host_public {
        out.push(
            Finding::new(
                "NEXTRS-DOC-412",
                Level::Info,
                r,
                "Non-loopback --host",
                "NextRS is experimental; put a reverse proxy in front of it in production.",
            )
            .component("--host")
            .docs(DOCS_SECURITY),
        );
    }
    out
}

/// Rewrites to other origins and remote images, against the network policy
/// nextrs start would enforce (`NetworkPolicy::check_url`, no DNS lookup).
fn network_findings(
    info: &BuildInfo,
    config: Option<&NetworkConfig>,
    executor: bool,
) -> Vec<Finding> {
    let r = Category::RuntimeConfig;
    let mut out = Vec::new();
    let destinations: Vec<String> = info
        .routing
        .before_files
        .iter()
        .chain(&info.routing.after_files)
        .chain(&info.routing.fallback)
        .filter_map(|rule| rule.destination.as_deref())
        .filter(|d| !d.starts_with('/'))
        .map(str::to_owned)
        .collect();
    if !destinations.is_empty() {
        // Fixed origins only; destinations with parameters in the host are
        // checked per request by nextrs start.
        let origins: Vec<String> = destinations
            .iter()
            .filter_map(|d| {
                let (scheme, rest) = d.split_once("://")?;
                let authority = rest.split(['/', '?', '#']).next()?;
                // Never print credentials a destination carries.
                let authority = authority.rsplit('@').next()?;
                (!authority.contains(':') || authority.rsplit(':').next()?.parse::<u16>().is_ok())
                    .then(|| format!("{scheme}://{authority}"))
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let denied: Vec<String> = match config {
            None => origins.clone(),
            Some(config) => origins
                .iter()
                .filter(|o| {
                    url::Url::parse(&format!("{o}/"))
                        .map_or(true, |url| config.policy.check_url(&url).is_err())
                })
                .cloned()
                .collect(),
        };
        out.push(if denied.is_empty() && config.is_some() {
            Finding::new(
                "NEXTRS-DOC-413",
                Level::Pass,
                r,
                "Rewrites to other origins allowed by --net-allow",
                format!(
                    "{} rewrite rule(s) forward to {}.",
                    destinations.len(),
                    listed(&origins, 5)
                ),
            )
            .support(Support::Experimental)
            .component("--net-allow")
            .inspect_code("external-rewrites")
        } else {
            Finding::new(
                "NEXTRS-DOC-413",
                Level::Warning,
                r,
                "Rewrites to other origins need --net-allow",
                format!(
                    "{} rewrite rule(s) forward to other origins; NextRS answers 502 for \
                     destinations the network policy does not allow: {}.",
                    destinations.len(),
                    if denied.is_empty() {
                        "destinations with parameters in the host".to_owned()
                    } else {
                        listed(&denied, 5)
                    }
                ),
            )
            .support(Support::Experimental)
            .component("--net-allow")
            .inspect_code("external-rewrites")
            .suggest(match denied.first() {
                Some(origin) => format!(
                    "Add --net-allow {origin} (one per origin) to nextrs start; loopback and \
                     private addresses also need --net-allow-addr."
                ),
                None => "Add --net-allow for each destination host.".to_owned(),
            })
            .docs(DOCS_NETWORK)
        });
    }
    if let Some(images) = &info.images
        && !images.disabled()
        && (!images.remote_patterns.is_empty() || !images.domains.is_empty())
    {
        out.push(if config.is_none() {
            // The startup warning nextrs start logs.
            Finding::new(
                "NEXTRS-DOC-414",
                Level::Warning,
                r,
                "Remote images need --net-allow",
                "images.remotePatterns is configured but no --net-allow is given: remote images \
                 answer 400.",
            )
            .component("--net-allow")
            .inspect_code("remote-images")
            .suggest("Add --net-allow for the image hosts your remotePatterns name.")
            .docs(DOCS_IMAGES)
        } else {
            Finding::new(
                "NEXTRS-DOC-414",
                Level::Info,
                r,
                "Remote images: both remotePatterns and --net-allow apply",
                "Remote images are fetched only from hosts images.remotePatterns and --net-allow \
                 both allow; Doctor does not compare the two lists.",
            )
            .component("--net-allow")
            .inspect_code("remote-images")
            .docs(DOCS_IMAGES)
        });
    }
    if executor && needs_javascript(info) {
        out.push(match config {
            None => Finding::new(
                "NEXTRS-DOC-415",
                Level::Info,
                r,
                "Outbound fetch is denied",
                "Without --net-allow, every fetch from server code fails. Doctor cannot tell which \
                 APIs the server code calls.",
            )
            .component("--net-allow")
            .suggest("If server code calls external APIs, add --net-allow <host> for each one.")
            .docs(DOCS_NETWORK),
            Some(config) => Finding::new(
                "NEXTRS-DOC-415",
                Level::Pass,
                r,
                "Outbound fetch allowed for --net-allow destinations",
                format!(
                    "{} rules; other destinations, and hosts resolving to private addresses, are \
                     refused.",
                    config.policy.allow.len()
                ),
            )
            .component("--net-allow"),
        });
    }
    out
}

/// `a`, `a and b`, `a, b and c`.
fn join_and(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}
