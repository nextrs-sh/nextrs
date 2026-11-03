use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use nextrs_adapter::{
    BuildInfo, InspectOptions, Matcher, MetadataSource, Rendering, RouteKind, Runtime, Severity,
};
use nextrs_server::ServerOptions;

#[derive(Parser)]
#[command(
    name = "nextrs",
    version,
    about = "Experimental Rust runtime for Next.js build output"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Report what a `next build` output contains and what NextRS can serve.
    Inspect {
        /// Next.js project directory (or its `.next` directory).
        path: PathBuf,
        /// Print the full inspection as JSON.
        #[arg(long)]
        json: bool,
        /// Read internal manifests even if adapter output is present.
        #[arg(long)]
        ignore_adapter_output: bool,
    },
    /// Serve the prerendered parts of a build (Phase 0: no JavaScript execution).
    Start {
        path: PathBuf,
        /// Address to bind. Defaults to loopback: this is experimental software.
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        #[arg(long, short, default_value_t = 3000)]
        port: u16,
        /// Start even if the build uses features NextRS would serve incorrectly.
        #[arg(long)]
        allow_unsupported: bool,
        #[arg(long)]
        ignore_adapter_output: bool,
    },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nextrs=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Inspect {
            path,
            json,
            ignore_adapter_output,
        } => {
            let info = nextrs_adapter::inspect_with(
                &path,
                &InspectOptions {
                    ignore_adapter_output,
                },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            } else {
                print!("{}", render_report(&info));
            }
            Ok(())
        }
        Command::Start {
            path,
            host,
            port,
            allow_unsupported,
            ignore_adapter_output,
        } => {
            let started = std::time::Instant::now();
            let info = nextrs_adapter::inspect_with(
                &path,
                &InspectOptions {
                    ignore_adapter_output,
                },
            )?;
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async move {
                let app = nextrs_server::app(&info, &ServerOptions { allow_unsupported }).await?;
                let addr = SocketAddr::new(host, port);
                if !host.is_loopback() {
                    tracing::warn!(%addr, "binding a non-loopback address; NextRS is experimental");
                }
                let listener = tokio::net::TcpListener::bind(addr).await?;
                tracing::info!(
                    %addr,
                    build_id = %info.build_id,
                    ready_ms = started.elapsed().as_millis() as u64,
                    "nextrs ready"
                );
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = tokio::signal::ctrl_c().await;
                    })
                    .await?;
                Ok::<_, Box<dyn std::error::Error>>(())
            })
        }
    }
}

fn render_report(info: &BuildInfo) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let detected = |yes: bool| if yes { "detected" } else { "not detected" };
    let _ = writeln!(out, "NextRS Inspector\n");
    let _ = writeln!(out, "Project:          {}", info.project_dir.display());
    match &info.next_version {
        Some(v) => {
            let _ = writeln!(out, "Next.js version:  {} (from {})", v.version, v.source);
        }
        None => {
            let _ = writeln!(out, "Next.js version:  unknown");
        }
    }
    let _ = writeln!(out, "Build ID:         {}", info.build_id);
    let source = match info.source {
        MetadataSource::AdapterApi => "Deployment Adapter API (.next/nextrs/build-output.json)",
        MetadataSource::InternalManifests => "internal .next manifests (undocumented)",
    };
    let _ = writeln!(out, "Metadata source:  {source}");
    let _ = writeln!(out, "App Router:       {}", detected(info.routers.app));
    let _ = writeln!(out, "Pages Router:     {}", detected(info.routers.pages));

    let _ = writeln!(out, "\nRoutes:");
    let width = info.routes.iter().map(|r| r.page.len()).max().unwrap_or(0);
    for route in &info.routes {
        let kind = match route.kind {
            RouteKind::AppPage => "page",
            RouteKind::AppRoute => "route handler",
            RouteKind::PagesPage => "pages page",
            RouteKind::PagesApi => "pages api",
        };
        let rendering = match &route.rendering {
            Rendering::Static => "prerendered".to_owned(),
            Rendering::Revalidated { seconds } => format!("ISR (revalidate {seconds}s)"),
            Rendering::Dynamic => "dynamic".to_owned(),
            Rendering::Unknown { reason } => format!("uncertain: {reason}"),
        };
        let mut notes = Vec::new();
        if route.internal {
            notes.push("framework".to_owned());
        }
        if route.runtime == Runtime::Edge {
            notes.push("edge runtime".to_owned());
        }
        if let Matcher::Pattern { params, .. } = &route.matcher {
            notes.push(format!("params: {}", params.join(", ")));
        }
        if let Some(p) = info.prerender(&route.page) {
            let mut stored = Vec::new();
            if p.html.is_some() {
                stored.push("html".to_owned());
            }
            if p.rsc.is_some() {
                stored.push("rsc".to_owned());
            }
            if !p.segments.is_empty() {
                stored.push(format!("{} segments", p.segments.len()));
            }
            notes.push(format!("stored: {}", stored.join(" + ")));
        }
        let notes = if notes.is_empty() {
            String::new()
        } else {
            format!("  ({})", notes.join("; "))
        };
        let _ = writeln!(
            out,
            "  {:<width$}  {:<13}  {rendering}{notes}",
            route.page, kind
        );
    }

    let needs_js: Vec<&str> = info
        .routes
        .iter()
        .filter(|r| !r.internal && r.rendering != Rendering::Static)
        .map(|r| r.page.as_str())
        .collect();
    let node_runtime = info
        .routes
        .iter()
        .any(|r| r.runtime == Runtime::Nodejs && r.rendering != Rendering::Static);
    let _ = writeln!(out, "\nRuntime requirements:");
    if needs_js.is_empty() {
        let _ = writeln!(
            out,
            "  JavaScript execution: not required (fully prerendered)"
        );
    } else {
        let _ = writeln!(
            out,
            "  JavaScript execution: required for {} route(s): {}",
            needs_js.len(),
            needs_js.join(", ")
        );
    }
    let _ = writeln!(
        out,
        "  Node.js APIs:         {}",
        if node_runtime {
            "required (nodejs-runtime handlers take IncomingMessage/ServerResponse); not provided by NextRS Phase 0"
        } else {
            "not required by any non-static route"
        }
    );
    let _ = writeln!(
        out,
        "  NextRS Phase 0 serves: /_next/static, public/, prerendered HTML/RSC/segments"
    );

    let _ = writeln!(out, "\nDiagnostics:");
    let mut diagnostics: Vec<_> = info.diagnostics.iter().collect();
    diagnostics.sort_by_key(|d| std::cmp::Reverse(d.severity));
    for d in diagnostics {
        let level = match d.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Unsupported => "unsupported",
        };
        let _ = writeln!(out, "  [{level}] {}: {}", d.code, d.message);
    }
    out
}
