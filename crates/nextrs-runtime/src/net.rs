//! Outbound HTTP for JavaScript `fetch`, as an explicit capability.
//!
//! Without a [`NetworkConfig`] JavaScript has no network access at all.
//! With one, a request is sent only if:
//!
//! 1. its URL is `http`/`https`, carries no credentials, and its scheme,
//!    host and port match an [`AllowRule`] (checked before any DNS lookup);
//! 2. every address its host resolves to is public, or inside an
//!    explicitly allowed [`AddrRange`]. The resolver that performs this
//!    check is the one the HTTP client connects with, so the addresses
//!    checked are the addresses connected to (no second lookup that DNS
//!    rebinding could answer differently). IP-literal hosts are checked
//!    the same way before connecting;
//! 3. after a redirect, steps 1 and 2 pass again for the new location.
//!
//! Proxies from the environment are ignored, the client never retries, and
//! HTTP/1.1 is used (as Node's `fetch`). Requests run on a dedicated Tokio
//! runtime shared by all isolates, so neither DNS, TLS nor body decoding
//! runs on a V8 thread, and isolate restarts do not tear down the pool.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::sync::{mpsc, oneshot};
use url::{Host, Url};

use crate::metrics::{self, Counters};

/// Most redirects one fetch follows (the Fetch standard's limit).
const MAX_REDIRECTS: usize = 20;

/// Request headers JavaScript may not set: they control framing or the
/// connection, or would let a request claim another host.
pub(crate) fn forbidden_request_header(name: &str) -> bool {
    matches!(
        name,
        "connection" | "keep-alive" | "transfer-encoding" | "upgrade" | "te" | "trailer" | "host"
    ) || name.starts_with("proxy-")
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

/// A destination JavaScript may request: one scheme, one host (or every
/// subdomain of one), one port.
///
/// Parsed from `host`, `host:port`, `scheme://host[:port]` or
/// `*.domain[:port]`. Without a scheme only `https` is allowed; without a
/// port only the scheme's default port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowRule {
    https: bool,
    host: HostPattern,
    port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HostPattern {
    Domain(String),
    /// `*.example.com`: strict subdomains of `example.com`.
    Subdomains(String),
    Ip(IpAddr),
}

/// `example.com.` and `example.com` name the same host.
fn normalize_domain(domain: &str) -> &str {
    domain.strip_suffix('.').unwrap_or(domain)
}

impl FromStr for AllowRule {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let with_scheme = if text.contains("://") {
            text.to_owned()
        } else {
            format!("https://{text}")
        };
        let (wildcard, parse_from) = match with_scheme.split_once("://*.") {
            Some((scheme, rest)) => (true, format!("{scheme}://{rest}")),
            None => (false, with_scheme.clone()),
        };
        let url = Url::parse(&parse_from).map_err(|e| format!("'{text}': {e}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(format!("'{text}': only http and https can be allowed"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(format!("'{text}': credentials are not allowed"));
        }
        if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
            return Err(format!(
                "'{text}': give an origin (scheme, host, port), not a path"
            ));
        }
        let port = url
            .port_or_known_default()
            .ok_or_else(|| format!("'{text}': no port"))?;
        let host = match url.host() {
            Some(Host::Domain(domain)) => {
                let domain = normalize_domain(domain).to_owned();
                if domain.contains('*') {
                    return Err(format!(
                        "'{text}': '*' is only allowed as a leading '*.' label"
                    ));
                }
                if wildcard {
                    if domain.split('.').filter(|label| !label.is_empty()).count() < 2 {
                        return Err(format!(
                            "'{text}': a wildcard needs a domain with at least two labels"
                        ));
                    }
                    HostPattern::Subdomains(domain)
                } else {
                    HostPattern::Domain(domain)
                }
            }
            Some(Host::Ipv4(ip)) if !wildcard => HostPattern::Ip(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) if !wildcard => HostPattern::Ip(IpAddr::V6(ip)),
            _ => return Err(format!("'{text}': not a host")),
        };
        Ok(Self {
            https: url.scheme() == "https",
            host,
            port,
        })
    }
}

impl fmt::Display for AllowRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = if self.https { "https" } else { "http" };
        match &self.host {
            HostPattern::Domain(d) => write!(f, "{scheme}://{d}:{}", self.port),
            HostPattern::Subdomains(d) => write!(f, "{scheme}://*.{d}:{}", self.port),
            HostPattern::Ip(IpAddr::V6(ip)) => write!(f, "{scheme}://[{ip}]:{}", self.port),
            HostPattern::Ip(ip) => write!(f, "{scheme}://{ip}:{}", self.port),
        }
    }
}

impl AllowRule {
    fn matches(&self, https: bool, host: &Host<&str>, port: u16) -> bool {
        if self.https != https || self.port != port {
            return false;
        }
        match (&self.host, host) {
            (HostPattern::Domain(rule), Host::Domain(domain)) => rule == normalize_domain(domain),
            (HostPattern::Subdomains(rule), Host::Domain(domain)) => normalize_domain(domain)
                .strip_suffix(rule.as_str())
                .is_some_and(|prefix| prefix.len() > 1 && prefix.ends_with('.')),
            (HostPattern::Ip(rule), Host::Ipv4(ip)) => *rule == IpAddr::V4(*ip),
            (HostPattern::Ip(rule), Host::Ipv6(ip)) => *rule == IpAddr::V6(*ip),
            _ => false,
        }
    }
}

/// An address range that may be connected to although it is not public
/// (loopback, private, link-local, ...): `127.0.0.1`, `10.0.0.0/8`, `::1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddrRange {
    network: IpAddr,
    prefix: u8,
}

impl FromStr for AddrRange {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let (addr, prefix) = match text.trim().split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (text.trim(), None),
        };
        let network: IpAddr = addr
            .parse()
            .map_err(|_| format!("'{text}': not an IP address"))?;
        let network = network.to_canonical();
        let max = if network.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(|| format!("'{text}': bad prefix length"))?,
            None => max,
        };
        if prefix == 0 {
            return Err(format!(
                "'{text}': a /0 range would allow every address; list ranges explicitly"
            ));
        }
        Ok(Self { network, prefix })
    }
}

impl AddrRange {
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.network, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX << (32 - u32::from(self.prefix));
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX << (128 - u32::from(self.prefix));
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

fn in_v4(ip: Ipv4Addr, net: [u8; 4], prefix: u32) -> bool {
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    u32::from(ip) & mask == u32::from(Ipv4Addr::from(net)) & mask
}

fn in_v6(ip: Ipv6Addr, net: u128, prefix: u32) -> bool {
    let mask = u128::MAX << (128 - prefix);
    u128::from(ip) & mask == net & mask
}

/// Why an address is not public, or `None` if it is.
fn non_public(ip: IpAddr) -> Option<&'static str> {
    match ip.to_canonical() {
        IpAddr::V4(ip) => non_public_v4(ip),
        IpAddr::V6(ip) => non_public_v6(ip),
    }
}

fn non_public_v4(ip: Ipv4Addr) -> Option<&'static str> {
    const RANGES: &[([u8; 4], u32, &str)] = &[
        ([0, 0, 0, 0], 8, "unspecified"),
        ([10, 0, 0, 0], 8, "private"),
        ([100, 64, 0, 0], 10, "shared address space"),
        ([127, 0, 0, 0], 8, "loopback"),
        // Includes cloud metadata services (169.254.169.254).
        ([169, 254, 0, 0], 16, "link-local"),
        ([172, 16, 0, 0], 12, "private"),
        ([192, 0, 0, 0], 24, "IETF protocol assignment"),
        ([192, 0, 2, 0], 24, "documentation"),
        ([192, 88, 99, 0], 24, "6to4 relay"),
        ([192, 168, 0, 0], 16, "private"),
        ([198, 18, 0, 0], 15, "benchmarking"),
        ([198, 51, 100, 0], 24, "documentation"),
        ([203, 0, 113, 0], 24, "documentation"),
        ([224, 0, 0, 0], 4, "multicast"),
        ([240, 0, 0, 0], 4, "reserved"),
    ];
    RANGES
        .iter()
        .find(|(net, prefix, _)| in_v4(ip, *net, *prefix))
        .map(|(_, _, class)| *class)
}

fn non_public_v6(ip: Ipv6Addr) -> Option<&'static str> {
    let bits = u128::from(ip);
    // NAT64 (RFC 6052 well-known prefix): judge the embedded IPv4 address.
    if in_v6(ip, 0x0064_ff9b_0000_0000_0000_0000_0000_0000, 96) {
        return non_public_v4(Ipv4Addr::from(bits as u32));
    }
    // Only global unicast (2000::/3) is public; that excludes loopback,
    // unspecified, IPv4-compatible, unique-local (fc00::/7, including
    // fd00:ec2::254), link-local, site-local and multicast.
    if !in_v6(ip, 0x2000 << 112, 3) {
        return Some(if ip.is_loopback() {
            "loopback"
        } else if ip.is_unspecified() {
            "unspecified"
        } else if in_v6(ip, 0xfc00 << 112, 7) {
            "unique local"
        } else if in_v6(ip, 0xfe80 << 112, 10) {
            "link-local"
        } else if in_v6(ip, 0xff00 << 112, 8) {
            "multicast"
        } else {
            "not global unicast"
        });
    }
    const RANGES: &[(u128, u32, &str)] = &[
        // IETF protocol assignments, including Teredo (2001::/32).
        (0x2001_0000 << 96, 23, "IETF protocol assignment"),
        (0x2001_0db8 << 96, 32, "documentation"),
        // 6to4 tunnels to arbitrary IPv4 addresses.
        (0x2002 << 112, 16, "6to4"),
        (0x3fff << 112, 20, "documentation"),
    ];
    RANGES
        .iter()
        .find(|(net, prefix, _)| in_v6(ip, *net, *prefix))
        .map(|(_, _, class)| *class)
}

/// Where JavaScript may send requests. Empty: nowhere.
#[derive(Debug, Clone, Default)]
pub struct NetworkPolicy {
    pub allow: Vec<AllowRule>,
    pub allow_addrs: Vec<AddrRange>,
}

/// Why a request was refused by the policy. Messages name the origin,
/// never the path or query (which may carry tokens).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denied {
    Scheme(String),
    Credentials,
    NoHost,
    NotAllowed(String),
    Address { addr: IpAddr, class: &'static str },
}

impl fmt::Display for Denied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scheme(scheme) => write!(f, "the '{scheme}:' scheme is not supported"),
            Self::Credentials => f.write_str("URLs with credentials are not allowed"),
            Self::NoHost => f.write_str("the URL has no host"),
            Self::NotAllowed(origin) => {
                write!(f, "{origin} is not allowed by the NextRS network policy")
            }
            Self::Address { addr, class } => write!(
                f,
                "{addr} is a {class} address, not allowed by the NextRS network policy"
            ),
        }
    }
}

impl std::error::Error for Denied {}

fn origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

impl NetworkPolicy {
    /// Checks a URL before any lookup: scheme, credentials, allowlist, and
    /// the address of an IP-literal host.
    pub fn check_url(&self, url: &Url) -> Result<(), Denied> {
        let https = match url.scheme() {
            "https" => true,
            "http" => false,
            other => return Err(Denied::Scheme(other.to_owned())),
        };
        if !url.username().is_empty() || url.password().is_some() {
            return Err(Denied::Credentials);
        }
        let host = url.host().ok_or(Denied::NoHost)?;
        let port = url.port_or_known_default().ok_or(Denied::NoHost)?;
        if !self
            .allow
            .iter()
            .any(|rule| rule.matches(https, &host, port))
        {
            return Err(Denied::NotAllowed(origin(url)));
        }
        match host {
            Host::Ipv4(ip) => self.check_addr(IpAddr::V4(ip)),
            Host::Ipv6(ip) => self.check_addr(IpAddr::V6(ip)),
            Host::Domain(_) => Ok(()),
        }
    }

    /// An address may be connected to if it is public or explicitly allowed.
    pub fn check_addr(&self, addr: IpAddr) -> Result<(), Denied> {
        match non_public(addr) {
            None => Ok(()),
            Some(_) if self.allow_addrs.iter().any(|r| r.contains(addr)) => Ok(()),
            Some(class) => Err(Denied::Address { addr, class }),
        }
    }
}

/// Limits applied to every outbound request.
#[derive(Debug, Clone)]
pub struct NetworkLimits {
    /// Connecting, per address tried.
    pub connect_timeout: Duration,
    /// From sending the request to receiving the response head, including
    /// connecting, redirects, and uploading a request body.
    pub headers_timeout: Duration,
    /// Longest wait for the next chunk of a response body.
    pub body_idle_timeout: Duration,
    /// Response body bytes (after content decoding) per fetch.
    pub max_response_bytes: u64,
    /// Fetches in flight per isolate; more fail at once.
    pub max_in_flight_per_isolate: usize,
    /// How long an idle pooled connection may be reused. It must be shorter
    /// than the upstream's own keep-alive: a connection the upstream closes
    /// just as a request is sent on it fails that request (a reset, not a
    /// retry). Node closes idle connections after 5 s, so 4 s, as Node's
    /// fetch (undici) uses.
    pub pool_idle_timeout: Duration,
}

impl Default for NetworkLimits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            headers_timeout: Duration::from_secs(30),
            body_idle_timeout: Duration::from_secs(30),
            max_response_bytes: 32 * 1024 * 1024,
            max_in_flight_per_isolate: 256,
            pool_idle_timeout: Duration::from_secs(4),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NetworkConfig {
    pub policy: NetworkPolicy,
    pub limits: NetworkLimits,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a fetch failed. `code()` is what JavaScript sees as `cause.code`
/// (Node/undici codes where one applies); `reason()` is the metric label.
#[derive(Debug, Clone)]
pub(crate) enum FetchError {
    Denied(Denied),
    Invalid(String),
    Dns(String),
    Refused,
    ConnectTimeout,
    HeadersTimeout,
    BodyTimeout,
    TooLarge(u64),
    Redirect(String),
    Network(String),
}

impl FetchError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Denied(_) => "ERR_NEXTRS_NET_DENIED",
            Self::Invalid(_) => "ERR_INVALID_ARG_VALUE",
            Self::Dns(_) => "ENOTFOUND",
            Self::Refused => "ECONNREFUSED",
            Self::ConnectTimeout => "UND_ERR_CONNECT_TIMEOUT",
            Self::HeadersTimeout => "UND_ERR_HEADERS_TIMEOUT",
            Self::BodyTimeout => "UND_ERR_BODY_TIMEOUT",
            Self::TooLarge(_) => "ERR_NEXTRS_NET_RESPONSE_TOO_LARGE",
            Self::Redirect(_) => "ERR_NEXTRS_NET_REDIRECT",
            Self::Network(_) => "UND_ERR_SOCKET",
        }
    }

    pub fn reason(&self) -> usize {
        match self {
            Self::Denied(_) => metrics::FETCH_DENIED,
            Self::Invalid(_) => metrics::FETCH_INVALID,
            Self::Dns(_) => metrics::FETCH_DNS,
            Self::Refused | Self::Network(_) => metrics::FETCH_NETWORK,
            Self::ConnectTimeout | Self::HeadersTimeout | Self::BodyTimeout => {
                metrics::FETCH_TIMEOUT
            }
            Self::TooLarge(_) => metrics::FETCH_TOO_LARGE,
            Self::Redirect(_) => metrics::FETCH_REDIRECT,
        }
    }

    /// `CODE: message`, the format the JavaScript side parses.
    pub fn to_js(&self) -> String {
        format!("{}: {self}", self.code())
    }

    fn from_reqwest(error: reqwest::Error) -> Self {
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        while let Some(current) = source {
            // `io::Error::source` skips a custom payload: look at it directly.
            let io = current.downcast_ref::<std::io::Error>();
            let failure = current.downcast_ref::<ResolveFailure>().or_else(|| {
                io.and_then(|io| io.get_ref())
                    .and_then(|inner| inner.downcast_ref::<ResolveFailure>())
            });
            if let Some(failure) = failure {
                return match failure {
                    ResolveFailure::Denied(denied) => Self::Denied(denied.clone()),
                    ResolveFailure::Dns(message) => Self::Dns(message.clone()),
                };
            }
            if io.is_some_and(|io| io.kind() == std::io::ErrorKind::ConnectionRefused) {
                return Self::Refused;
            }
            source = current.source();
        }
        if error.is_timeout() {
            return if error.is_connect() {
                Self::ConnectTimeout
            } else {
                Self::HeadersTimeout
            };
        }
        if error.is_builder() {
            return Self::Invalid(error.without_url().to_string());
        }
        // Never the URL (its query may carry tokens): the error without it,
        // then its causes.
        let error = error.without_url();
        let mut message = error.to_string();
        let mut source = std::error::Error::source(&error);
        while let Some(cause) = source {
            message.push_str(": ");
            message.push_str(&cause.to_string());
            source = cause.source();
        }
        Self::Network(message)
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied(denied) => write!(f, "{denied}"),
            Self::Invalid(message) | Self::Network(message) | Self::Redirect(message) => {
                f.write_str(message)
            }
            Self::Dns(message) => write!(f, "DNS lookup failed: {message}"),
            Self::Refused => f.write_str("connection refused"),
            Self::ConnectTimeout => f.write_str("connect timeout"),
            Self::HeadersTimeout => f.write_str("timed out waiting for the response headers"),
            Self::BodyTimeout => f.write_str("timed out waiting for the response body"),
            Self::TooLarge(limit) => {
                write!(f, "the response body exceeds the {limit}-byte limit")
            }
        }
    }
}

/// Raised by the resolver; found again in reqwest's error chain.
#[derive(Debug)]
enum ResolveFailure {
    Denied(Denied),
    Dns(String),
}

impl fmt::Display for ResolveFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Denied(denied) => write!(f, "{denied}"),
            Self::Dns(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ResolveFailure {}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Resolves with the system resolver, then refuses the whole answer if any
/// address is not allowed. The HTTP client connects only to the addresses
/// returned here.
struct PolicyResolver {
    policy: Arc<NetworkPolicy>,
    lookup: Lookup,
}

/// Name resolution: the system resolver, or a scripted one in tests.
type Lookup = Arc<
    dyn Fn(
            String,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = std::io::Result<Vec<SocketAddr>>> + Send>,
        > + Send
        + Sync,
>;

fn system_lookup() -> Lookup {
    Arc::new(|host| {
        Box::pin(async move { Ok(tokio::net::lookup_host((host.as_str(), 0)).await?.collect()) })
    })
}

impl reqwest::dns::Resolve for PolicyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let policy = self.policy.clone();
        let lookup = (self.lookup)(name.as_str().to_owned());
        Box::pin(async move {
            // Boxed once, as `ResolveFailure` (not `Box<ResolveFailure>`),
            // so `FetchError::from_reqwest` can find it again.
            let fail = |failure| -> Box<dyn std::error::Error + Send + Sync> { Box::new(failure) };
            let addrs = lookup
                .await
                .map_err(|e| fail(ResolveFailure::Dns(e.to_string())))?;
            if addrs.is_empty() {
                return Err(fail(ResolveFailure::Dns("no addresses".to_owned())));
            }
            for addr in &addrs {
                policy
                    .check_addr(addr.ip())
                    .map_err(|denied| fail(ResolveFailure::Denied(denied)))?;
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RedirectMode {
    Follow,
    Manual,
    Error,
}

pub(crate) enum OutBody {
    None,
    Bytes(Bytes),
    /// Chunks written by JavaScript; consumed by the first request sent.
    Stream(mpsc::Receiver<Bytes>),
}

pub(crate) struct FetchRequest {
    pub method: reqwest::Method,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: OutBody,
    pub redirect: RedirectMode,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FetchHead {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub url: String,
    pub redirected: bool,
}

pub(crate) enum BodyItem {
    Chunk(Bytes),
    End,
    Error(FetchError),
}

/// A running fetch, seen from the isolate.
pub(crate) struct FetchTask {
    pub head: oneshot::Receiver<Result<FetchHead, FetchError>>,
    pub body: mpsc::Receiver<BodyItem>,
    pub abort: tokio::task::AbortHandle,
}

struct Shared {
    client: reqwest::Client,
    /// The same policy and resolver without response decoding or an
    /// `Accept-Encoding` of its own: external rewrites pass the upstream's
    /// bytes through as next start's proxy does.
    raw_client: reqwest::Client,
    policy: Arc<NetworkPolicy>,
    limits: NetworkLimits,
    counters: Arc<Counters>,
}

/// The process's outbound HTTP client: one runtime, one connection pool.
pub(crate) struct NetClient {
    runtime: Option<tokio::runtime::Runtime>,
    shared: Arc<Shared>,
}

impl Drop for NetClient {
    fn drop(&mut self) {
        // Safe from any context, including inside another runtime.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl NetClient {
    pub fn new(config: &NetworkConfig, counters: Arc<Counters>) -> Result<Self, String> {
        Self::with_lookup(config, counters, system_lookup())
    }

    fn with_lookup(
        config: &NetworkConfig,
        counters: Arc<Counters>,
        lookup: Lookup,
    ) -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("nextrs-net")
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let mut tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let policy = Arc::new(config.policy.clone());
        let resolver = Arc::new(PolicyResolver {
            policy: policy.clone(),
            lookup,
        });
        let builder = |tls: rustls::ClientConfig| {
            reqwest::Client::builder()
                .tls_backend_preconfigured(tls)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .http1_only()
                .referer(false)
                .connect_timeout(config.limits.connect_timeout)
                .pool_max_idle_per_host(32)
                .pool_idle_timeout(config.limits.pool_idle_timeout)
                .dns_resolver(resolver.clone())
        };
        let client = builder(tls.clone()).build().map_err(|e| e.to_string())?;
        let raw_client = builder(tls)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            runtime: Some(runtime),
            shared: Arc::new(Shared {
                client,
                raw_client,
                policy,
                limits: config.limits.clone(),
                counters,
            }),
        })
    }

    pub fn limits(&self) -> &NetworkLimits {
        &self.shared.limits
    }

    /// Checks the policy for the first URL at once (no lookup), then runs
    /// the request on the network runtime.
    pub fn start(&self, request: FetchRequest) -> Result<FetchTask, FetchError> {
        metrics::add(&self.shared.counters.fetch_requests, 1);
        if let Err(denied) = self.shared.policy.check_url(&request.url) {
            let error = FetchError::Denied(denied);
            self.shared.counters.fetch_failed(error.reason());
            return Err(error);
        }
        let (head_tx, head) = oneshot::channel();
        let (body_tx, body) = mpsc::channel(1);
        let shared = self.shared.clone();
        let runtime = self.runtime.as_ref().expect("runtime lives until drop");
        let task = runtime.spawn(async move { shared.run(request, head_tx, body_tx).await });
        Ok(FetchTask {
            head,
            body,
            abort: task.abort_handle(),
        })
    }
}

fn header_map(headers: &[(String, String)]) -> Result<HeaderMap, FetchError> {
    let mut map = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| FetchError::Invalid(format!("invalid header name '{name}'")))?;
        if forbidden_request_header(name.as_str()) || name == "content-length" {
            return Err(FetchError::Invalid(format!(
                "the '{name}' header cannot be set"
            )));
        }
        // Header values are byte strings (Latin-1); JavaScript checks this
        // too, but nothing is truncated here either.
        let bytes: Vec<u8> = value
            .chars()
            .map(|c| u8::try_from(u32::from(c)))
            .collect::<Result<_, _>>()
            .map_err(|_| {
                FetchError::Invalid(format!("the '{name}' header value is not a byte string"))
            })?;
        let value = HeaderValue::from_bytes(&bytes)
            .map_err(|_| FetchError::Invalid(format!("invalid value for header '{name}'")))?;
        map.append(name, value);
    }
    Ok(map)
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.origin() == b.origin()
}

impl Shared {
    async fn run(
        self: Arc<Self>,
        request: FetchRequest,
        head_tx: oneshot::Sender<Result<FetchHead, FetchError>>,
        body_tx: mpsc::Sender<BodyItem>,
    ) {
        let started = Instant::now();
        let response = tokio::time::timeout(self.limits.headers_timeout, self.send(request))
            .await
            .unwrap_or(Err(FetchError::HeadersTimeout));
        let (response, redirected) = match response {
            Ok(response) => response,
            Err(error) => {
                self.counters.fetch_failed(error.reason());
                let _ = head_tx.send(Err(error));
                return;
            }
        };
        self.counters.observe_fetch(started.elapsed());
        let limit = self.limits.max_response_bytes;
        if response.content_length().is_some_and(|len| len > limit) {
            let error = FetchError::TooLarge(limit);
            self.counters.fetch_failed(error.reason());
            let _ = head_tx.send(Err(error));
            return;
        }
        let status = response.status();
        let status_text = response
            .extensions()
            .get::<hyper::ext::ReasonPhrase>()
            .map(|reason| latin1(reason.as_bytes()))
            .unwrap_or_else(|| status.canonical_reason().unwrap_or("").to_owned());
        let head = FetchHead {
            status: status.as_u16(),
            status_text,
            headers: response
                .headers()
                .iter()
                .map(|(n, v)| (n.as_str().to_owned(), latin1(v.as_bytes())))
                .collect(),
            url: response.url().to_string(),
            redirected,
        };
        if head_tx.send(Ok(head)).is_err() {
            return;
        }
        let mut response = response;
        let mut total: u64 = 0;
        loop {
            let item =
                match tokio::time::timeout(self.limits.body_idle_timeout, response.chunk()).await {
                    Err(_) => BodyItem::Error(FetchError::BodyTimeout),
                    Ok(Err(error)) => BodyItem::Error(FetchError::from_reqwest(error)),
                    Ok(Ok(None)) => BodyItem::End,
                    Ok(Ok(Some(chunk))) => {
                        total += chunk.len() as u64;
                        if total > limit {
                            BodyItem::Error(FetchError::TooLarge(limit))
                        } else {
                            BodyItem::Chunk(chunk)
                        }
                    }
                };
            let last = !matches!(item, BodyItem::Chunk(_));
            if let BodyItem::Error(error) = &item {
                self.counters.fetch_failed(error.reason());
            }
            // Waits while JavaScript is not reading: backpressure reaches
            // the socket, and nothing is buffered beyond one chunk.
            if body_tx.send(item).await.is_err() || last {
                return;
            }
        }
    }

    /// Sends the request, following redirects as `request.redirect` says.
    /// Returns the final response and whether a redirect was followed.
    async fn send(&self, request: FetchRequest) -> Result<(reqwest::Response, bool), FetchError> {
        let FetchRequest {
            mut method,
            mut url,
            headers,
            mut body,
            redirect,
        } = request;
        let mut headers = header_map(&headers)?;
        let mut redirected = false;
        let mut streamed = false;
        for hop in 0..=MAX_REDIRECTS {
            self.policy.check_url(&url).map_err(FetchError::Denied)?;
            let mut builder = self
                .client
                .request(method.clone(), url.clone())
                .headers(headers.clone());
            match std::mem::replace(&mut body, OutBody::None) {
                OutBody::None => {}
                OutBody::Bytes(bytes) => {
                    builder = builder.body(bytes.clone());
                    body = OutBody::Bytes(bytes);
                }
                OutBody::Stream(chunks) => {
                    streamed = true;
                    let stream = futures_util::stream::unfold(chunks, |mut chunks| async move {
                        chunks
                            .recv()
                            .await
                            .map(|chunk| (Ok::<_, std::io::Error>(chunk), chunks))
                    });
                    builder = builder.body(reqwest::Body::wrap_stream(stream));
                }
            }
            let response = builder.send().await.map_err(FetchError::from_reqwest)?;
            let status = response.status().as_u16();
            if !matches!(status, 301 | 302 | 303 | 307 | 308) {
                return Ok((response, redirected));
            }
            let location = match redirect {
                RedirectMode::Manual => return Ok((response, redirected)),
                RedirectMode::Error => {
                    return Err(FetchError::Redirect(
                        "unexpected redirect (redirect mode is 'error')".to_owned(),
                    ));
                }
                RedirectMode::Follow => {
                    match response.headers().get(reqwest::header::LOCATION) {
                        // A redirect without a location is the response.
                        None => return Ok((response, redirected)),
                        Some(location) => latin1(location.as_bytes()),
                    }
                }
            };
            let next = url
                .join(&location)
                .map_err(|_| FetchError::Redirect("invalid redirect location".to_owned()))?;
            if hop == MAX_REDIRECTS {
                return Err(FetchError::Redirect("too many redirects".to_owned()));
            }
            drop(response);
            let to_get = (status == 303 && method != reqwest::Method::HEAD)
                || (matches!(status, 301 | 302) && method == reqwest::Method::POST);
            if to_get {
                method = reqwest::Method::GET;
                body = OutBody::None;
                streamed = false;
                for name in [
                    "content-encoding",
                    "content-language",
                    "content-location",
                    "content-type",
                ] {
                    headers.remove(name);
                }
            } else if streamed {
                return Err(FetchError::Redirect(
                    "cannot follow a redirect with a streamed request body".to_owned(),
                ));
            }
            if !same_origin(&url, &next) {
                for name in ["authorization", "proxy-authorization", "cookie"] {
                    headers.remove(name);
                }
            }
            url = next;
            redirected = true;
        }
        unreachable!("the loop returns by MAX_REDIRECTS")
    }
}

// ---------------------------------------------------------------------------
// External rewrites
// ---------------------------------------------------------------------------

/// A request NextRS forwards to another origin for an external rewrite
/// (`next.config` rewrites or `NextResponse.rewrite()` to an absolute URL).
pub struct UpstreamRequest {
    pub method: reqwest::Method,
    pub url: Url,
    /// Sent as given; the caller has removed hop-by-hop and internal
    /// headers. `Host` comes from the URL.
    pub headers: HeaderMap,
    pub body: UpstreamBody,
    /// Decode `Content-Encoding` (gzip, br, deflate) as Next's image
    /// optimizer does. External rewrites pass the upstream's bytes through
    /// unchanged (`false`).
    pub decompress: bool,
}

pub enum UpstreamBody {
    Empty,
    Bytes(Bytes),
    /// The client's body, chunk by chunk (sent with the client's
    /// `Content-Length` when the headers carry one, else chunked). An error
    /// item aborts the upstream request instead of ending its body.
    Stream(mpsc::Receiver<Result<Bytes, std::io::Error>>),
}

/// The upstream's response head and a body that ends the upstream request
/// when dropped (a client that disconnects aborts it).
pub struct UpstreamResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: UpstreamBodyStream,
}

/// Why an external rewrite could not be forwarded.
#[derive(Debug, Clone)]
pub struct UpstreamError(FetchError);

impl UpstreamError {
    /// The network policy refused the destination (scheme, host, port, or
    /// the address it resolved to).
    pub fn is_denied(&self) -> bool {
        matches!(self.0, FetchError::Denied(_))
    }

    /// Node/undici-style code, as for `fetch`.
    pub fn code(&self) -> &'static str {
        self.0.code()
    }
}

impl fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for UpstreamError {}

/// Response body chunks of a forwarded request.
pub struct UpstreamBodyStream {
    rx: mpsc::Receiver<Result<Bytes, UpstreamError>>,
    task: tokio::task::AbortHandle,
}

impl Drop for UpstreamBodyStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl futures_util::Stream for UpstreamBodyStream {
    type Item = Result<Bytes, UpstreamError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Aborts the upstream task unless disarmed (the head was delivered and the
/// body stream owns the task from then on).
struct AbortOnDrop(Option<tokio::task::AbortHandle>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
    }
}

/// Forwards external rewrites under the process's network policy: the same
/// allowlist, address classes and resolver as `fetch` (the resolver checks
/// every address a connection uses), on the same network runtime. Redirects
/// are never followed: the upstream's 3xx goes back to the client, as next
/// start's proxy returns it.
#[derive(Clone)]
pub struct Upstream {
    net: Arc<NetClient>,
}

impl Upstream {
    pub(crate) fn new(net: Arc<NetClient>) -> Self {
        Self { net }
    }

    /// A client of its own under `config` (same allowlist, address checks
    /// and resolver as `fetch`), for requests NextRS makes without
    /// JavaScript: remote images.
    pub fn standalone(config: &NetworkConfig) -> Result<Self, String> {
        Ok(Self::new(Arc::new(NetClient::new(
            config,
            Arc::new(Counters::default()),
        )?)))
    }

    /// The policy's verdict on a destination before any connection
    /// (scheme, credentials, allowlist, IP literals).
    pub fn check(&self, url: &Url) -> Result<(), Denied> {
        self.net.shared.policy.check_url(url)
    }

    /// Sends the request and waits for the response head (bounded by
    /// `--net-timeout`); body chunks then wait at most `--net-body-timeout`
    /// each. Dropping the future or the body aborts the upstream request.
    pub async fn send(&self, request: UpstreamRequest) -> Result<UpstreamResponse, UpstreamError> {
        let shared = self.net.shared.clone();
        metrics::add(&shared.counters.upstream_requests, 1);
        if let Err(denied) = shared.policy.check_url(&request.url) {
            let error = FetchError::Denied(denied);
            shared.counters.upstream_failed(error.reason());
            return Err(UpstreamError(error));
        }
        let (head_tx, head_rx) = oneshot::channel();
        let (body_tx, body_rx) = mpsc::channel(1);
        let runtime = self.net.runtime.as_ref().expect("runtime lives until drop");
        let task = runtime.spawn(async move { shared.forward(request, head_tx, body_tx).await });
        let mut guard = AbortOnDrop(Some(task.abort_handle()));
        match head_rx.await {
            Ok(Ok((status, headers))) => {
                let task = guard.0.take().expect("armed");
                Ok(UpstreamResponse {
                    status,
                    headers,
                    body: UpstreamBodyStream { rx: body_rx, task },
                })
            }
            Ok(Err(error)) => Err(UpstreamError(error)),
            Err(_) => Err(UpstreamError(FetchError::Network(
                "the network runtime stopped".to_owned(),
            ))),
        }
    }
}

impl Shared {
    async fn forward(
        self: Arc<Self>,
        request: UpstreamRequest,
        head_tx: oneshot::Sender<Result<(u16, HeaderMap), FetchError>>,
        body_tx: mpsc::Sender<Result<Bytes, UpstreamError>>,
    ) {
        struct InFlight<'a>(&'a Counters);
        impl Drop for InFlight<'_> {
            fn drop(&mut self) {
                self.0
                    .upstream_in_flight
                    .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        metrics::add(&self.counters.upstream_in_flight, 1);
        let _in_flight = InFlight(&self.counters);
        let UpstreamRequest {
            method,
            url,
            headers,
            body,
            decompress,
        } = request;
        let client = if decompress {
            &self.client
        } else {
            &self.raw_client
        };
        let mut builder = client.request(method, url).headers(headers);
        match body {
            UpstreamBody::Empty => {}
            UpstreamBody::Bytes(bytes) => builder = builder.body(bytes),
            UpstreamBody::Stream(chunks) => {
                let stream = futures_util::stream::unfold(chunks, |mut chunks| async move {
                    chunks.recv().await.map(|item| (item, chunks))
                });
                builder = builder.body(reqwest::Body::wrap_stream(stream));
            }
        }
        let sent = match tokio::time::timeout(self.limits.headers_timeout, builder.send()).await {
            Err(_) => Err(FetchError::HeadersTimeout),
            Ok(result) => result.map_err(FetchError::from_reqwest),
        };
        let mut response = match sent {
            Ok(response) => response,
            Err(error) => {
                self.counters.upstream_failed(error.reason());
                let _ = head_tx.send(Err(error));
                return;
            }
        };
        let head = (response.status().as_u16(), response.headers().clone());
        if head_tx.send(Ok(head)).is_err() {
            return;
        }
        loop {
            let item =
                match tokio::time::timeout(self.limits.body_idle_timeout, response.chunk()).await {
                    Err(_) => Err(FetchError::BodyTimeout),
                    Ok(Err(error)) => Err(FetchError::from_reqwest(error)),
                    Ok(Ok(None)) => return,
                    Ok(Ok(Some(chunk))) => Ok(chunk),
                };
            let failed = item.is_err();
            if let Err(error) = &item {
                self.counters.upstream_failed(error.reason());
            }
            // Waits while the client is not reading: backpressure reaches
            // the upstream socket, and nothing is buffered beyond one chunk.
            if body_tx.send(item.map_err(UpstreamError)).await.is_err() || failed {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(allow: &[&str], addrs: &[&str]) -> NetworkPolicy {
        NetworkPolicy {
            allow: allow.iter().map(|r| r.parse().unwrap()).collect(),
            allow_addrs: addrs.iter().map(|r| r.parse().unwrap()).collect(),
        }
    }

    fn check(policy: &NetworkPolicy, url: &str) -> Result<(), Denied> {
        policy.check_url(&Url::parse(url).unwrap())
    }

    #[test]
    fn rules_parse_with_secure_defaults() {
        let rule: AllowRule = "api.example.com".parse().unwrap();
        assert_eq!(rule.to_string(), "https://api.example.com:443");
        let rule: AllowRule = "http://API.Example.com:8080".parse().unwrap();
        assert_eq!(rule.to_string(), "http://api.example.com:8080");
        let rule: AllowRule = "*.example.com".parse().unwrap();
        assert_eq!(rule.to_string(), "https://*.example.com:443");
        let rule: AllowRule = "[::1]:4000".parse().unwrap();
        assert_eq!(rule.to_string(), "https://[::1]:4000");
        for bad in [
            "ftp://example.com",
            "https://user:pw@example.com",
            "https://example.com/path",
            "https://example.com?q",
            "*.com",
            "*",
            "",
        ] {
            assert!(bad.parse::<AllowRule>().is_err(), "{bad}");
        }
    }

    #[test]
    fn default_policy_denies_everything() {
        let policy = NetworkPolicy::default();
        assert!(matches!(
            check(&policy, "https://example.com/"),
            Err(Denied::NotAllowed(_))
        ));
    }

    #[test]
    fn allowlist_matches_scheme_host_and_port_exactly() {
        let p = policy(&["api.example.com", "*.cdn.example.net"], &[]);
        assert!(check(&p, "https://api.example.com/v1?x=1").is_ok());
        assert!(check(&p, "https://API.EXAMPLE.COM./v1").is_ok());
        assert!(check(&p, "https://a.cdn.example.net/").is_ok());
        assert!(check(&p, "https://a.b.cdn.example.net/").is_ok());
        for denied in [
            "http://api.example.com/",       // scheme
            "https://api.example.com:8443/", // port
            "https://example.com/",          // parent
            "https://evilapi.example.com/",  // suffix without a dot boundary
            "https://api.example.com.evil.net/",
            "https://cdn.example.net/", // wildcard is strict subdomains
            "https://xcdn.example.net/",
        ] {
            assert!(
                matches!(check(&p, denied), Err(Denied::NotAllowed(_))),
                "{denied}"
            );
        }
    }

    #[test]
    fn urls_with_other_schemes_or_credentials_are_denied() {
        let p = policy(&["api.example.com"], &[]);
        assert!(matches!(
            check(&p, "file:///etc/passwd"),
            Err(Denied::Scheme(_))
        ));
        assert!(matches!(
            check(&p, "data:text/plain,x"),
            Err(Denied::Scheme(_))
        ));
        assert_eq!(
            check(&p, "https://user:pass@api.example.com/"),
            Err(Denied::Credentials)
        );
    }

    #[test]
    fn ip_literals_are_normalized_before_the_address_check() {
        // Allowing a loopback host by name is not enough: its address must
        // be allowed too.
        let p = policy(&["http://127.0.0.1", "http://[::1]"], &[]);
        for url in [
            "http://127.0.0.1/",
            "http://2130706433/", // decimal
            "http://0x7f.0.0.1/", // hex
            "http://0177.0.0.1/", // octal
            "http://127.1/",      // short form
            "http://[::1]/",
        ] {
            assert!(
                matches!(check(&p, url), Err(Denied::Address { .. })),
                "{url}"
            );
        }
        let p = policy(&["http://127.0.0.1:4000"], &["127.0.0.1"]);
        assert!(check(&p, "http://127.0.0.1:4000/").is_ok());
        assert!(check(&p, "http://2130706433:4000/").is_ok());
    }

    #[test]
    fn non_public_addresses_are_classified() {
        let cases = [
            ("127.0.0.1", Some("loopback")),
            ("10.1.2.3", Some("private")),
            ("172.16.0.1", Some("private")),
            ("172.31.255.255", Some("private")),
            ("192.168.1.1", Some("private")),
            ("169.254.169.254", Some("link-local")),
            ("100.100.100.200", Some("shared address space")),
            ("0.0.0.0", Some("unspecified")),
            ("192.0.0.192", Some("IETF protocol assignment")),
            ("224.0.0.1", Some("multicast")),
            ("255.255.255.255", Some("reserved")),
            ("198.18.0.1", Some("benchmarking")),
            ("::1", Some("loopback")),
            ("::", Some("unspecified")),
            ("::ffff:127.0.0.1", Some("loopback")),
            ("::ffff:169.254.169.254", Some("link-local")),
            ("fd00:ec2::254", Some("unique local")),
            ("fe80::1", Some("link-local")),
            ("ff02::1", Some("multicast")),
            ("64:ff9b::a00:1", Some("private")),
            ("2001::1", Some("IETF protocol assignment")),
            ("2001:db8::1", Some("documentation")),
            ("2002:7f00:1::", Some("6to4")),
            ("::127.0.0.1", Some("not global unicast")),
            ("8.8.8.8", None),
            ("172.32.0.1", None),
            ("1.1.1.1", None),
            ("::ffff:8.8.8.8", None),
            ("64:ff9b::808:808", None),
            ("2606:4700:4700::1111", None),
        ];
        for (addr, expected) in cases {
            assert_eq!(non_public(addr.parse().unwrap()), expected, "{addr}");
        }
    }

    #[test]
    fn allowed_ranges_admit_only_their_addresses() {
        let p = policy(&[], &["10.0.0.0/8", "::1"]);
        assert!(p.check_addr("10.9.8.7".parse().unwrap()).is_ok());
        assert!(p.check_addr("::ffff:10.9.8.7".parse().unwrap()).is_ok());
        assert!(p.check_addr("::1".parse().unwrap()).is_ok());
        assert!(p.check_addr("11.0.0.1".parse().unwrap()).is_ok()); // public
        assert!(p.check_addr("127.0.0.1".parse().unwrap()).is_err());
        assert!(p.check_addr("169.254.169.254".parse().unwrap()).is_err());
        assert!("0.0.0.0/0".parse::<AddrRange>().is_err());
        assert!("10.0.0.0/33".parse::<AddrRange>().is_err());
        assert!("example.com".parse::<AddrRange>().is_err());
    }

    /// A minimal HTTP/1.1 server answering every request with
    /// `Connection: close`, so each fetch opens (and resolves) anew.
    async fn closing_server() -> (u16, Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = accepted.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    let _ = socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                        )
                        .await;
                });
            }
        });
        (port, accepted)
    }

    /// Answers lookups from a script, one answer per call (the last one
    /// repeats).
    fn scripted(answers: Vec<Vec<&'static str>>) -> Lookup {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        Arc::new(move |_host| {
            let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let answer = answers[n.min(answers.len() - 1)]
                .iter()
                .map(|ip| SocketAddr::new(ip.parse().unwrap(), 0))
                .collect();
            Box::pin(async move { Ok(answer) })
        })
    }

    async fn fetch_once(client: &NetClient, url: &str) -> Result<u16, FetchError> {
        let task = client.start(FetchRequest {
            method: reqwest::Method::GET,
            url: Url::parse(url).unwrap(),
            headers: Vec::new(),
            body: OutBody::None,
            redirect: RedirectMode::Follow,
        })?;
        task.head.await.unwrap().map(|head| head.status)
    }

    fn client(port: u16, lookup: Lookup) -> NetClient {
        let config = NetworkConfig {
            policy: NetworkPolicy {
                allow: vec![format!("http://rebind.test:{port}").parse().unwrap()],
                // Stands in for "public": the only address the answer may hold.
                allow_addrs: vec!["127.0.0.1".parse().unwrap()],
            },
            limits: NetworkLimits::default(),
        };
        NetClient::with_lookup(&config, Arc::new(Counters::default()), lookup).unwrap()
    }

    async fn forward_once(upstream: &Upstream, url: &str) -> Result<u16, UpstreamError> {
        let response = upstream
            .send(UpstreamRequest {
                method: reqwest::Method::GET,
                url: url.parse().unwrap(),
                headers: HeaderMap::new(),
                body: UpstreamBody::Empty,
                decompress: false,
            })
            .await?;
        let status = response.status;
        use futures_util::StreamExt;
        let mut body = response.body;
        while body.next().await.is_some() {}
        Ok(status)
    }

    /// External rewrites go through the same resolver as fetch: a second
    /// answer that points at the metadata address is refused, and never
    /// becomes a connection.
    #[tokio::test(flavor = "multi_thread")]
    async fn external_rewrites_use_the_policy_resolver() {
        let (port, accepted) = closing_server().await;
        let upstream = Upstream::new(Arc::new(client(
            port,
            scripted(vec![vec!["127.0.0.1"], vec!["169.254.169.254"]]),
        )));
        let url = format!("http://rebind.test:{port}/x");
        assert_eq!(forward_once(&upstream, &url).await.unwrap(), 200);
        let error = forward_once(&upstream, &url).await.unwrap_err();
        assert!(error.is_denied(), "{error:?}");
        assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    /// The URL check runs before any lookup: other origins, credentials,
    /// other schemes and disguised literals are refused without a
    /// connection.
    #[tokio::test(flavor = "multi_thread")]
    async fn external_rewrites_refuse_unlisted_destinations_before_connecting() {
        let (port, accepted) = closing_server().await;
        let upstream = Upstream::new(Arc::new(client(port, scripted(vec![vec!["127.0.0.1"]]))));
        for url in [
            format!("http://other.test:{port}/"),
            format!("http://rebind.test:{}/", port + 1),
            format!("https://rebind.test:{port}/"),
            format!("http://user:pw@rebind.test:{port}/"),
            "http://169.254.169.254/latest/meta-data/".to_owned(),
            "http://2130706433/".to_owned(),
            "http://[::ffff:127.0.0.1]/".to_owned(),
            "file:///etc/passwd".to_owned(),
        ] {
            let error = forward_once(&upstream, &url).await.unwrap_err();
            assert!(error.is_denied(), "{url}: {error:?}");
        }
        assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    /// A response the client stops reading is not read ahead: dropping the
    /// body aborts the upstream request.
    #[tokio::test(flavor = "multi_thread")]
    async fn dropping_a_forwarded_body_aborts_the_upstream() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (closed_tx, closed_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n")
                .await;
            let chunk = [b'x'; 16 * 1024];
            let mut written = 0usize;
            loop {
                let head = format!("{:x}\r\n", chunk.len());
                let wrote = socket.write_all(head.as_bytes()).await.is_ok()
                    && socket.write_all(&chunk).await.is_ok()
                    && socket.write_all(b"\r\n").await.is_ok();
                if !wrote {
                    break;
                }
                written += chunk.len();
                if written > 512 * 1024 * 1024 {
                    break;
                }
            }
            let _ = closed_tx.send(written);
        });
        let upstream = Upstream::new(Arc::new(client(port, scripted(vec![vec!["127.0.0.1"]]))));
        let response = upstream
            .send(UpstreamRequest {
                method: reqwest::Method::GET,
                url: format!("http://rebind.test:{port}/").parse().unwrap(),
                headers: HeaderMap::new(),
                body: UpstreamBody::Empty,
                decompress: false,
            })
            .await
            .unwrap();
        use futures_util::StreamExt;
        let mut body = response.body;
        let _ = body.next().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(body);
        let written = tokio::time::timeout(Duration::from_secs(10), closed_rx)
            .await
            .expect("the upstream connection was closed")
            .unwrap();
        // Socket buffers only: nowhere near the 512 MiB the server offers.
        assert!(written < 64 * 1024 * 1024, "{written} bytes written");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_rebinding_answer_is_refused_on_the_next_connection() {
        let (port, accepted) = closing_server().await;
        // First lookup: the allowed address. Second: an internal one.
        let client = client(
            port,
            scripted(vec![vec!["127.0.0.1"], vec!["169.254.169.254"]]),
        );
        let url = format!("http://rebind.test:{port}/");
        assert_eq!(fetch_once(&client, &url).await.unwrap(), 200);
        let error = fetch_once(&client, &url).await.unwrap_err();
        assert!(
            matches!(
                error,
                FetchError::Denied(Denied::Address {
                    class: "link-local",
                    ..
                })
            ),
            "{error:?}"
        );
        // The refused lookup never became a connection.
        assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn an_answer_with_any_forbidden_address_is_refused_whole() {
        let (port, accepted) = closing_server().await;
        let client = client(port, scripted(vec![vec!["127.0.0.1", "10.0.0.5"]]));
        let error = fetch_once(&client, &format!("http://rebind.test:{port}/"))
            .await
            .unwrap_err();
        assert!(matches!(error, FetchError::Denied(_)), "{error:?}");
        assert_eq!(accepted.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
