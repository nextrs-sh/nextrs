//! `nextrs doctor`: read-only diagnostics of whether a Next.js project and
//! its production build suit NextRS.
//!
//! The checks reuse what `nextrs start` runs: the adapter's inspection
//! (`nextrs_adapter::inspect_with`) and its diagnostics, the version
//! policy, the server's startup checks (`nextrs_server::app`, without an
//! executor or a socket) and the same flag parsers. Doctor never runs
//! JavaScript, `next build` or a server, creates nothing, writes nothing and
//! makes no network request.
//!
//! - `environment`: platform, C library, project, installed Next.js;
//! - `build`: the production build and the startup checks;
//! - `compat`: framework features, classified with the capability matrix;
//! - `node`: Node.js APIs, from Next's file traces;
//! - `config`: runtime flags;
//! - `report`: findings, status and exit code; `render`: text and JSON;
//! - `fsread`: reading project files without leaving the project.

mod build;
mod compat;
mod config;
mod environment;
mod fsread;
mod node;
mod render;
mod report;

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nextrs_adapter::{BuildInfo, InspectOptions, MetadataSource, Rendering};

use report::{EnvironmentInfo, Finding, ProjectInfo, Report};

pub(crate) const DOCS_COMPAT_MATRIX: &str =
    "https://github.com/nextrs-sh/nextrs/blob/main/engineering/compatibility.md#capability-matrix";
pub(crate) const DOCS_COMPAT_GAPS: &str =
    "https://github.com/nextrs-sh/nextrs/blob/main/engineering/compatibility.md#known-gaps";
pub(crate) const DOCS_COMPAT_UNSUPPORTED: &str = "https://github.com/nextrs-sh/nextrs/blob/main/engineering/compatibility.md#unsupported-features";
pub(crate) const DOCS_COMPAT_VERSIONS: &str = "https://github.com/nextrs-sh/nextrs/blob/main/engineering/compatibility.md#tested-versions-and-fixtures";
pub(crate) const DOCS_CONFIG: &str =
    "https://github.com/nextrs-sh/nextrs/blob/main/engineering/configuration.md";
pub(crate) const DOCS_INSTALL: &str =
    "https://github.com/nextrs-sh/nextrs/blob/main/engineering/installation.md";
pub(crate) const DOCS_SECURITY: &str =
    "https://github.com/nextrs-sh/nextrs/blob/main/engineering/security.md";

/// Exit code for invalid arguments or a check that could not run (clap
/// uses it for argument errors too).
pub const EXIT_INVOCATION: u8 = 2;

#[derive(clap::Args)]
pub struct DoctorArgs {
    /// Next.js project directory (or its `.next` directory).
    #[arg(default_value = ".")]
    pub path: PathBuf,
    /// Print one JSON document (schema version 1) on stdout.
    #[arg(long)]
    pub json: bool,
    /// Exit with 1 on warnings too, not only on confirmed blockers.
    /// Unknown results never fail.
    #[arg(long)]
    pub strict: bool,
    /// Read internal manifests even if adapter output is present (as
    /// nextrs start --ignore-adapter-output).
    #[arg(long)]
    pub ignore_adapter_output: bool,
    #[command(flatten)]
    pub runtime: RuntimeArgs,
    #[command(flatten)]
    pub net: crate::NetArgs,
    #[command(flatten)]
    pub proxy: crate::ProxyArgs,
}

/// The `nextrs start` options Doctor can check without starting anything.
/// All optional: without them Doctor reports what the build needs.
#[derive(clap::Args)]
#[command(next_help_heading = "Runtime options to check (as for nextrs start)")]
pub struct RuntimeArgs {
    /// The --js-runtime you will start with. Without it, Doctor assumes
    /// embedded-v8 for compatibility and tells you when the build needs it.
    #[arg(long, value_enum)]
    pub js_runtime: Option<crate::JsRuntime>,
    /// The --host you will bind.
    #[arg(long)]
    pub host: Option<IpAddr>,
    /// The --isolates you will start with (default 1).
    #[arg(long, value_name = "N")]
    pub isolates: Option<usize>,
    /// A --js-env name you will pass (repeatable). Doctor checks that it is
    /// set here; values are never read into the report.
    #[arg(long = "js-env", value_name = "NAME")]
    pub js_env: Vec<String>,
    /// The --max-request-body you will start with (default 10).
    #[arg(long, value_name = "MIB")]
    pub max_request_body: Option<u64>,
    /// The --cache-dir you will start with. Checked, never created.
    #[arg(long, value_name = "DIR")]
    pub cache_dir: Option<PathBuf>,
    /// You will start with --no-shared-cache.
    #[arg(long, conflicts_with = "cache_dir")]
    pub no_shared_cache: bool,
    /// The --image-cache-dir you will start with. Checked, never created.
    #[arg(long, value_name = "DIR")]
    pub image_cache_dir: Option<PathBuf>,
}

/// Whether some route or feature runs JavaScript.
pub(crate) fn needs_javascript(info: &BuildInfo) -> bool {
    info.routes
        .iter()
        .any(|r| !r.internal && r.rendering != Rendering::Static)
        || info.proxy.is_some()
        || info.server_actions.count > 0
        || info.prerenders.iter().any(|p| p.revalidate.is_some())
}

/// Replaces machine-specific absolute paths in report text: the project
/// directory, the workspace root and the home directory.
struct Sanitizer(Vec<(String, String)>);

impl Sanitizer {
    fn new(project: &Path, workspace: Option<&Path>) -> Self {
        let mut pairs = vec![(project.display().to_string(), ".".to_owned())];
        if let Some(root) = workspace {
            pairs.push((root.display().to_string(), "<workspace>".to_owned()));
        }
        if let Some(home) = std::env::var_os("HOME").filter(|h| h.len() > 1) {
            pairs.push((PathBuf::from(home).display().to_string(), "~".to_owned()));
        }
        // Longest first: the project lies inside the others.
        pairs.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
        pairs.retain(|(from, _)| from.len() > 1);
        Self(pairs)
    }

    fn apply(&self, text: &str) -> String {
        let mut text = fsread::printable(text);
        for (from, to) in &self.0 {
            text = text.replace(from.as_str(), to);
        }
        text
    }

    fn finding(&self, mut f: Finding) -> Finding {
        f.title = self.apply(&f.title);
        f.message = self.apply(&f.message);
        f.component = f.component.map(|c| self.apply(&c));
        f.suggestion = f.suggestion.map(|s| self.apply(&s));
        f
    }
}

/// Runs every check. `Err` is an invocation error (exit 2).
pub fn diagnose(args: &DoctorArgs, env_is_set: &dyn Fn(&str) -> bool) -> Result<Report, String> {
    let given = args.path.display().to_string();
    let path = args
        .path
        .canonicalize()
        .map_err(|e| format!("{given}: {e}"))?;
    if !path.is_dir() {
        return Err(format!("{given} is not a directory"));
    }
    // A `.next` directory itself is accepted, as by nextrs inspect.
    let project_dir = if build::is_dist_dir(&path) {
        path.parent().unwrap_or(&path).to_path_buf()
    } else {
        path.clone()
    };

    let mut findings = Vec::new();
    let glibc = environment::glibc_version();
    findings.extend(environment::platform_findings(
        std::env::consts::OS,
        std::env::consts::ARCH,
        environment::libc_family(),
        glibc.as_deref(),
    ));

    let dist = if build::is_dist_dir(&path) {
        path.clone()
    } else {
        path.join(".next")
    };
    let (info, complete) = if let Some(file) = build::metadata_outside_build(&dist) {
        findings.push(build::metadata_outside_build_finding(&file));
        (None, false)
    } else {
        match nextrs_adapter::inspect_with(
            &path,
            &InspectOptions {
                ignore_adapter_output: args.ignore_adapter_output,
            },
        ) {
            Ok(info) => (Some(info), true),
            Err(error) => {
                findings.push(build::inspect_failure(&error));
                (None, false)
            }
        }
    };
    let workspace = info.as_ref().and_then(|i| i.workspace_root.clone());

    let package = environment::read_package(&project_dir);
    let has_config = ["js", "mjs", "cjs", "ts", "mts"]
        .iter()
        .any(|ext| project_dir.join(format!("next.config.{ext}")).is_file());
    let installed = environment::installed_next(&project_dir)
        .or_else(|| workspace.as_deref().and_then(environment::installed_next));
    findings.extend(environment::project_findings(
        package.as_ref(),
        has_config,
        installed.as_deref(),
        info.is_some(),
    ));

    let network = args.net.config().map_err(|e| e.to_string());
    if let Some(info) = &info {
        findings.push(build::build_found(info));
        findings.extend(build::version_findings(info));
        findings.extend(build::runtime_files_findings(
            info,
            installed.as_deref(),
            args.runtime.js_runtime == Some(crate::JsRuntime::None),
        ));
        findings.push(match build::preflight_guard(info) {
            Ok(()) => build::preflight(info, network.as_ref().ok().cloned().flatten()),
            Err(reason) => build::preflight_skipped(&reason),
        });
        findings.extend(info.diagnostics.iter().filter_map(build::passthrough));
        findings.extend(compat::findings(info));
        findings.extend(node::findings(info));
    }
    findings.extend(config::findings(&config::Inputs {
        args,
        info: info.as_ref(),
        project_dir: &project_dir,
        network: &network,
        env_is_set,
    }));

    let sanitizer = Sanitizer::new(&project_dir, workspace.as_deref());
    let findings = findings.into_iter().map(|f| sanitizer.finding(f)).collect();
    let printable = |s: &String| fsread::printable(s);
    let project = ProjectInfo {
        path: fsread::printable(&given),
        name: package
            .as_ref()
            .and_then(|p| p.name.as_ref())
            .map(printable),
        nextjs_project: package.as_ref().is_some_and(|p| p.next_range.is_some()),
        build_found: info.is_some(),
        build_id: info.as_ref().map(|i| printable(&i.build_id)),
        metadata_source: info.as_ref().map(|i| match i.source {
            MetadataSource::AdapterApi => "adapter-api",
            MetadataSource::InternalManifests => "internal-manifests",
        }),
        app_router: info.as_ref().map(|i| i.routers.app),
        pages_router: info.as_ref().map(|i| i.routers.pages),
    };
    let environment = EnvironmentInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        libc: environment::libc_family(),
        glibc_version: glibc,
        next_installed: installed,
        next_build: info
            .as_ref()
            .and_then(|i| i.next_version.as_ref())
            .map(|v| printable(&v.version)),
        next_build_source: info
            .as_ref()
            .and_then(|i| i.next_version.as_ref())
            .map(|v| printable(&v.source)),
    };
    Ok(Report::finish(
        project,
        environment,
        findings,
        complete,
        args.strict,
    ))
}

/// `nextrs doctor`: prints the report and returns its exit code.
pub fn run(args: DoctorArgs) -> ExitCode {
    let env_is_set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    match diagnose(&args, &env_is_set) {
        Ok(report) => {
            let out = if args.json {
                render::json(&report) + "\n"
            } else {
                render::text(&report, render::Style::detect())
            };
            // Not print!: it panics when stdout is closed early
            // (`nextrs doctor --json | head -c1`). A reader that stopped
            // reading is not an error; the exit code still reports the
            // findings. Any other write failure means no report was given.
            use std::io::Write;
            let mut stdout = std::io::stdout().lock();
            match stdout
                .write_all(out.as_bytes())
                .and_then(|()| stdout.flush())
            {
                Ok(()) => ExitCode::from(report.exit_code),
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                    ExitCode::from(report.exit_code)
                }
                Err(error) => {
                    eprintln!("error: nextrs doctor: writing the report: {error}");
                    ExitCode::from(EXIT_INVOCATION)
                }
            }
        }
        Err(error) => {
            eprintln!("error: nextrs doctor: {error}");
            ExitCode::from(EXIT_INVOCATION)
        }
    }
}

#[cfg(test)]
mod tests;
