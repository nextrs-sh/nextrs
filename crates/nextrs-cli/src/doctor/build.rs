//! The production build: what `nextrs inspect` reads, the version policy
//! `nextrs start` applies, and `nextrs start`'s own startup checks.

use std::path::Path;

use nextrs_adapter::{
    BuildInfo, Diagnostic, InspectError, MetadataSource, Severity, UNSUPPORTED_VERSION_CODE,
};

use super::report::{Category, Finding, Level, Support};
use super::{DOCS_COMPAT_UNSUPPORTED, DOCS_COMPAT_VERSIONS, DOCS_CONFIG, needs_javascript};

/// The build could not be read: a missing build is incomplete, anything
/// else (unparsable or inconsistent metadata) is a blocker, because
/// `nextrs start` fails the same way.
pub fn inspect_failure(error: &InspectError) -> Finding {
    match error {
        InspectError::NoBuildOutput { .. } => Finding::new(
            "NEXTRS-DOC-101",
            Level::Warning,
            Category::Build,
            "Production build missing",
            "No .next/BUILD_ID: route, feature and runtime-configuration checks need a production \
             build, so this assessment is incomplete. Doctor never runs next build itself.",
        )
        .component(".next")
        .suggest("Run next build, then repeat nextrs doctor."),
        other => Finding::new(
            "NEXTRS-DOC-102",
            Level::Error,
            Category::Build,
            "Build metadata could not be read",
            format!("{other}. nextrs start and nextrs inspect fail on this build too."),
        )
        .component(".next")
        .suggest(
            "Run a clean next build (delete .next first); do not edit or partially copy .next.",
        ),
    }
}

pub fn build_found(info: &BuildInfo) -> Finding {
    let source = match info.source {
        MetadataSource::AdapterApi => {
            "the deployment adapter output (.next/nextrs/build-output.json)"
        }
        MetadataSource::InternalManifests => "the manifests next start reads",
    };
    Finding::new(
        "NEXTRS-DOC-103",
        Level::Pass,
        Category::Build,
        format!("Production build {} found", info.build_id),
        format!(
            "{} routes and {} prerendered outputs read from {source}.",
            info.routes.iter().filter(|r| !r.internal).count(),
            info.prerenders.len()
        ),
    )
    .component(".next")
}

/// The version policy, as the adapter applied it (the same diagnostics
/// `nextrs start` refuses or warns with).
pub fn version_findings(info: &BuildInfo) -> Vec<Finding> {
    let find = |code: &str| info.diagnostics.iter().find(|d| d.code == code);
    let version = info
        .next_version
        .as_ref()
        .map(|v| format!("{} (from {})", v.version, v.source));
    let tested = nextrs_adapter::TESTED_NEXT_VERSION;
    let mut findings = Vec::new();
    findings.push(if let Some(d) = find(UNSUPPORTED_VERSION_CODE) {
        Finding::new(
            "NEXTRS-DOC-104",
            Level::Error,
            Category::Build,
            format!(
                "Next.js {} is not supported",
                info.next_version
                    .as_ref()
                    .map_or("?", |v| v.version.as_str())
            ),
            format!("{}. nextrs start refuses this build.", d.message),
        )
        .support(Support::Unsupported)
        .inspect_code(d.code)
        .suggest(format!("Rebuild with Next.js {tested}.x."))
        .docs(DOCS_COMPAT_VERSIONS)
    } else if let Some(d) = find("untested-next-version") {
        Finding::new(
            "NEXTRS-DOC-104",
            Level::Warning,
            Category::Build,
            format!(
                "Next.js {} is not the tested release line",
                info.next_version
                    .as_ref()
                    .map_or("?", |v| v.version.as_str())
            ),
            format!("{}. nextrs start serves it with a warning.", d.message),
        )
        .support(Support::Unknown)
        .inspect_code(d.code)
        .suggest(format!(
            "Prefer Next.js {tested}.x, or test this version against next start."
        ))
        .docs(DOCS_COMPAT_VERSIONS)
    } else if let Some(d) = find("unknown-next-version") {
        Finding::new(
            "NEXTRS-DOC-104",
            Level::Unknown,
            Category::Build,
            "Next.js version of the build unknown",
            format!(
                "{}: neither .next/diagnostics/framework.json nor node_modules/next/package.json \
                 names it. nextrs start serves it with a warning.",
                d.message
            ),
        )
        .inspect_code(d.code)
        .docs(DOCS_COMPAT_VERSIONS)
    } else {
        Finding::new(
            "NEXTRS-DOC-104",
            Level::Pass,
            Category::Build,
            format!("Next.js {}", version.as_deref().unwrap_or("?")),
            format!("The release line NextRS is tested with ({tested}.x)."),
        )
        .support(Support::Supported)
    });
    if let Some(d) = find("next-version-has-advisories") {
        findings.push(
            Finding::new(
                "NEXTRS-DOC-006",
                Level::Warning,
                Category::Build,
                "The build's Next.js version has published security advisories",
                squash(&d.message),
            )
            .inspect_code(d.code)
            .suggest({
                let (major, minor, patch) = nextrs_adapter::NEXT_SECURITY_BASELINE;
                format!("Rebuild with Next.js {major}.{minor}.{patch} or later.")
            })
            .docs(DOCS_COMPAT_VERSIONS),
        );
    }
    findings
}

/// Next's runtime files: the executor loads Next's compiled handlers, which
/// require `node_modules/next` (the project's, or the workspace root's in a
/// monorepo; nothing else is readable to JavaScript).
pub fn runtime_files_findings(
    info: &BuildInfo,
    installed: Option<&str>,
    js_runtime_none: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    let build_version = info.next_version.as_ref();
    // Dynamic routes and the executor features need the isolates; routes
    // whose rendering is open (generateStaticParams) only for unlisted
    // params.
    let required = info
        .routes
        .iter()
        .any(|r| !r.internal && r.rendering == nextrs_adapter::Rendering::Dynamic)
        || info
            .diagnostics
            .iter()
            .any(|d| nextrs_adapter::EXECUTOR_FEATURES.contains(&d.code));
    match installed {
        None if !js_runtime_none && needs_javascript(info) => findings.push(
            Finding::new(
                "NEXTRS-DOC-105",
                if required {
                    Level::Error
                } else {
                    Level::Warning
                },
                Category::Build,
                "node_modules/next missing",
                "Dynamic routes, Server Actions, ISR and the proxy run Next's compiled handlers, \
                 which load Next.js from node_modules/next (in the project, or in the workspace \
                 root of a monorepo). Without it, nextrs start --js-runtime embedded-v8 fails to \
                 start its isolates.",
            )
            .component("node_modules/next")
            .suggest(
                "Install production dependencies next to the build (npm ci --omit=dev, or ship \
                 output: 'standalone' with its node_modules).",
            )
            .docs(super::DOCS_INSTALL),
        ),
        None => {}
        Some(installed) => {
            if let Some(build) = build_version
                && build.version != installed
            {
                findings.push(
                    Finding::new(
                        "NEXTRS-DOC-106",
                        Level::Warning,
                        Category::Build,
                        "Installed Next.js differs from the build's",
                        format!(
                            "The build was made with Next.js {} (from {}), but node_modules/next is \
                             {installed}. NextRS runs the build's compiled handlers with the \
                             installed Next.js runtime files.",
                            build.version, build.source
                        ),
                    )
                    .component("node_modules/next")
                    .suggest("Rebuild, or reinstall the dependencies the build was made with."),
                );
            }
        }
    }
    findings
}

/// The first build metadata file (`.next/BUILD_ID`, or a link among the
/// files directly in `.next`, `.next/server` and `.next/nextrs`, where the
/// adapter reads its manifests) that resolves outside `dist`. Inspection
/// prints what these files contain, the build ID verbatim: Doctor does not
/// read them through a link that leaves the build (to `/proc/self/environ`
/// or a credentials file, say).
pub fn metadata_outside_build(dist: &Path) -> Option<String> {
    let real = dist.canonicalize().ok()?;
    let roots = [real];
    let mut candidates = vec![dist.join("BUILD_ID")];
    for sub in ["", "server", "nextrs"] {
        let Ok(entries) = std::fs::read_dir(dist.join(sub)) else {
            continue;
        };
        candidates.extend(
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_symlink()))
                .map(|e| e.path()),
        );
    }
    candidates.sort();
    candidates.dedup();
    candidates
        .into_iter()
        // A dangling link is left to inspection, which reports it.
        .find(|p| p.exists() && !super::fsread::contained(p, &roots))
        .map(|p| match p.strip_prefix(dist) {
            Ok(rel) => format!(".next/{}", rel.display()),
            Err(_) => ".next".to_owned(),
        })
}

pub fn metadata_outside_build_finding(file: &str) -> Finding {
    Finding::new(
        "NEXTRS-DOC-108",
        Level::Warning,
        Category::Build,
        "Build metadata links outside the build",
        format!(
            "{file} resolves outside .next. Doctor does not read build files through such a link, \
             because their content would appear in this report, so the build was not checked. \
             nextrs start and nextrs inspect do follow it."
        ),
    )
    .component(file.to_owned())
    .suggest("Run Doctor on a build whose .next holds its own files, such as a fresh next build.")
}

/// Total size of the stored responses the startup checks may load into
/// memory before Doctor declines to run them.
const PREFLIGHT_BUDGET: u64 = 1 << 30;
/// Entries of `public/` the startup checks list, at most.
const PUBLIC_ENTRY_LIMIT: usize = 500_000;

/// Why the startup checks should not run here, if they should not: they
/// read every prerendered response into memory and list `public/`, both
/// from paths the build metadata names. A stored response must be a regular
/// file inside `.next` (not `/dev/zero`, a FIFO, or a file elsewhere whose
/// read error would name it), they must fit a memory budget, and `public/`
/// must have no linked directories (a link back to `public/` makes the
/// listing loop).
pub fn preflight_guard(info: &BuildInfo) -> Result<(), String> {
    let dist = &info.dist_dir;
    let real_dist = dist
        .canonicalize()
        .map_err(|_| ".next could not be resolved".to_owned())?;
    let roots = [real_dist];
    let mut total: u64 = 0;
    for prerender in &info.prerenders {
        let stored = [&prerender.html, &prerender.rsc, &prerender.data]
            .into_iter()
            .flatten()
            .chain(prerender.segments.iter().map(|(_, s)| s));
        for response in stored {
            let file = &response.file;
            let outside = format!(
                "the stored response of {} is not a file inside .next",
                prerender.pathname
            );
            if !super::fsread::lexically_below(file, dist) {
                return Err(outside);
            }
            // A missing file is reported by the startup checks themselves,
            // with its path inside .next.
            let Ok(meta) = std::fs::metadata(file) else {
                continue;
            };
            if !meta.is_file() || !super::fsread::contained(file, &roots) {
                return Err(outside);
            }
            total = total.saturating_add(meta.len());
        }
    }
    if total > PREFLIGHT_BUDGET {
        return Err(format!(
            "the prerendered responses total {} MiB, more than Doctor loads into memory",
            total >> 20
        ));
    }
    if let Some(public) = &info.public_dir {
        let mut stack = vec![public.clone()];
        let mut seen = 0usize;
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                seen = seen.saturating_add(1);
                if seen > PUBLIC_ENTRY_LIMIT {
                    return Err(format!(
                        "public/ has more than {PUBLIC_ENTRY_LIMIT} entries"
                    ));
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_dir() {
                    stack.push(entry.path());
                } else if kind.is_symlink() && entry.path().is_dir() {
                    let rel = entry.path();
                    let rel = rel.strip_prefix(public).unwrap_or(&rel);
                    return Err(format!("public/{} is a link to a directory", rel.display()));
                }
            }
        }
    }
    Ok(())
}

pub fn preflight_skipped(reason: &str) -> Finding {
    Finding::new(
        "NEXTRS-DOC-107",
        Level::Unknown,
        Category::Build,
        "nextrs start's startup checks not run",
        format!(
            "Doctor did not run them because {reason}. They load stored responses and list \
             public/ from paths the build names."
        ),
    )
    .suggest("Start the build with nextrs start to see whether its startup checks pass.")
}

/// `nextrs start`'s own startup checks (routing rules, route patterns,
/// stored responses, the image configuration), run without an executor,
/// without binding a socket and without a cache directory. Call
/// [`preflight_guard`] first.
pub fn preflight(info: &BuildInfo, network: Option<nextrs_runtime::NetworkConfig>) -> Finding {
    let options = nextrs_server::ServerOptions {
        // Refusals are reported feature by feature instead.
        allow_unsupported: true,
        executor: None,
        images: nextrs_server::ImageOptions {
            network,
            // Never create the image cache directory here.
            cache: nextrs_server::ImageCacheOptions::default(),
            ..nextrs_server::ImageOptions::default()
        },
        ..nextrs_server::ServerOptions::default()
    };
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())
        .and_then(|runtime| {
            runtime
                .block_on(nextrs_server::app(info, &options))
                .map(drop)
                .map_err(|e| e.to_string())
        });
    match result {
        Ok(()) => Finding::new(
            "NEXTRS-DOC-107",
            Level::Pass,
            Category::Build,
            "nextrs start's startup checks pass",
            "Routing rules, route patterns, stored responses and the image configuration load as \
             nextrs start loads them.",
        ),
        Err(error) => Finding::new(
            "NEXTRS-DOC-107",
            Level::Error,
            Category::Build,
            "nextrs start would fail to start",
            format!("Startup check failed: {error}"),
        )
        .suggest(
            "Rebuild, and check the rule or file named above; NextRS refuses rules it cannot \
             apply exactly rather than skip them.",
        )
        .docs(DOCS_COMPAT_UNSUPPORTED),
    }
}

/// Diagnostics no dedicated check reports, so that none is lost (new
/// adapter codes included).
pub fn passthrough(diagnostic: &Diagnostic) -> Option<Finding> {
    let d = diagnostic;
    let message = squash(&d.message);
    let finding = |code, level, category, title: &str| {
        Finding::new(code, level, category, title, message.clone()).inspect_code(d.code)
    };
    Some(match d.code {
        // Reported by dedicated checks.
        c if super::compat::REPORTED_CODES.contains(&c) => return None,
        "unsupported-next-version"
        | "untested-next-version"
        | "unknown-next-version"
        | "next-version-has-advisories" => return None,
        "stale-adapter-output" => finding(
            "NEXTRS-DOC-110",
            Level::Warning,
            Category::Build,
            "Stale deployment adapter output",
        )
        .component(".next/nextrs/build-output.json")
        .suggest("Rebuild with the adapter, or delete the stale file; NextRS ignores it."),
        "missing-static-dir" => finding(
            "NEXTRS-DOC-111",
            Level::Warning,
            Category::Build,
            ".next/static is missing",
        )
        .component(".next/static")
        .suggest("Copy .next/static next to the build (output: 'standalone' does not include it)."),
        "workspace-root" => finding(
            "NEXTRS-DOC-112",
            Level::Warning,
            Category::Build,
            "Monorepo workspace root not found",
        )
        .suggest("Serve the build from the directory it was built in, inside its workspace."),
        "pages-adapter-mismatch" => finding(
            "NEXTRS-DOC-113",
            Level::Error,
            Category::Build,
            "Adapter output and manifests disagree",
        )
        .support(Support::Unsupported)
        .suggest("Rebuild with the adapter from a clean .next; nextrs start refuses this build.")
        .docs(DOCS_COMPAT_UNSUPPORTED),
        // Framework pages (/404, /500, /_global-error) in valid builds.
        "route-not-in-routes-manifest" => finding(
            "NEXTRS-DOC-117",
            Level::Info,
            Category::Build,
            "Route matched by its exact path",
        ),
        "prerender-missing" => finding(
            "NEXTRS-DOC-114",
            Level::Warning,
            Category::Build,
            "Build manifests are incomplete",
        )
        .suggest("Run a clean next build; a partially copied .next loses files."),
        "adapter-missing-prerender" | "adapter-routing-rules" | "adapter-proxy-output" => finding(
            "NEXTRS-DOC-115",
            Level::Info,
            Category::Build,
            "Deployment adapter output supplemented",
        ),
        "standalone" => finding(
            "NEXTRS-DOC-116",
            Level::Info,
            Category::Build,
            "output: 'standalone' bundle present",
        )
        .docs(DOCS_CONFIG),
        "routing-rule" | "route-regex" => finding(
            "NEXTRS-DOC-220",
            Level::Error,
            Category::Compatibility,
            "Routing rule NextRS cannot apply",
        )
        .support(Support::Unsupported)
        .suggest("nextrs start refuses this build; simplify the rule or route pattern named above.")
        .docs(DOCS_COMPAT_UNSUPPORTED),
        "images-config" => finding(
            "NEXTRS-DOC-221",
            Level::Error,
            Category::Compatibility,
            "Image configuration NextRS cannot apply",
        )
        .support(Support::Unsupported)
        .docs(DOCS_COMPAT_UNSUPPORTED),
        _ => match d.severity {
            Severity::Unsupported => finding(
                "NEXTRS-DOC-190",
                Level::Error,
                Category::Compatibility,
                &format!("Unsupported: {}", d.code),
            )
            .support(Support::Unsupported)
            .suggest("nextrs start refuses builds with unsupported features.")
            .docs(DOCS_COMPAT_UNSUPPORTED),
            Severity::Warning => finding(
                "NEXTRS-DOC-191",
                Level::Warning,
                Category::Build,
                &format!("Inspection warning: {}", d.code),
            ),
            Severity::Info => finding(
                "NEXTRS-DOC-192",
                Level::Info,
                Category::Build,
                &format!("Inspection note: {}", d.code),
            ),
        },
    })
}

/// One adapter message spans source lines with runs of spaces.
pub fn squash(message: &str) -> String {
    message.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether `dir` looks like a `.next` directory itself (as `inspect`
/// accepts one).
pub fn is_dist_dir(dir: &Path) -> bool {
    dir.join("BUILD_ID").is_file() && dir.join("server").is_dir()
}
