use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use nextrs_adapter::{
    BuildInfo, InspectOptions, Matcher, MetadataSource, Rendering, RouteKind, Runtime, Severity,
};
use nextrs_runtime::{
    AddrRange, AllowRule, CacheConfig, Executor, ExecutorConfig, Limits, NetworkConfig,
    NetworkLimits, NetworkPolicy,
};
use nextrs_server::{ServeOptions, ServerOptions};

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
// Parsed once at startup; the size difference costs nothing.
#[allow(clippy::large_enum_variant)]
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
    /// Serve a build. Prerendered output is served from Rust; dynamic routes
    /// need `--js-runtime embedded-v8`, or they answer 501.
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
        /// How to execute dynamic routes.
        #[arg(long, value_enum, default_value_t = JsRuntime::None)]
        js_runtime: JsRuntime,
        /// Host environment variable to expose to JavaScript (repeatable).
        /// Nothing else is inherited; NODE_ENV is always "production".
        #[arg(long = "js-env", value_name = "NAME")]
        js_env: Vec<String>,
        #[command(flatten)]
        limits: LimitArgs,
        #[command(flatten)]
        net: NetArgs,
        #[command(flatten)]
        cache: CacheArgs,
    },
}

/// The data cache (fetch cache, unstable_cache) shared by all isolates.
#[derive(clap::Args)]
#[command(next_help_heading = "Data cache")]
struct CacheArgs {
    /// Persist the data cache in this directory (created with mode 0700),
    /// so entries survive restarts. Must be outside the build and
    /// node_modules. Without it the cache is in memory only.
    #[arg(long, value_name = "DIR")]
    cache_dir: Option<PathBuf>,
    /// Entry bytes kept in memory.
    #[arg(long, value_name = "MIB", default_value_t = 50)]
    cache_max_memory: u64,
    /// Entry bytes kept on disk (with --cache-dir).
    #[arg(long, value_name = "MIB", default_value_t = 1024)]
    cache_max_disk: u64,
    /// Larger entries are not cached (Next's own limit is 2 MB).
    #[arg(long, value_name = "MIB", default_value_t = 2)]
    cache_max_entry: u64,
    /// Leave caching to Next's default handler: separate per isolate,
    /// in memory only.
    #[arg(long, conflicts_with = "cache_dir")]
    no_shared_cache: bool,
}

impl CacheArgs {
    fn config(
        &self,
        read_roots: &[PathBuf],
    ) -> Result<Option<CacheConfig>, Box<dyn std::error::Error>> {
        if self.no_shared_cache {
            return Ok(None);
        }
        let dir = match &self.cache_dir {
            None => None,
            Some(dir) => {
                // Checked before anything is created: the nearest existing
                // ancestor, symlinks resolved, plus the components to create.
                let absolute = std::path::absolute(dir)?;
                let mut existing = absolute.as_path();
                let mut missing = Vec::new();
                while !existing.exists() {
                    missing.push(existing.file_name().ok_or("--cache-dir has no parent")?);
                    existing = existing.parent().ok_or("--cache-dir has no parent")?;
                }
                let mut resolved = std::fs::canonicalize(existing)?;
                for part in missing.iter().rev() {
                    if *part == ".." || *part == "." {
                        return Err("--cache-dir must not contain '.' or '..' components".into());
                    }
                    resolved.push(part);
                }
                if let Some(root) = read_roots.iter().find(|root| resolved.starts_with(root)) {
                    return Err(format!(
                        "--cache-dir {} is inside {}, which JavaScript can read; choose a directory outside the build and node_modules",
                        dir.display(),
                        root.display()
                    )
                    .into());
                }
                std::fs::create_dir_all(&resolved)?;
                Some(resolved)
            }
        };
        let mib = |value: u64| value.max(1) * 1024 * 1024;
        Ok(Some(CacheConfig {
            dir,
            max_memory_bytes: mib(self.cache_max_memory),
            max_disk_bytes: mib(self.cache_max_disk),
            max_entry_bytes: mib(self.cache_max_entry),
        }))
    }
}

/// Outbound `fetch`. Denied entirely unless `--net-allow` is given.
#[derive(clap::Args)]
#[command(next_help_heading = "Outbound network (fetch)")]
struct NetArgs {
    /// Destination JavaScript may fetch (repeatable): `host`, `host:port`,
    /// `scheme://host[:port]` or `*.domain`. Without a scheme only https;
    /// without a port only the scheme's default. Hosts must also resolve to
    /// public addresses (see --net-allow-addr).
    #[arg(long = "net-allow", value_name = "ORIGIN")]
    allow: Vec<String>,
    /// Non-public address or CIDR range that allowed hosts may resolve to
    /// (repeatable), e.g. 127.0.0.1 for a local development API. Loopback,
    /// private, link-local (cloud metadata) and other special ranges are
    /// refused otherwise.
    #[arg(long = "net-allow-addr", value_name = "CIDR")]
    allow_addr: Vec<String>,
    /// From sending a request to its response head, redirects included.
    #[arg(long = "net-timeout", value_name = "SECONDS", default_value_t = 30.0)]
    timeout: f64,
    /// Longest wait for the next chunk of a response body.
    #[arg(
        long = "net-body-timeout",
        value_name = "SECONDS",
        default_value_t = 30.0
    )]
    body_timeout: f64,
    /// Response body limit per fetch (after decompression).
    #[arg(long = "net-max-response", value_name = "MIB", default_value_t = 32)]
    max_response: u64,
}

impl NetArgs {
    fn config(&self) -> Result<Option<NetworkConfig>, Box<dyn std::error::Error>> {
        if self.allow.is_empty() {
            if !self.allow_addr.is_empty() {
                return Err("--net-allow-addr needs at least one --net-allow".into());
            }
            return Ok(None);
        }
        let allow = self
            .allow
            .iter()
            .map(|rule| {
                rule.parse::<AllowRule>()
                    .map_err(|e| format!("--net-allow {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let allow_addrs = self
            .allow_addr
            .iter()
            .map(|range| {
                range
                    .parse::<AddrRange>()
                    .map_err(|e| format!("--net-allow-addr {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let defaults = NetworkLimits::default();
        let positive = |value: f64, name: &str| {
            seconds(value).ok_or_else(|| format!("{name} must be positive"))
        };
        Ok(Some(NetworkConfig {
            policy: NetworkPolicy { allow, allow_addrs },
            limits: NetworkLimits {
                headers_timeout: positive(self.timeout, "--net-timeout")?,
                body_idle_timeout: positive(self.body_timeout, "--net-body-timeout")?,
                max_response_bytes: self.max_response.max(1) * 1024 * 1024,
                ..defaults
            },
        }))
    }
}

/// Runtime limits. Durations are in seconds; 0 disables a timeout.
#[derive(clap::Args)]
#[command(next_help_heading = "Runtime limits")]
struct LimitArgs {
    /// V8 isolates for dynamic routes, each with its own thread, heap and
    /// event loop.
    #[arg(long, default_value_t = 1)]
    isolates: usize,
    /// Requests executing concurrently in one isolate.
    #[arg(long, default_value_t = 128)]
    max_concurrent: usize,
    /// Requests that may wait for an isolate; beyond this they get 503.
    #[arg(long, default_value_t = 512)]
    max_queued: usize,
    /// Longest wait for an isolate before a 503.
    #[arg(long, value_name = "SECONDS", default_value_t = 10.0)]
    queue_timeout: f64,
    /// Deadline per dynamic request, including waitUntil/after() work.
    /// Expiry gives a 504 (or aborts a started response).
    #[arg(long, value_name = "SECONDS", default_value_t = 60.0)]
    request_timeout: f64,
    /// Terminate and replace an isolate whose JavaScript runs this long
    /// without yielding. Affects every request in that isolate.
    #[arg(long, value_name = "SECONDS", default_value_t = 10.0)]
    js_stall_timeout: f64,
    /// V8 heap limit per isolate; reaching it replaces the isolate.
    /// Default: V8's own limit.
    #[arg(long, value_name = "MIB")]
    js_heap_limit: Option<usize>,
    /// Response bytes buffered per request before backpressure applies.
    #[arg(long, value_name = "KIB", default_value_t = 64)]
    response_buffer: usize,
    /// Request bodies above this get a 413 (chunked ones included).
    #[arg(long, value_name = "MIB", default_value_t = 10)]
    max_request_body: u64,
    /// Open connections; more wait in the accept backlog.
    #[arg(long, default_value_t = 1024)]
    max_connections: usize,
    /// Close connections whose client stops reading for this long.
    #[arg(long, value_name = "SECONDS", default_value_t = 30.0)]
    write_timeout: f64,
    /// On SIGINT/SIGTERM, how long in-flight requests may finish.
    #[arg(long, value_name = "SECONDS", default_value_t = 10.0)]
    shutdown_grace: f64,
    /// Experimental: V8 flags, space-separated (e.g.
    /// "--max-semi-space-size=8"). Apply to every isolate. Some flags
    /// weaken the sandbox; for tuning experiments only.
    #[arg(long, value_name = "FLAGS", allow_hyphen_values = true)]
    v8_flags: Option<String>,
    /// Serve executor metrics (/metrics, /metrics.json, /healthz) on this
    /// address. Keep it on loopback.
    #[arg(long, value_name = "ADDR")]
    metrics_addr: Option<SocketAddr>,
}

fn seconds(value: f64) -> Option<Duration> {
    (value > 0.0).then(|| Duration::from_secs_f64(value))
}

impl LimitArgs {
    fn executor_limits(&self) -> Limits {
        Limits {
            isolates: self.isolates.max(1),
            max_active_per_isolate: self.max_concurrent.max(1),
            max_queued: self.max_queued,
            queue_timeout: seconds(self.queue_timeout).unwrap_or(Duration::ZERO),
            request_timeout: seconds(self.request_timeout),
            stall_timeout: seconds(self.js_stall_timeout),
            heap_limit_bytes: self.js_heap_limit.map(|mib| mib * 1024 * 1024),
            response_buffer_bytes: self.response_buffer.max(1) * 1024,
            shutdown_grace: seconds(self.shutdown_grace).unwrap_or(Duration::ZERO),
            ..Limits::default()
        }
    }

    fn serve_options(&self) -> ServeOptions {
        ServeOptions {
            max_connections: self.max_connections.max(1),
            write_timeout: seconds(self.write_timeout),
            shutdown_grace: seconds(self.shutdown_grace).unwrap_or(Duration::ZERO),
            ..ServeOptions::default()
        }
    }
}

/// Resolves on SIGINT or SIGTERM.
async fn shutdown_signal() {
    let interrupt = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = interrupt => {}
        () = terminate => {}
    }
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum JsRuntime {
    /// Never execute JavaScript: dynamic routes answer 501.
    None,
    /// Experimental: Next.js route handlers in an embedded V8 isolate.
    EmbeddedV8,
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
            js_runtime,
            js_env,
            limits,
            net,
            cache,
        } => {
            let network = net.config()?;
            if network.is_some() && js_runtime == JsRuntime::None {
                return Err("--net-allow needs --js-runtime embedded-v8".into());
            }
            let started = std::time::Instant::now();
            let info = nextrs_adapter::inspect_with(
                &path,
                &InspectOptions {
                    ignore_adapter_output,
                },
            )?;
            let executor = match js_runtime {
                JsRuntime::None => None,
                JsRuntime::EmbeddedV8 => {
                    let mut config =
                        ExecutorConfig::new(info.project_dir.clone(), info.dist_dir.clone());
                    config.limits = limits.executor_limits();
                    if let Some(network) = &network {
                        let allow: Vec<String> = network
                            .policy
                            .allow
                            .iter()
                            .map(ToString::to_string)
                            .collect();
                        tracing::info!(allow = %allow.join(", "), "outbound fetch enabled");
                    }
                    config.network = network;
                    config.cache = cache.config(config.fs_policy().read_roots())?;
                    match &config.cache {
                        Some(cache) => tracing::info!(
                            persistent = cache.dir.is_some(),
                            dir = %cache.dir.as_deref().map(|d| d.display().to_string()).unwrap_or_default(),
                            "shared data cache enabled"
                        ),
                        None => tracing::info!(
                            "shared data cache disabled (Next's per-isolate default)"
                        ),
                    }
                    // next start reads the Server Actions key from the
                    // environment before the build's manifest; a
                    // deployment that sets it must get the same key here.
                    // Logged by name only.
                    const ACTION_KEY: &str = "NEXT_SERVER_ACTIONS_ENCRYPTION_KEY";
                    if let Ok(value) = std::env::var(ACTION_KEY) {
                        config.env.insert(ACTION_KEY.to_owned(), value);
                        tracing::info!("{ACTION_KEY} is set; Server Actions use it");
                    }
                    for name in js_env {
                        match std::env::var(&name) {
                            Ok(value) => {
                                config.env.insert(name, value);
                            }
                            Err(_) => tracing::warn!(%name, "--js-env variable is not set"),
                        }
                    }
                    if let Some(flags) = &limits.v8_flags {
                        let flags: Vec<String> =
                            flags.split_whitespace().map(str::to_owned).collect();
                        let unknown = nextrs_runtime::set_v8_flags(&flags);
                        if !unknown.is_empty() {
                            return Err(format!("unknown V8 flags: {}", unknown.join(" ")).into());
                        }
                        tracing::warn!(flags = %flags.join(" "), "experimental V8 flags set");
                    }
                    let executor = Executor::start(config)?;
                    tracing::info!(
                        ready_ms = started.elapsed().as_millis() as u64,
                        isolates = executor.limits().isolates,
                        "embedded V8 executor ready"
                    );
                    Some(executor)
                }
            };
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            let max_request_body = limits.max_request_body * 1024 * 1024;
            if executor.is_some() && info.server_actions.count > 0 {
                // Two limits apply to an action body: NextRS's own
                // (--max-request-body, enforced before JavaScript runs) and
                // Next's serverActions.bodySizeLimit (enforced by Next's
                // action handler). The lower one is the effective limit.
                let next_limit = info.server_actions.body_size_limit;
                tracing::info!(
                    actions = info.server_actions.count,
                    next_body_limit = next_limit,
                    nextrs_body_limit = max_request_body,
                    effective = next_limit.min(max_request_body),
                    "Server Actions enabled"
                );
                if !executor.as_ref().is_some_and(|e| e.has_shared_cache()) {
                    tracing::warn!(
                        "--no-shared-cache: revalidatePath/revalidateTag/updateTag called by \
                         Server Actions stay inside one isolate, and prerendered pages are not \
                         refreshed after them"
                    );
                }
                if next_limit > max_request_body {
                    tracing::warn!(
                        "serverActions.bodySizeLimit ({}) is above --max-request-body ({} MiB): \
                         action bodies over {max_request_body} bytes get a 413 from NextRS; \
                         raise --max-request-body to accept what Next accepts",
                        info.server_actions
                            .body_size_limit_config
                            .as_deref()
                            .unwrap_or("1 MB"),
                        limits.max_request_body
                    );
                }
            }
            runtime.block_on(async move {
                let options = ServerOptions {
                    allow_unsupported,
                    executor: executor.clone(),
                    max_request_body,
                };
                let app = nextrs_server::app(&info, &options).await?;
                if let Some(metrics_addr) = limits.metrics_addr {
                    let Some(executor) = executor.clone() else {
                        return Err("--metrics-addr needs --js-runtime embedded-v8".into());
                    };
                    if !metrics_addr.ip().is_loopback() {
                        tracing::warn!(%metrics_addr, "serving metrics on a non-loopback address");
                    }
                    let listener = tokio::net::TcpListener::bind(metrics_addr).await?;
                    tracing::info!(%metrics_addr, "metrics listening");
                    tokio::spawn(async move {
                        let _ = axum::serve(listener, nextrs_server::metrics_app(executor)).await;
                    });
                }
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
                nextrs_server::serve(listener, app, limits.serve_options(), shutdown_signal())
                    .await?;
                // Connections are drained (or the grace period is over);
                // now let the isolates finish and stop.
                if let Some(executor) = executor {
                    tracing::info!("stopping JavaScript isolates");
                    tokio::task::spawn_blocking(move || executor.shutdown()).await?;
                }
                tracing::info!("nextrs stopped");
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
        "  NextRS serves: /_next/static, public/, prerendered HTML/RSC/segments;\n  dynamic App Router routes with --js-runtime embedded-v8 (experimental)"
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
