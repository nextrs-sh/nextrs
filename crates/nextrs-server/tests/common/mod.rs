//! A real `nextrs` server on an ephemeral loopback port, serving the built
//! fixture through `nextrs_server::serve`, and a raw HTTP/1.1 client that
//! decides exactly when it reads. Needs the fixture build:
//!
//!   (cd fixtures/next-app && npm ci && npx next build)
#![allow(dead_code)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nextrs_runtime::{Executor, ExecutorConfig, Limits, MetricsSnapshot};
use nextrs_server::{ServeOptions, ServerOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub const IGNORE: &str = "requires fixtures/next-app to be built (npm ci && next build)";

/// Bytes per chunk produced by the fixture's `/api/stream` route.
pub const STREAM_CHUNK: usize = 16 * 1024;

pub fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/next-app")
}

pub struct Server {
    pub addr: SocketAddr,
    pub executor: Arc<Executor>,
    shutdown: Option<oneshot::Sender<()>>,
    serving: Option<JoinHandle<std::io::Result<()>>>,
}

pub struct Setup {
    pub limits: Limits,
    pub serve: ServeOptions,
    pub max_request_body: u64,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            serve: ServeOptions::default(),
            max_request_body: nextrs_server::DEFAULT_MAX_REQUEST_BODY,
        }
    }
}

impl Server {
    pub async fn start(setup: Setup) -> Self {
        let info = nextrs_adapter::inspect(&fixture()).expect("fixture is built");
        let mut config = ExecutorConfig::new(info.project_dir.clone(), info.dist_dir.clone());
        config.limits = setup.limits;
        let executor = tokio::task::spawn_blocking(move || Executor::start(config))
            .await
            .unwrap()
            .expect("executor starts");
        let app = nextrs_server::app(
            &info,
            &ServerOptions {
                executor: Some(executor.clone()),
                max_request_body: setup.max_request_body,
                ..ServerOptions::default()
            },
        )
        .await
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown, signal) = oneshot::channel::<()>();
        let serving = tokio::spawn(nextrs_server::serve(listener, app, setup.serve, async {
            let _ = signal.await;
        }));
        Self {
            addr,
            executor,
            shutdown: Some(shutdown),
            serving: Some(serving),
        }
    }

    pub fn metrics(&self) -> MetricsSnapshot {
        self.executor.metrics()
    }

    /// Starts graceful shutdown of the HTTP layer; `stopped` waits for it.
    pub fn begin_shutdown(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }

    pub async fn stopped(&mut self) {
        if let Some(serving) = self.serving.take() {
            serving.await.unwrap().unwrap();
        }
        let executor = self.executor.clone();
        tokio::task::spawn_blocking(move || executor.shutdown())
            .await
            .unwrap();
    }

    pub async fn connect(&self) -> Client {
        Client::connect(self.addr).await
    }

    /// Polls the executor metrics until `check` holds.
    pub async fn wait_for(&self, what: &str, check: impl Fn(&MetricsSnapshot) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let snapshot = self.metrics();
            if check(&snapshot) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: {snapshot:#?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.begin_shutdown();
    }
}

#[derive(Debug)]
pub struct Head {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

impl Head {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn is_chunked(&self) -> bool {
        self.header("transfer-encoding")
            .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
    }
}

pub struct Client {
    pub stream: TcpStream,
    buf: Vec<u8>,
}

impl Client {
    pub async fn connect(addr: SocketAddr) -> Self {
        Self {
            stream: TcpStream::connect(addr).await.expect("connects"),
            buf: Vec::new(),
        }
    }

    pub async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.stream.write_all(bytes).await
    }

    pub async fn get(&mut self, path: &str) {
        let request = format!("GET {path} HTTP/1.1\r\nhost: localhost\r\n\r\n");
        self.send(request.as_bytes()).await.unwrap();
    }

    /// Reads more bytes into the buffer; false at EOF.
    async fn fill(&mut self) -> std::io::Result<bool> {
        let mut chunk = [0u8; 16 * 1024];
        let n = self.stream.read(&mut chunk).await?;
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(n > 0)
    }

    pub async fn head(&mut self) -> std::io::Result<Head> {
        loop {
            if let Some(end) = find(&self.buf, b"\r\n\r\n") {
                let text = String::from_utf8_lossy(&self.buf[..end]).into_owned();
                self.buf.drain(..end + 4);
                let mut lines = text.split("\r\n");
                let status = lines
                    .next()
                    .and_then(|l| l.split(' ').nth(1))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let headers = lines
                    .filter_map(|l| l.split_once(':'))
                    .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
                    .collect();
                return Ok(Head { status, headers });
            }
            if !self.fill().await? {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed before the response head",
                ));
            }
        }
    }

    /// The next piece of a chunked body: `Ok(None)` at the terminating
    /// chunk, `Err` if the connection ended or was reset first (an aborted
    /// response).
    pub async fn chunk(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let size = loop {
            if let Some(end) = find(&self.buf, b"\r\n") {
                let line = String::from_utf8_lossy(&self.buf[..end]).into_owned();
                let size = usize::from_str_radix(line.split(';').next().unwrap().trim(), 16)
                    .map_err(|_| std::io::Error::other(format!("bad chunk size {line:?}")))?;
                self.buf.drain(..end + 2);
                break size;
            }
            if !self.fill().await? {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
        };
        while self.buf.len() < size + 2 {
            if !self.fill().await? {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
        }
        let data: Vec<u8> = self.buf.drain(..size).collect();
        self.buf.drain(..2);
        Ok((size > 0).then_some(data))
    }

    /// A whole response with a chunked or `content-length` body.
    pub async fn response(&mut self) -> std::io::Result<(Head, Vec<u8>)> {
        let head = self.head().await?;
        let mut body = Vec::new();
        if head.is_chunked() {
            while let Some(chunk) = self.chunk().await? {
                body.extend_from_slice(&chunk);
            }
        } else if let Some(length) = head.header("content-length") {
            let length: usize = length.parse().unwrap();
            while self.buf.len() < length {
                if !self.fill().await? {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
            }
            body = self.buf.drain(..length).collect();
        }
        Ok((head, body))
    }

    /// Reads until the server closes the connection; true if it did within
    /// `within`.
    pub async fn closed_within(&mut self, within: Duration) -> bool {
        tokio::time::timeout(within, async {
            loop {
                match self.fill().await {
                    Ok(true) => {}
                    Ok(false) | Err(_) => return,
                }
            }
        })
        .await
        .is_ok()
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Checks bytes of `/api/stream` output, starting at `offset`: every 16 KiB
/// chunk starts with its big-endian index and is filled with `index % 251`.
/// Returns the new offset. Order or content errors panic.
pub fn verify_stream(offset: usize, data: &[u8]) -> usize {
    for (i, byte) in data.iter().enumerate() {
        let at = offset + i;
        let index = at / STREAM_CHUNK;
        let position = at % STREAM_CHUNK;
        let expected = if position < 4 {
            (index as u32).to_be_bytes()[position]
        } else {
            (index % 251) as u8
        };
        assert_eq!(
            *byte, expected,
            "stream byte {at} (chunk {index}) out of order or corrupted"
        );
    }
    offset + data.len()
}

/// Resident set size of this process (Linux), in bytes.
pub fn rss_bytes() -> usize {
    let statm = std::fs::read_to_string("/proc/self/statm").expect("Linux /proc");
    let pages: usize = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    pages * 4096
}

pub fn chunked_body(data: &[u8], piece: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for part in data.chunks(piece) {
        out.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
        out.extend_from_slice(part);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"0\r\n\r\n");
    out
}

pub fn payload(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
