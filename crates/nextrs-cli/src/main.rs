mod doctor;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use nextrs_adapter::{
    BuildInfo, DEPLOYMENT_ID_ENV, InspectOptions, Matcher, MetadataSource, Rendering, RouteKind,
    Runtime, Severity,
};
use nextrs_runtime::{
    AddrRange, AllowRule, CacheConfig, Executor, ExecutorConfig, Limits, NetworkConfig,
    NetworkLimits, NetworkPolicy,
};
use nextrs_server::{
    ForwardedHeaders, HttpMetrics, MetricsSources, ReclaimPolicy, ReclaimStats, ServeOptions,
    ServerOptions,
};

#[derive(Parser)]
#[command(
    name = "nextrs",
    version,
    about = "Experimental Rust runtime for Next.js build output"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Log line format on stderr: text, or json (one object per line, for
    /// log collectors). NEXTRS_LOG_FORMAT sets the default. Levels and
    /// targets come from RUST_LOG (default nextrs=info).
    // The display slot it had before `doctor` was added (clap numbers
    // subcommands and this global option together), so `nextrs start
    // --help` keeps its order.
    #[arg(long, global = true, value_enum, display_order = 2)]
    log_format: Option<LogFormat>,
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum LogFormat {
    Text,
    Json,
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
        resources: ResourceArgs,
        #[command(flatten)]
        net: NetArgs,
        #[command(flatten)]
        cache: CacheArgs,
        #[command(flatten)]
        proxy: ProxyArgs,
        #[command(flatten)]
        images: ImageArgs,
    },
    /// Check, without running anything, whether a Next.js project and its
    /// production build suit NextRS. Exit 0: no confirmed blockers (not a
    /// guarantee of compatibility); 1: blockers (with --strict, warnings
    /// too); 2: invalid arguments or the check could not run.
    Doctor(doctor::DoctorArgs),
}

/// `/_next/image`: transformations in Rust, a bounded pool, a cache.
#[derive(clap::Args)]
#[command(next_help_heading = "Image optimization")]
struct ImageArgs {
    /// Image transformations running at once, each on its own thread.
    /// Default: half the CPUs.
    #[arg(long, value_name = "N")]
    image_concurrency: Option<usize>,
    /// Transformations that may wait for a thread; beyond this, 503.
    #[arg(long, value_name = "N", default_value_t = 64)]
    image_max_queued: usize,
    /// Larger source images (width x height) are served unchanged, as next
    /// start does when sharp refuses them. The build's
    /// experimental.imgOptMaxInputPixels applies when lower.
    #[arg(long, value_name = "PIXELS", default_value_t = 64_000_000)]
    image_max_input_pixels: u64,
    /// Longest transformation; then the source is served unchanged.
    /// Default: the build's experimental.imgOptTimeoutInSeconds (7).
    #[arg(long, value_name = "SECONDS")]
    image_timeout: Option<f64>,
    /// Memory image transformations may hold at once. Each reserves what
    /// its source's header says it will allocate before it starts; work
    /// larger than the whole budget is served unchanged.
    #[arg(long, value_name = "MIB", default_value_t = 1024)]
    image_memory_budget: u64,
    /// Source image bytes held in memory at once (downloading, waiting,
    /// being transformed); beyond it, new image work answers 503.
    #[arg(long, value_name = "MIB", default_value_t = 512)]
    image_source_budget: u64,
    /// glibc's mmap threshold: allocations of at least this many KiB are
    /// returned to the system when freed (image buffers otherwise stay
    /// resident in malloc's arenas). 0 leaves glibc's adaptive default;
    /// MALLOC_MMAP_THRESHOLD_ or GLIBC_TUNABLES take precedence.
    #[arg(long, value_name = "KIB", default_value_t = 1024)]
    malloc_mmap_threshold: usize,
    /// Persist optimized images in this directory (created with mode
    /// 0700; outside the project). Without it they are cached in memory.
    #[arg(long, value_name = "DIR")]
    image_cache_dir: Option<PathBuf>,
    /// Optimized image bytes kept in memory.
    #[arg(long, value_name = "MIB", default_value_t = 64)]
    image_cache_max_memory: u64,
    /// Optimized image bytes kept on disk (with --image-cache-dir); the
    /// build's images.maximumDiskCacheSize applies when lower.
    #[arg(long, value_name = "MIB", default_value_t = 1024)]
    image_cache_max_disk: u64,
}

impl ImageArgs {
    fn options(
        &self,
        network: Option<NetworkConfig>,
        project_dir: &std::path::Path,
    ) -> Result<nextrs_server::ImageOptions, Box<dyn std::error::Error>> {
        let defaults = nextrs_server::ImageOptions::default();
        let dir = match &self.image_cache_dir {
            Some(dir) => Some(checked_dir(
                dir,
                &[project_dir.to_path_buf()],
                "--image-cache-dir",
            )?),
            None => None,
        };
        let mib = |value: u64| value.max(1) * 1024 * 1024;
        Ok(nextrs_server::ImageOptions {
            network,
            concurrency: self
                .image_concurrency
                .unwrap_or(defaults.concurrency)
                .max(1),
            max_queued: self.image_max_queued,
            max_input_pixels: self.image_max_input_pixels.max(1),
            timeout: match self.image_timeout {
                Some(value) => Some(seconds(value).ok_or("--image-timeout must be positive")?),
                None => None,
            },
            memory_budget: mib(self.image_memory_budget),
            source_budget: mib(self.image_source_budget),
            cache: nextrs_server::ImageCacheOptions {
                dir,
                max_memory_bytes: mib(self.image_cache_max_memory),
                max_disk_bytes: mib(self.image_cache_max_disk),
                ..defaults.cache
            },
            ..defaults
        })
    }
}

/// Resolves `dir` (symlinks in existing ancestors included) and refuses it
/// inside one of `forbidden`; does not create it.
fn checked_dir(
    dir: &std::path::Path,
    forbidden: &[PathBuf],
    flag: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let absolute = std::path::absolute(dir)?;
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    while !existing.exists() {
        missing.push(
            existing
                .file_name()
                .ok_or(format!("{flag} has no parent"))?,
        );
        existing = existing.parent().ok_or(format!("{flag} has no parent"))?;
    }
    let mut resolved = std::fs::canonicalize(existing)?;
    for part in missing.iter().rev() {
        if *part == ".." || *part == "." {
            return Err(format!("{flag} must not contain '.' or '..' components").into());
        }
        resolved.push(part);
    }
    if let Some(root) = forbidden.iter().find(|root| resolved.starts_with(root)) {
        return Err(format!(
            "{flag} {} is inside {}; choose a directory outside the build and node_modules",
            dir.display(),
            root.display()
        )
        .into());
    }
    Ok(resolved)
}

/// Reverse proxies and the X-Forwarded-* headers Next.js trusts.
#[derive(clap::Args)]
#[command(next_help_heading = "Reverse proxy")]
struct ProxyArgs {
    /// Address or CIDR range of a reverse proxy (repeatable). Then only
    /// connections from these addresses keep X-Forwarded-Host/-Proto/-Port/
    /// -For and Forwarded; they are removed from every other request before
    /// Next.js (which uses them for Server Action origin checks, redirects
    /// and absolute URLs) sees it. Without this or --strip-forwarded-headers
    /// they are passed through from every client, as next start does, and
    /// the proxy in front must overwrite them.
    #[arg(
        long = "trusted-proxy",
        value_name = "CIDR",
        conflicts_with = "strip_forwarded_headers"
    )]
    trusted_proxy: Vec<String>,
    /// Remove forwarded headers from every request (no proxy in front).
    #[arg(long)]
    strip_forwarded_headers: bool,
}

impl ProxyArgs {
    fn policy(&self) -> Result<ForwardedHeaders, Box<dyn std::error::Error>> {
        if self.strip_forwarded_headers {
            return Ok(ForwardedHeaders::Strip);
        }
        if self.trusted_proxy.is_empty() {
            return Ok(ForwardedHeaders::Pass);
        }
        let ranges = self
            .trusted_proxy
            .iter()
            .map(|cidr| {
                cidr.parse::<AddrRange>()
                    .map_err(|e| format!("--trusted-proxy {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ForwardedHeaders::TrustedProxies(ranges))
    }
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
    /// Larger data-cache entries (fetch, unstable_cache) are not cached
    /// (Next's own limit is 2 MB).
    #[arg(long, value_name = "MIB", default_value_t = 2)]
    cache_max_entry: u64,
    /// Larger response-cache entries (ISR pages and route handler output,
    /// all of a page's HTML, RSC and segments) are not cached; such a page
    /// is rendered on every request.
    #[arg(long, value_name = "MIB", default_value_t = 16)]
    cache_max_response: u64,
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
                create_private_dir(&resolved)?;
                Some(resolved)
            }
        };
        let mib = |value: u64| value.max(1) * 1024 * 1024;
        Ok(Some(CacheConfig {
            dir,
            max_memory_bytes: mib(self.cache_max_memory),
            max_disk_bytes: mib(self.cache_max_disk),
            max_entry_bytes: mib(self.cache_max_entry),
            max_response_bytes: mib(self.cache_max_response),
        }))
    }
}

/// Creates a cache directory (and any missing ancestors) with mode 0700, as
/// `--help` promises. An existing directory keeps its mode: the operator chose
/// it. (`create_dir_all` here used the umask, 0755 by default, and the
/// runtime's own 0700 creation then found the directory already present.)
fn create_private_dir(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
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
    /// PEM file of certificate authorities to trust beside the built-in
    /// roots (repeatable), as NODE_EXTRA_CA_CERTS does for next start: a
    /// private CA for internal image or API hosts. Verification is never
    /// turned off.
    #[arg(long = "net-ca-file", value_name = "PEM")]
    ca_file: Vec<PathBuf>,
}

impl NetArgs {
    fn config(&self) -> Result<Option<NetworkConfig>, Box<dyn std::error::Error>> {
        if self.allow.is_empty() {
            if !self.allow_addr.is_empty() {
                return Err("--net-allow-addr needs at least one --net-allow".into());
            }
            if !self.ca_file.is_empty() {
                return Err("--net-ca-file needs at least one --net-allow".into());
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
        let mut config = NetworkConfig {
            policy: NetworkPolicy { allow, allow_addrs },
            limits: NetworkLimits {
                headers_timeout: positive(self.timeout, "--net-timeout")?,
                body_idle_timeout: positive(self.body_timeout, "--net-body-timeout")?,
                max_response_bytes: self.max_response.max(1) * 1024 * 1024,
                ..defaults
            },
            extra_roots: Vec::new(),
        };
        for path in &self.ca_file {
            let pem = std::fs::read(path)
                .map_err(|e| format!("--net-ca-file {}: {e}", path.display()))?;
            config
                .add_ca_pem(&pem)
                .map_err(|e| format!("--net-ca-file {}: {e}", path.display()))?;
        }
        Ok(Some(config))
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
    /// Terminate and replace an isolate whose JavaScript runs this long on
    /// the CPU without yielding (thread CPU time; a turn blocked off the CPU,
    /// as on a slow disk or under CPU throttling, is ended only after 6
    /// times this in wall time). Affects every request in that isolate.
    #[arg(long, value_name = "SECONDS", default_value_t = 10.0)]
    js_stall_timeout: f64,
    /// Longest an isolate may take to start (loading Next's bundles); 0
    /// disables it. Expiry fails startup, or restarts the isolate with
    /// backoff after a failure.
    #[arg(long, value_name = "SECONDS", default_value_t = 60.0)]
    js_bootstrap_timeout: f64,
    /// V8 heap limit per isolate; reaching it replaces the isolate. A
    /// number of MiB, or `auto`: budget the container's memory limit (or
    /// the machine's memory) over all --isolates after reserving NextRS's
    /// native memory, and size the rest of each heap as V8 would for that
    /// share. Default: V8's own limit, from the machine's memory (`auto`
    /// with --profile balanced or low-memory). Bounds the JavaScript heap
    /// only, not the process.
    #[arg(long, value_name = "MIB|auto")]
    js_heap_limit: Option<HeapLimit>,
    /// Response bytes buffered per request before backpressure applies.
    #[arg(long, value_name = "KIB", default_value_t = 64)]
    response_buffer: usize,
    /// Request bodies above this get a 413 (chunked ones included).
    #[arg(long, value_name = "MIB", default_value_t = 10)]
    max_request_body: u64,
    /// Memory for gzip copies of immutable responses (/_next/static files
    /// and prerendered pages), each compressed once instead of per
    /// request; least recently used copies are dropped beyond it. 0 turns
    /// the cache off.
    /// [default: 32; 8 with --profile low-memory]
    #[arg(long, value_name = "MIB")]
    compression_cache: Option<usize>,
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
    /// Serve metrics and health on this address: /metrics (Prometheus),
    /// /metrics.json, /healthz (liveness) and /readyz (readiness). Keep it
    /// on loopback or a private network.
    #[arg(long, value_name = "ADDR")]
    metrics_addr: Option<SocketAddr>,
    /// /readyz reports ready only with at least this many isolates ready
    /// (and never once shutdown has begun).
    #[arg(long, value_name = "N", default_value_t = 1)]
    ready_min_isolates: usize,
    /// Preset values for the memory settings below and for
    /// --js-heap-limit and --compression-cache; flags given explicitly
    /// win. `performance` is the default behaviour. See
    /// engineering/configuration.md for what each profile sets.
    #[arg(long, value_enum, value_name = "PROFILE")]
    profile: Option<Profile>,
    /// After this long without requests, every isolate collects garbage
    /// and the native allocator returns its free memory to the system,
    /// once per idle period. Up to about one request per 3 s (health
    /// checks) still counts as idle. 0 disables it; at most 86400. [default: 0;
    /// 30 with --profile balanced, 10 with low-memory]
    #[arg(long, value_name = "SECONDS")]
    memory_reclaim_idle: Option<f64>,
    /// The same, also under load (at most every 10 s), while the
    /// container's working set is at least this percentage of its memory
    /// limit; no effect without a limit. Not a memory limit: the kernel
    /// still enforces the container's. 0 disables it. [default: 0; 90
    /// with --profile balanced, 85 with low-memory]
    #[arg(long, value_name = "PERCENT")]
    memory_pressure_reclaim: Option<f64>,
    /// Native allocator (glibc) arenas. Fewer can mean less retained
    /// memory with many allocating threads, or more (on Velasia, 2 arenas
    /// retained more than the default); measure before using it. 0 keeps
    /// glibc's default (8 per CPU of the machine). MALLOC_ARENA_MAX in the
    /// environment takes precedence. [default: 0]
    #[arg(long, value_name = "N")]
    malloc_arena_max: Option<usize>,
}

/// RE-4 resource controls. All off by default and in every profile; for
/// validation on the target hardware before any becomes a default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum AdmissionAccounting {
    Sampled,
    Committed,
}

#[derive(clap::Args)]
#[command(next_help_heading = "Resource controls (experimental, opt-in)")]
struct ResourceArgs {
    /// Memory-aware admission: while the container's working set is near
    /// its memory budget, JavaScript requests wait (elevated) or get a 503
    /// with Retry-After (critical); one request is always admitted when
    /// none runs. Static files, prerendered pages and health checks are
    /// not affected (unless the build's proxy/middleware runs for them).
    /// Best effort, on measured memory and per-request estimates: not a
    /// memory limit.
    #[arg(long)]
    memory_admission: bool,
    /// Budget admission compares against [default: the container's memory
    /// limit]. Required without a memory cgroup. Set it below the limit
    /// when other processes share the container's cgroup.
    #[arg(long, value_name = "MIB", requires = "memory_admission")]
    memory_admission_budget: Option<u64>,
    /// Working set, as a percentage of the budget, from which concurrency
    /// is capped (--memory-admission-max-active) and the rest wait.
    #[arg(
        long,
        value_name = "PERCENT",
        default_value_t = 85.0,
        requires = "memory_admission"
    )]
    memory_admission_pressure: f64,
    /// Working set from which new requests are refused (503).
    #[arg(
        long,
        value_name = "PERCENT",
        default_value_t = 95.0,
        requires = "memory_admission"
    )]
    memory_admission_critical: f64,
    /// Memory each admitted request is assumed to need (uploads add their
    /// declared length). An estimate; how long it counts is
    /// --memory-admission-accounting.
    #[arg(
        long,
        value_name = "MIB",
        default_value_t = 2,
        requires = "memory_admission"
    )]
    memory_admission_reserve: u64,
    /// JavaScript requests allowed at once, over all isolates, under
    /// memory pressure [default: 4 per isolate].
    #[arg(long, value_name = "N", requires = "memory_admission")]
    memory_admission_max_active: Option<usize>,
    /// How long an admitted request's estimate counts: `sampled`, until the
    /// next memory reading (50 ms); `committed`, until the request ends,
    /// less the working-set growth measured since, at most
    /// --memory-admission-commitment-ttl. `committed` also counts requests
    /// that allocate after waiting (on a backend, a body), and holds more
    /// requests back while estimates are outstanding.
    #[arg(
        long,
        value_name = "POLICY",
        value_enum,
        default_value_t = AdmissionAccounting::Sampled,
        requires = "memory_admission"
    )]
    memory_admission_accounting: AdmissionAccounting,
    /// With committed accounting, the longest one request's estimate
    /// counts (long-lived streams must not hold the headroom).
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 2.0,
        requires = "memory_admission"
    )]
    memory_admission_commitment_ttl: f64,
    /// Working set from which new JavaScript requests are refused even
    /// when none runs, except one per
    /// --memory-admission-emergency-interval [default: no such state;
    /// critical admits one whenever none runs]. At least the critical
    /// threshold.
    #[arg(long, value_name = "PERCENT", requires = "memory_admission")]
    memory_admission_emergency: Option<f64>,
    /// In the emergency state, least time between two requests admitted
    /// while none runs; 0: none until memory falls (an application whose
    /// resident memory alone is above the threshold then serves no
    /// JavaScript).
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 5.0,
        requires = "memory_admission_emergency"
    )]
    memory_admission_emergency_interval: f64,
    /// Limit on ArrayBuffer memory (new ArrayBuffer, typed arrays, Buffer)
    /// over all isolates. An allocation of 64 KiB or more past it fails
    /// with a RangeError in the request that made it, after V8 has
    /// collected garbage; the isolate keeps serving. Buffers made from
    /// Rust (request bodies, fetch chunks, file reads) are not counted.
    #[arg(long, value_name = "MIB")]
    js_external_memory_limit: Option<usize>,
    /// Byte budget for native buffers, over all requests: static and
    /// public/ file bytes read for a response, and request bodies buffered
    /// for the proxy (middleware). Requests wait for room up to
    /// --native-buffer-wait, then get a 503 with Retry-After. Files above a
    /// quarter of the budget (or 4 MiB) are streamed in chunks; a proxy
    /// request body that cannot fit in the budget gets a 413.
    #[arg(long, value_name = "MIB")]
    native_buffer_budget: Option<u64>,
    /// Experimental: a floor for each isolate's semi-space (MiB, rounded
    /// down to a power of two). The young generation (3 × this) takes its
    /// room from the old generation, so --js-heap-limit is unchanged; at
    /// most half the heap. Needs --js-heap-limit (or a profile that sets
    /// it). For CPU measurements; not validated as a default.
    #[arg(long, value_name = "MIB")]
    js_young_semi_space: Option<usize>,
    /// Longest wait for room in --native-buffer-budget.
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 2.0,
        requires = "native_buffer_budget"
    )]
    native_buffer_wait: f64,
}

impl ResourceArgs {
    /// Checks the server-side settings (also without an executor).
    fn validate(&self) -> Result<(), String> {
        if self.native_buffer_budget == Some(0) {
            return Err(
                "--native-buffer-budget: at least 1 MiB (omit the flag to turn it off)".into(),
            );
        }
        if !self.native_buffer_wait.is_finite() || !(0.0..=60.0).contains(&self.native_buffer_wait)
        {
            return Err("--native-buffer-wait: seconds, 0 to 60".into());
        }
        Ok(())
    }

    fn native_buffer_budget(&self) -> Option<Arc<nextrs_server::NativeBufferBudget>> {
        let mib = self.native_buffer_budget.filter(|m| *m > 0)?;
        // Checked in `apply`: finite, 0 to 60.
        let wait = Duration::from_secs_f64(self.native_buffer_wait.clamp(0.0, 60.0));
        tracing::info!(
            budget_mib = mib,
            wait_ms = wait.as_millis() as u64,
            "native buffer budget enabled (static files, proxy request bodies)"
        );
        Some(nextrs_server::NativeBufferBudget::new(
            mib.saturating_mul(1024 * 1024),
            wait,
        ))
    }

    fn apply(&self, limits: &mut Limits) -> Result<(), String> {
        const MIB: u64 = 1024 * 1024;
        if self.memory_admission {
            if self.memory_admission_budget == Some(0) {
                return Err("--memory-admission-budget: at least 1 MiB".into());
            }
            let seconds = |value: f64, flag: &str| {
                if value.is_finite() && (0.0..=3600.0).contains(&value) {
                    Ok(Duration::from_secs_f64(value))
                } else {
                    Err(format!("{flag}: seconds, 0 to 3600"))
                }
            };
            let interval = seconds(
                self.memory_admission_emergency_interval,
                "--memory-admission-emergency-interval",
            )?;
            let admission = nextrs_runtime::MemoryAdmission {
                budget_bytes: self.memory_admission_budget.map(|m| m.saturating_mul(MIB)),
                pressure_percent: self.memory_admission_pressure,
                critical_percent: self.memory_admission_critical,
                request_reserve_bytes: self.memory_admission_reserve.saturating_mul(MIB),
                pressure_max_active: self.memory_admission_max_active.map(|n| n.max(1)),
                accounting: match self.memory_admission_accounting {
                    AdmissionAccounting::Sampled => nextrs_runtime::AdmissionAccounting::Sampled,
                    AdmissionAccounting::Committed => {
                        nextrs_runtime::AdmissionAccounting::Committed
                    }
                },
                commitment_ttl: seconds(
                    self.memory_admission_commitment_ttl,
                    "--memory-admission-commitment-ttl",
                )?,
                emergency_percent: self.memory_admission_emergency,
                emergency_progress_interval: (!interval.is_zero()).then_some(interval),
                ..nextrs_runtime::MemoryAdmission::default()
            };
            admission
                .validate()
                .map_err(|e| format!("--memory-admission: {e}"))?;
            limits.memory_admission = Some(admission);
        }
        if let Some(mib) = self.js_external_memory_limit {
            if mib == 0 {
                return Err("--js-external-memory-limit: at least 1 MiB".into());
            }
            limits.external_memory_limit_bytes = Some(mib.saturating_mul(1024 * 1024));
        }
        self.validate()?;
        if let Some(mib) = self.js_young_semi_space {
            if !(1..=64).contains(&mib) {
                return Err("--js-young-semi-space: 1 to 64 MiB".into());
            }
            if limits.heap_limit_bytes.is_none() && limits.heap_memory_bytes.is_none() {
                tracing::warn!(
                    "--js-young-semi-space has no effect without --js-heap-limit (or a profile \
                     that sets it): V8 sizes the heap itself"
                );
            } else {
                limits.young_semi_space_floor_bytes = Some(mib * 1024 * 1024);
            }
        }
        Ok(())
    }
}

/// `--js-heap-limit`: MiB, or `auto` (from the container's memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeapLimit {
    Auto,
    Mib(usize),
}

impl std::str::FromStr for HeapLimit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        match s.parse::<usize>() {
            Ok(mib) if mib > 0 && mib <= 1 << 20 => Ok(Self::Mib(mib)),
            _ => Err(format!("{s}: a number of MiB (1 to 1048576) or `auto`")),
        }
    }
}

/// `--profile`: documented presets for the memory settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Profile {
    /// Today's defaults: V8's own heap sizing, no reclamation, glibc's
    /// arenas, a 32 MiB compression cache.
    Performance,
    /// Heap sized from the container's memory, idle (30 s) and pressure
    /// (90 %) reclamation.
    Balanced,
    /// As balanced with earlier reclamation (10 s, 85 %) and an 8 MiB
    /// compression cache.
    LowMemory,
}

/// The memory settings in effect: the profile's values, then explicit flags.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MemorySettings {
    heap_limit: Option<HeapLimit>,
    compression_cache_mib: usize,
    reclaim_idle: Option<Duration>,
    pressure_percent: Option<f64>,
    malloc_arena_max: usize,
}

impl MemorySettings {
    fn profile(profile: Profile) -> Self {
        match profile {
            Profile::Performance => Self {
                heap_limit: None,
                compression_cache_mib: 32,
                reclaim_idle: None,
                pressure_percent: None,
                malloc_arena_max: 0,
            },
            Profile::Balanced => Self {
                heap_limit: Some(HeapLimit::Auto),
                compression_cache_mib: 32,
                reclaim_idle: Some(Duration::from_secs(30)),
                pressure_percent: Some(90.0),
                malloc_arena_max: 0,
            },
            // No arena cap: two arenas retained more memory than glibc's
            // default in the RE-2 screening (RE-2-ALLOCATOR-COMPARISON).
            Profile::LowMemory => Self {
                heap_limit: Some(HeapLimit::Auto),
                compression_cache_mib: 8,
                reclaim_idle: Some(Duration::from_secs(10)),
                pressure_percent: Some(85.0),
                malloc_arena_max: 0,
            },
        }
    }
}

/// Applies `--js-heap-limit` (or the profile's) to the executor's limits.
/// `auto` budgets the container's memory over the whole isolate pool
/// ([`nextrs_runtime::heap_budget`]); a number is a per-isolate limit, as
/// given. Logs what was decided and warns when the heaps cannot fit.
fn apply_heap_limit(limits: &mut Limits, memory: &MemorySettings) {
    const MIB: u64 = 1024 * 1024;
    let isolates = limits.isolates.max(1);
    let cgroup = nextrs_runtime::Cgroup::locate();
    let cgroup_memory = cgroup.as_ref().and_then(nextrs_runtime::Cgroup::read);
    let container_limit = cgroup_memory.and_then(|m| m.limit);
    match memory.heap_limit {
        Some(HeapLimit::Mib(mib)) => {
            limits.heap_limit_bytes = Some(mib.saturating_mul(1024 * 1024));
            let pool = u64::try_from(mib)
                .unwrap_or(u64::MAX)
                .saturating_mul(MIB)
                .saturating_mul(u64::try_from(isolates).unwrap_or(u64::MAX));
            if let Some(limit) = container_limit
                && pool >= limit
            {
                tracing::warn!(
                    heap_limit_mib = mib,
                    isolates,
                    container_limit_mib = limit / MIB,
                    "--js-heap-limit: the isolates' heaps together may grow past the container's \
                     memory limit, which the kernel enforces by killing the process; \
                     consider --js-heap-limit auto"
                );
            }
        }
        Some(HeapLimit::Auto) => {
            let physical = nextrs_runtime::physical_memory();
            let available = match (container_limit, physical) {
                (Some(l), Some(p)) => Some(l.min(p)),
                (l, p) => l.or(p),
            };
            let Some(available) = available else {
                tracing::warn!(
                    "--js-heap-limit auto: neither a container limit nor the machine's memory \
                     could be read; V8's default heap sizing applies"
                );
                return;
            };
            let cache = u64::try_from(memory.compression_cache_mib)
                .unwrap_or(u64::MAX)
                .saturating_mul(MIB);
            let budget = nextrs_runtime::heap_budget(available, isolates, cache);
            limits.heap_memory_bytes = Some(budget.sizing_memory);
            limits.heap_limit_bytes =
                Some(usize::try_from(budget.heap_limit).unwrap_or(usize::MAX));
            tracing::info!(
                available_mib = budget.available / MIB,
                source = match (container_limit, cgroup_memory.and_then(|m| m.limit_kind)) {
                    (Some(_), Some(nextrs_runtime::LimitKind::High)) => "cgroup memory.high",
                    (Some(l), _) if physical.is_none_or(|p| l <= p) => "cgroup limit",
                    _ => "physical memory",
                },
                cgroup = %cgroup.as_ref().map_or_else(|| "none".into(), |c| c.dir.display().to_string()),
                isolates,
                reserved_mib = budget.reserved / MIB,
                heap_limit_mib = budget.heap_limit / MIB,
                sizing_memory_mib = budget.sizing_memory / MIB,
                "V8 heaps budgeted from the available memory (--js-heap-limit auto)"
            );
            if budget.clamped {
                tracing::warn!(
                    available_mib = budget.available / MIB,
                    isolates,
                    reserved_mib = budget.reserved / MIB,
                    heap_limit_mib = budget.heap_limit / MIB,
                    "--js-heap-limit auto: the memory left after NextRS's native reserve is too \
                     small for this many isolates; each gets the minimum heap and the process \
                     may not fit the container (use fewer --isolates or more memory)"
                );
            }
        }
        None => {}
    }
}

fn seconds(value: f64) -> Option<Duration> {
    (value > 0.0).then(|| Duration::from_secs_f64(value))
}

impl LimitArgs {
    fn memory(&self) -> Result<MemorySettings, String> {
        let mut m = MemorySettings::profile(self.profile.unwrap_or(Profile::Performance));
        if let Some(limit) = self.js_heap_limit {
            m.heap_limit = Some(limit);
        }
        if let Some(mib) = self.compression_cache {
            m.compression_cache_mib = mib;
        }
        if let Some(secs) = self.memory_reclaim_idle {
            if !secs.is_finite() || !(0.0..=86_400.0).contains(&secs) {
                return Err(format!("--memory-reclaim-idle {secs}: seconds, 0 to 86400"));
            }
            m.reclaim_idle = seconds(secs);
        }
        if let Some(percent) = self.memory_pressure_reclaim {
            if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
                return Err(format!(
                    "--memory-pressure-reclaim {percent}: a percentage, 0 to 100"
                ));
            }
            m.pressure_percent = (percent > 0.0).then_some(percent);
        }
        if let Some(arenas) = self.malloc_arena_max {
            m.malloc_arena_max = arenas;
        }
        Ok(m)
    }

    fn executor_limits(&self) -> Limits {
        Limits {
            isolates: self.isolates.max(1),
            max_active_per_isolate: self.max_concurrent.max(1),
            max_queued: self.max_queued,
            queue_timeout: seconds(self.queue_timeout).unwrap_or(Duration::ZERO),
            request_timeout: seconds(self.request_timeout),
            stall_timeout: seconds(self.js_stall_timeout),
            bootstrap_timeout: seconds(self.js_bootstrap_timeout),
            // `--js-heap-limit` (or the profile) is applied by the caller.
            response_buffer_bytes: self.response_buffer.max(1) * 1024,
            shutdown_grace: seconds(self.shutdown_grace).unwrap_or(Duration::ZERO),
            ..Limits::default()
        }
    }

    fn serve_options(&self, metrics: Arc<HttpMetrics>) -> ServeOptions {
        ServeOptions {
            max_connections: self.max_connections.max(1),
            write_timeout: seconds(self.write_timeout),
            shutdown_grace: seconds(self.shutdown_grace).unwrap_or(Duration::ZERO),
            metrics,
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
    let cli = Cli::parse();
    let format =
        cli.log_format
            .unwrap_or_else(|| match std::env::var("NEXTRS_LOG_FORMAT").as_deref() {
                Ok("json") => LogFormat::Json,
                _ => LogFormat::Text,
            });
    // Doctor reports through its findings; startup logs would repeat them.
    let default_filter = match cli.command {
        Command::Doctor(_) => "off",
        _ => "nextrs=info",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| default_filter.into());
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr);
    match format {
        LogFormat::Text => builder.init(),
        LogFormat::Json => builder
            .json()
            .flatten_event(true)
            .with_current_span(false)
            .init(),
    }

    if let Command::Doctor(args) = cli.command {
        return doctor::run(args);
    }
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Doctor(_) => unreachable!("dispatched in main"),
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
            resources,
            net,
            cache,
            proxy,
            images,
        } => {
            let memory = limits.memory()?;
            resources.validate()?;
            // Before any thread allocates (glibc's arenas are per thread).
            if memory.malloc_arena_max > 0 {
                let applied = nextrs_runtime::set_malloc_arena_max(memory.malloc_arena_max);
                tracing::debug!(
                    applied,
                    arenas = memory.malloc_arena_max,
                    "malloc arena max"
                );
            }
            if images.malloc_mmap_threshold > 0 {
                let applied = nextrs_runtime::set_malloc_mmap_threshold(
                    images.malloc_mmap_threshold.saturating_mul(1024),
                );
                tracing::debug!(
                    applied,
                    kib = images.malloc_mmap_threshold,
                    "malloc mmap threshold"
                );
            }
            let forwarded = proxy.policy()?;
            match &forwarded {
                // Reachable from other hosts with no forwarded-header
                // policy: any client can claim another host or scheme to
                // Next.js (redirect targets, absolute URLs, the Server
                // Action origin check). Kept as next start behaves, but said
                // loudly.
                ForwardedHeaders::Pass if !host.is_loopback() => tracing::warn!(
                    %host,
                    "X-Forwarded-* headers are passed through from every client (as next start): \
                     use --trusted-proxy <proxy address> behind a reverse proxy, or \
                     --strip-forwarded-headers when clients connect directly"
                ),
                ForwardedHeaders::Pass => tracing::info!(
                    "X-Forwarded-* headers are passed through from every client (as next start); \
                     a reverse proxy must overwrite them, or use --trusted-proxy"
                ),
                ForwardedHeaders::TrustedProxies(ranges) => tracing::info!(
                    proxies = ranges.len(),
                    "X-Forwarded-* headers kept only from trusted proxies"
                ),
                ForwardedHeaders::Strip => {
                    tracing::info!("X-Forwarded-* headers removed from every request")
                }
            }
            let network = net.config()?;
            let started = std::time::Instant::now();
            let info = nextrs_adapter::inspect_with(
                &path,
                &InspectOptions {
                    ignore_adapter_output,
                },
            )?;
            // Before the executor loads the build's code: a build of another
            // Next.js major fails there with errors that do not name the
            // cause (or, worse, half works).
            if let Some(d) = info
                .diagnostics
                .iter()
                .find(|d| d.code == nextrs_adapter::UNSUPPORTED_VERSION_CODE)
            {
                if !allow_unsupported {
                    return Err(format!(
                        "{}\npass --allow-unsupported to start anyway (behaviour is undefined)",
                        d.message
                    )
                    .into());
                }
                tracing::warn!(message = %d.message, "starting an unsupported Next.js build (--allow-unsupported)");
            }
            let image_options = images.options(network.clone(), &info.project_dir)?;
            if let Some(config) = &info.images
                && (!config.remote_patterns.is_empty() || !config.domains.is_empty())
                && network.is_none()
            {
                tracing::warn!(
                    "images.remotePatterns is configured but no --net-allow: remote images answer 400"
                );
            }
            let executor = match js_runtime {
                JsRuntime::None => None,
                JsRuntime::EmbeddedV8 => {
                    let mut config =
                        ExecutorConfig::new(info.project_dir.clone(), info.dist_dir.clone());
                    config.limits = limits.executor_limits();
                    apply_heap_limit(&mut config.limits, &memory);
                    resources.apply(&mut config.limits)?;
                    config.build_route_cache = info.route_cache_files();
                    if let Some(root) = &info.workspace_root {
                        tracing::info!(
                            packages = %root.join("node_modules").display(),
                            "monorepo build: the workspace root's packages are readable"
                        );
                        config.workspace_dir = Some(root.clone());
                    }
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
                    // next start's base server assigns its deployment ID to
                    // process.env.NEXT_DEPLOYMENT_ID before any route runs
                    // (an empty string without one); Next's server code
                    // reads it from there. With runtimeServerDeploymentId it
                    // reads the variable instead, and refuses to start
                    // without it.
                    let deployment = &info.config.deployment;
                    let passed = config.env.get(DEPLOYMENT_ID_ENV).cloned();
                    let id = deployment.server_id(passed.as_deref())?;
                    if let Some(passed) = passed.filter(|p| *p != id) {
                        tracing::warn!(
                            passed = %passed,
                            deployment_id = %id,
                            "--js-env {DEPLOYMENT_ID_ENV} replaced by the build's deploymentId, \
                             as next start replaces it"
                        );
                    }
                    if !id.is_empty() {
                        tracing::info!(deployment_id = %id, "deployment ID");
                    }
                    config.env.insert(DEPLOYMENT_ID_ENV.to_owned(), id);
                    if let Some(flags) = &limits.v8_flags {
                        let flags: Vec<String> =
                            flags.split_whitespace().map(str::to_owned).collect();
                        let unknown = nextrs_runtime::set_v8_flags(&flags);
                        if !unknown.is_empty() {
                            return Err(format!("unknown V8 flags: {}", unknown.join(" ")).into());
                        }
                        tracing::warn!(flags = %flags.join(" "), "experimental V8 flags set");
                        // V8 applies these after the embedder's constraints.
                        if memory.heap_limit.is_some()
                            && flags.iter().any(|f| {
                                let f = f.trim_start_matches('-').replace('_', "-");
                                ["max-old-space-size", "max-semi-space-size", "max-heap-size"]
                                    .iter()
                                    .any(|k| f.starts_with(k))
                            })
                        {
                            tracing::warn!(
                                "--v8-flags heap sizing overrides --js-heap-limit; the log line \
                                 'V8 heap limit in effect' shows the result"
                            );
                        }
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
            if let Some(executor) = &executor
                && !executor.has_shared_cache()
            {
                let isr = info.prerenders.iter().any(|p| p.revalidate.is_some());
                if (isr || info.server_actions.count > 0) && executor.limits().isolates > 1 {
                    // Each isolate would keep its own pages and tags: one
                    // isolate serves a page another regenerated, or old
                    // data after a Server Action revalidated it.
                    let message = "--no-shared-cache with more than one isolate: ISR pages and \
                                   revalidations by Server Actions would differ between \
                                   isolates; use the shared cache or --isolates 1";
                    if !allow_unsupported {
                        return Err(format!("{message} (or pass --allow-unsupported)").into());
                    }
                    tracing::warn!("{message}");
                } else if isr {
                    tracing::warn!(
                        "--no-shared-cache: ISR pages are regenerated in memory only (lost on \
                         restart), as with next start's default handler minus its disk copy"
                    );
                }
            }
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
            let compression_cache = Arc::new(nextrs_server::CompressionCache::with_capacity(
                memory.compression_cache_mib.saturating_mul(1024 * 1024),
            ));
            let reclaim_policy = ReclaimPolicy {
                idle_after: memory.reclaim_idle,
                pressure_percent: memory.pressure_percent,
                ..ReclaimPolicy::default()
            };
            if reclaim_policy.is_enabled() {
                tracing::info!(
                    profile = ?limits.profile,
                    idle_seconds = memory.reclaim_idle.map_or(0.0, |d| d.as_secs_f64()),
                    pressure_percent = memory.pressure_percent.unwrap_or(0.0),
                    "memory reclamation enabled"
                );
            }
            runtime.block_on(async move {
                let http_metrics = HttpMetrics::new();
                let reclaim_stats = Arc::new(ReclaimStats::default());
                let _reclaimer = nextrs_server::reclaim::spawn(
                    reclaim_policy,
                    http_metrics.clone(),
                    executor.clone(),
                    reclaim_stats.clone(),
                );
                let native_buffers = resources.native_buffer_budget();
                let options = ServerOptions {
                    allow_unsupported,
                    executor: executor.clone(),
                    max_request_body,
                    forwarded: forwarded.clone(),
                    listen: SocketAddr::new(host, port),
                    images: image_options,
                    compression_cache: compression_cache.clone(),
                    metrics: http_metrics.clone(),
                    native_buffer_budget: native_buffers.clone(),
                };
                let (app, image_stats) = nextrs_server::app_with_stats(&info, &options).await?;
                if let Some(metrics_addr) = limits.metrics_addr {
                    let sources = MetricsSources {
                        executor: executor.clone(),
                        images: image_stats,
                        compression: Some(compression_cache.clone()),
                        http: http_metrics.clone(),
                        ready_min_isolates: limits.ready_min_isolates,
                        reclaim: reclaim_policy.is_enabled().then(|| reclaim_stats.clone()),
                        native_buffers: native_buffers.clone(),
                    };
                    if !metrics_addr.ip().is_loopback() {
                        tracing::warn!(%metrics_addr, "serving metrics on a non-loopback address");
                    }
                    let listener = tokio::net::TcpListener::bind(metrics_addr).await?;
                    tracing::info!(%metrics_addr, "metrics listening");
                    tokio::spawn(async move {
                        let _ = axum::serve(listener, nextrs_server::metrics_app(sources)).await;
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
                nextrs_server::serve(
                    listener,
                    app,
                    limits.serve_options(http_metrics),
                    shutdown_signal(),
                )
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
    if let Some(root) = &info.workspace_root {
        let _ = writeln!(out, "Workspace root:   {} (monorepo)", root.display());
    }
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
        "  NextRS serves: /_next/static, public/, prerendered HTML/RSC/segments and Pages Router HTML;\n  dynamic App Router and Pages Router routes with --js-runtime embedded-v8 (experimental)"
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

#[cfg(test)]
mod tests {
    use super::create_private_dir;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn cache_directories_are_created_private() {
        let base = std::env::temp_dir().join(format!("nextrs-cli-dir-{}", std::process::id()));
        let dir = base.join("a/b/cache");
        create_private_dir(&dir).unwrap();
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&base.join("a")), 0o700);
        // An existing directory is left as the operator made it.
        let existing = base.join("existing");
        std::fs::create_dir(&existing).unwrap();
        std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o750)).unwrap();
        create_private_dir(&existing).unwrap();
        assert_eq!(mode(&existing), 0o750);
        std::fs::remove_dir_all(&base).unwrap();
    }

    mod resource_controls {
        use super::super::{Cli, Command};
        use clap::Parser;
        use nextrs_runtime::Limits;
        use std::time::Duration;

        fn limits(args: &[&str]) -> Result<Limits, String> {
            let mut argv = vec!["nextrs", "start", "."];
            argv.extend_from_slice(args);
            let cli = Cli::try_parse_from(argv).map_err(|e| e.to_string())?;
            match cli.command {
                Command::Start { resources, .. } => {
                    let mut limits = Limits::default();
                    resources.apply(&mut limits)?;
                    Ok(limits)
                }
                _ => unreachable!("parsed a start command"),
            }
        }

        #[test]
        fn everything_is_off_by_default() {
            let l = limits(&[]).unwrap();
            assert!(l.memory_admission.is_none());
            assert!(l.external_memory_limit_bytes.is_none());
        }

        #[test]
        fn the_native_buffer_budget_is_off_unless_given() {
            let parse = |args: &[&str]| {
                let mut argv = vec!["nextrs", "start", "."];
                argv.extend_from_slice(args);
                match Cli::try_parse_from(argv).map(|c| c.command) {
                    Ok(Command::Start { resources, .. }) => {
                        Ok(resources.native_buffer_budget().map(|b| b.total_bytes()))
                    }
                    Ok(_) => unreachable!(),
                    Err(e) => Err(e.to_string()),
                }
            };
            assert_eq!(parse(&[]).unwrap(), None);
            assert_eq!(
                parse(&["--native-buffer-budget", "32"]).unwrap(),
                Some(32 * 1024 * 1024)
            );
            assert!(parse(&["--native-buffer-wait", "1"]).is_err());
            assert!(limits(&["--native-buffer-budget", "0"]).is_err());
            assert!(
                limits(&["--native-buffer-budget", "8", "--native-buffer-wait", "NaN"]).is_err()
            );
            assert!(limits(&["--memory-admission", "--memory-admission-budget", "0"]).is_err());
        }

        #[test]
        fn the_young_semi_space_floor_needs_a_configured_heap() {
            // `limits()` starts from Limits::default(): no heap configured.
            assert!(
                limits(&["--js-young-semi-space", "8"])
                    .unwrap()
                    .young_semi_space_floor_bytes
                    .is_none()
            );
            assert!(limits(&["--js-young-semi-space", "0"]).is_err());
            assert!(limits(&["--js-young-semi-space", "128"]).is_err());
        }

        #[test]
        fn the_external_memory_limit_is_in_mib() {
            let l = limits(&["--js-external-memory-limit", "48"]).unwrap();
            assert_eq!(l.external_memory_limit_bytes, Some(48 * 1024 * 1024));
            assert!(limits(&["--js-external-memory-limit", "0"]).is_err());
        }

        #[test]
        fn memory_admission_takes_its_settings() {
            let l = limits(&[
                "--memory-admission",
                "--memory-admission-budget",
                "200",
                "--memory-admission-pressure",
                "80",
                "--memory-admission-critical",
                "90",
                "--memory-admission-reserve",
                "3",
                "--memory-admission-max-active",
                "6",
            ])
            .unwrap();
            let a = l.memory_admission.unwrap();
            assert_eq!(a.budget_bytes, Some(200 * 1024 * 1024));
            assert_eq!((a.pressure_percent, a.critical_percent), (80.0, 90.0));
            assert_eq!(a.request_reserve_bytes, 3 * 1024 * 1024);
            assert_eq!(a.pressure_max_active, Some(6));
            // RE-4.1 settings keep RE-4's behaviour unless given.
            assert_eq!(a.accounting, nextrs_runtime::AdmissionAccounting::Sampled);
            assert_eq!(a.emergency_percent, None);
        }

        #[test]
        fn accounting_and_emergency_settings() {
            let l = limits(&[
                "--memory-admission",
                "--memory-admission-budget",
                "200",
                "--memory-admission-accounting",
                "committed",
                "--memory-admission-commitment-ttl",
                "1.5",
                "--memory-admission-emergency",
                "98",
                "--memory-admission-emergency-interval",
                "0",
            ])
            .unwrap();
            let a = l.memory_admission.unwrap();
            assert_eq!(a.accounting, nextrs_runtime::AdmissionAccounting::Committed);
            assert_eq!(a.commitment_ttl, Duration::from_millis(1500));
            assert_eq!(a.emergency_percent, Some(98.0));
            assert_eq!(a.emergency_progress_interval, None);
            let b = limits(&[
                "--memory-admission",
                "--memory-admission-budget",
                "200",
                "--memory-admission-emergency",
                "97",
            ])
            .unwrap()
            .memory_admission
            .unwrap();
            assert_eq!(b.emergency_progress_interval, Some(Duration::from_secs(5)));
            // Below critical, a zero lifetime, a lone interval, NaN.
            for bad in [
                &[
                    "--memory-admission",
                    "--memory-admission-budget",
                    "200",
                    "--memory-admission-emergency",
                    "90",
                ][..],
                &[
                    "--memory-admission",
                    "--memory-admission-budget",
                    "200",
                    "--memory-admission-commitment-ttl",
                    "0",
                ][..],
                &[
                    "--memory-admission",
                    "--memory-admission-emergency-interval",
                    "3",
                ][..],
                &[
                    "--memory-admission",
                    "--memory-admission-budget",
                    "200",
                    "--memory-admission-commitment-ttl",
                    "NaN",
                ][..],
            ] {
                assert!(limits(bad).is_err(), "{bad:?}");
            }
        }

        #[test]
        fn memory_admission_settings_need_the_switch_and_valid_thresholds() {
            assert!(limits(&["--memory-admission-budget", "200"]).is_err());
            assert!(limits(&["--memory-admission", "--memory-admission-pressure", "96"]).is_err());
        }
    }

    mod memory_settings {
        use super::super::{Cli, Command, HeapLimit, MemorySettings, Profile};
        use clap::Parser;
        use std::time::Duration;

        fn settings(args: &[&str]) -> Result<MemorySettings, String> {
            let mut argv = vec!["nextrs", "start", "."];
            argv.extend_from_slice(args);
            let cli = Cli::try_parse_from(argv).map_err(|e| e.to_string())?;
            match cli.command {
                Command::Start { limits, .. } => limits.memory(),
                _ => unreachable!("parsed a start command"),
            }
        }

        #[test]
        fn without_a_profile_nothing_changes() {
            let m = settings(&[]).unwrap();
            assert_eq!(m, MemorySettings::profile(Profile::Performance));
            assert_eq!(m.heap_limit, None);
            assert_eq!(m.compression_cache_mib, 32);
            assert_eq!(m.reclaim_idle, None);
            assert_eq!(m.pressure_percent, None);
            assert_eq!(m.malloc_arena_max, 0);
        }

        #[test]
        fn profiles_set_documented_values() {
            let b = settings(&["--profile", "balanced"]).unwrap();
            assert_eq!(b.heap_limit, Some(HeapLimit::Auto));
            assert_eq!(b.reclaim_idle, Some(Duration::from_secs(30)));
            assert_eq!(b.pressure_percent, Some(90.0));
            let l = settings(&["--profile", "low-memory"]).unwrap();
            assert_eq!(l.compression_cache_mib, 8);
            assert_eq!(l.malloc_arena_max, 0);
            assert_eq!(l.reclaim_idle, Some(Duration::from_secs(10)));
        }

        #[test]
        fn explicit_flags_win_over_the_profile() {
            let m = settings(&[
                "--profile",
                "low-memory",
                "--js-heap-limit",
                "512",
                "--compression-cache",
                "64",
                "--memory-reclaim-idle",
                "0",
                "--memory-pressure-reclaim",
                "0",
                "--malloc-arena-max",
                "0",
            ])
            .unwrap();
            assert_eq!(m.heap_limit, Some(HeapLimit::Mib(512)));
            assert_eq!(m.compression_cache_mib, 64);
            assert_eq!(m.reclaim_idle, None);
            assert_eq!(m.pressure_percent, None);
            assert_eq!(m.malloc_arena_max, 0);
        }

        #[test]
        fn heap_limit_takes_mib_or_auto() {
            assert_eq!(
                settings(&["--js-heap-limit", "auto"]).unwrap().heap_limit,
                Some(HeapLimit::Auto)
            );
            assert_eq!(
                settings(&["--js-heap-limit", "AUTO"]).unwrap().heap_limit,
                Some(HeapLimit::Auto)
            );
            assert!(settings(&["--js-heap-limit", "0"]).is_err());
            assert!(settings(&["--js-heap-limit", "lots"]).is_err());
        }

        #[test]
        fn invalid_reclaim_values_are_refused() {
            assert!(settings(&["--memory-reclaim-idle", "-1"]).is_err());
            assert!(settings(&["--memory-reclaim-idle", "NaN"]).is_err());
            assert!(settings(&["--memory-reclaim-idle", "86401"]).is_err());
            assert!(settings(&["--memory-pressure-reclaim", "150"]).is_err());
            assert!(settings(&["--profile", "turbo"]).is_err());
        }
    }
}
