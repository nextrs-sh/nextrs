//! A byte budget for native buffers the server holds (RE-4C, RE-4.1A),
//! optional (`--native-buffer-budget`): static and `public/` file bytes
//! read for their response, and request bodies buffered for the proxy
//! (middleware). Each such buffer reserves its size before it is allocated.
//! The reservation is owned by the buffer's `Bytes` (`Bytes::from_owner`),
//! so it is given back when the last reference to the memory is dropped:
//! hyper's write queue, the gzip encoder and the compression cache's fill
//! keep it alive exactly as long as they keep the bytes. (JavaScript's
//! reads of a proxy body copy it into V8; the copies are not counted.) That covers completion, errors, cancellation,
//! timeouts and client disconnects alike, since all of them drop the body.
//!
//! Static files up to a whole-read limit (a quarter of the budget, at most
//! the compression cache's 4 MiB entry limit) are read whole, as without a
//! budget. Larger files and ranges are streamed in chunks (a quarter of the
//! budget, at most 64 KiB), each chunk reserved before it is read, so no
//! file is held whole and every reservation fits in the budget. A proxy
//! body is reserved as it arrives, frame by frame (a slow client holds
//! only what it has sent); one that can never fit (declared larger than
//! the budget, or growing past it) is refused with 413. The aggregate
//! of the bytes NextRS holds in these buffers therefore stays within the
//! budget (RE-4.1-NATIVE-BUFFER-HARDENING).
//!
//! When the budget is full, a request waits (first come, first served, as
//! the image budget does) up to `wait`, then gets a 503 with Retry-After.
//! A streamed file's later chunks may wait longer (`STREAM_WAIT`): its
//! response has started, and the chunks ahead of it are released as their
//! sockets drain (or their connections are closed by the write timeout).
//! A chunk still without room after that fails the body (hyper closes the
//! connection), so that a consumer holding every chunk it was given can
//! never stall the budget for good. Other native buffers keep their own
//! limits (RE-4-NATIVE-BUFFER-BUDGETS).

use std::future::Future;
use std::io::Read;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use http_body::{Body as HttpBody, Frame, SizeHint};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Reservations are counted in KiB (a semaphore's permits are u32).
const UNIT: u64 = 1024;

/// The largest chunk a streamed file is read in (Node's `send` streams
/// with the same 64 KiB high-water mark).
const STREAM_CHUNK: u64 = 64 * 1024;

/// The longest a streamed file's later chunk waits for room: the server's
/// default write timeout, after which a connection that makes no progress
/// is closed anyway.
const STREAM_WAIT: Duration = Duration::from_secs(30);

/// Files up to this size may be read whole: the compression cache's entry
/// limit (`compress_cache::DEFAULT_MAX_ENTRY`), so that every file the
/// cache can hold keeps its cached gzip copy when the budget allows it.
const WHOLE_READ_MAX: u64 = 4 * 1024 * 1024;

/// Which kind of buffer asked, for metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BufferKind {
    /// A static or `public/` file (or range) read whole.
    StaticFile,
    /// One chunk of a streamed static file (or range).
    StaticStream,
    ProxyBody,
}

impl BufferKind {
    const ALL: [Self; 3] = [Self::StaticFile, Self::StaticStream, Self::ProxyBody];

    fn index(self) -> usize {
        match self {
            Self::StaticFile => 0,
            Self::StaticStream => 1,
            Self::ProxyBody => 2,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::StaticFile => "static_file",
            Self::StaticStream => "static_stream",
            Self::ProxyBody => "proxy_body",
        }
    }
}

pub struct NativeBufferBudget {
    permits: Arc<Semaphore>,
    units: u64,
    wait: Duration,
    peak_units: AtomicU64,
    reserved_total: [AtomicU64; 3],
    waits: [AtomicU64; 3],
    refused: [AtomicU64; 3],
}

impl std::fmt::Debug for NativeBufferBudget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeBufferBudget")
            .field("bytes", &self.total_bytes())
            .field("reserved", &self.reserved_bytes())
            .field("wait", &self.wait)
            .finish()
    }
}

/// Bytes reserved from the budget, given back on drop.
pub(crate) struct BufferPermit {
    permit: OwnedSemaphorePermit,
    bytes: u64,
}

impl BufferPermit {
    /// Whether this permit already covers `bytes`.
    pub fn covers(&self, bytes: u64) -> bool {
        self.bytes >= bytes
    }

    /// `data` as `Bytes` that own this reservation: it is given back when
    /// the last clone of the bytes is dropped, wherever that happens.
    pub fn own<T: AsRef<[u8]> + Send + 'static>(self, data: T) -> Bytes {
        Bytes::from_owner(Reserved {
            data,
            _permit: self,
        })
    }
}

/// Memory and the reservation that pays for it.
struct Reserved<T> {
    data: T,
    _permit: BufferPermit,
}

impl<T: AsRef<[u8]>> AsRef<[u8]> for Reserved<T> {
    fn as_ref(&self) -> &[u8] {
        self.data.as_ref()
    }
}

impl NativeBufferBudget {
    /// A budget of `bytes`; requests wait up to `wait` for room.
    pub fn new(bytes: u64, wait: Duration) -> Arc<Self> {
        let units = (bytes / UNIT).clamp(1, u64::from(u32::MAX));
        Arc::new(Self {
            permits: Arc::new(Semaphore::new(units as usize)),
            units,
            wait,
            peak_units: AtomicU64::new(0),
            reserved_total: Default::default(),
            waits: Default::default(),
            refused: Default::default(),
        })
    }

    /// Units for `bytes`, at most the whole budget. Callers never ask for
    /// more than [`Self::total_bytes`] (files above the whole-read limit
    /// are streamed, larger proxy bodies refused); the clamp only keeps a
    /// mistake from waiting forever.
    fn units_for(&self, bytes: u64) -> u32 {
        debug_assert!(bytes <= self.total_bytes(), "{bytes} above the budget");
        bytes.div_ceil(UNIT).clamp(1, self.units) as u32
    }

    /// The chunk a streamed file is read in: a quarter of the budget, at
    /// least 1 KiB, at most 64 KiB.
    pub(crate) fn stream_chunk(&self) -> u64 {
        (self.total_bytes() / 4 / UNIT * UNIT).clamp(UNIT, STREAM_CHUNK)
    }

    /// The largest static file (or range) read whole; larger ones are
    /// streamed. A quarter of the budget, at most 4 MiB, at least a chunk.
    pub(crate) fn whole_read_limit(&self) -> u64 {
        (self.total_bytes() / 4)
            .min(WHOLE_READ_MAX)
            .max(self.stream_chunk())
    }

    fn granted(&self, permit: OwnedSemaphorePermit, kind: BufferKind, bytes: u64) -> BufferPermit {
        let in_use = self.units - self.permits.available_permits() as u64;
        self.peak_units.fetch_max(in_use, Ordering::Relaxed);
        self.reserved_total[kind.index()].fetch_add(bytes, Ordering::Relaxed);
        BufferPermit { permit, bytes }
    }

    /// Reserves `bytes` if they are free now (for blocking callers).
    pub(crate) fn try_reserve(&self, kind: BufferKind, bytes: u64) -> Option<BufferPermit> {
        let units = self.units_for(bytes);
        let permit = self.permits.clone().try_acquire_many_owned(units).ok()?;
        Some(self.granted(permit, kind, bytes))
    }

    /// Reserves `bytes`, waiting up to the budget's wait for them; `None`
    /// when they did not become free in time (the caller answers 503).
    pub(crate) async fn reserve(&self, kind: BufferKind, bytes: u64) -> Option<BufferPermit> {
        if let Some(permit) = self.try_reserve(kind, bytes) {
            return Some(permit);
        }
        self.waits[kind.index()].fetch_add(1, Ordering::Relaxed);
        let units = self.units_for(bytes);
        match tokio::time::timeout(self.wait, self.permits.clone().acquire_many_owned(units)).await
        {
            Ok(Ok(permit)) => Some(self.granted(permit, kind, bytes)),
            _ => {
                self.refused[kind.index()].fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Reserves `bytes` for a streamed file's next chunk, waiting up to
    /// `wait` (the response has started; the bytes ahead of it are released
    /// as their sockets drain or their connections close).
    async fn reserve_chunk(self: Arc<Self>, bytes: u64, wait: Duration) -> Option<BufferPermit> {
        let kind = BufferKind::StaticStream;
        if let Some(permit) = self.try_reserve(kind, bytes) {
            return Some(permit);
        }
        self.waits[kind.index()].fetch_add(1, Ordering::Relaxed);
        let units = self.units_for(bytes);
        match tokio::time::timeout(wait, self.permits.clone().acquire_many_owned(units)).await {
            Ok(Ok(permit)) => Some(self.granted(permit, kind, bytes)),
            _ => {
                self.refused[kind.index()].fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Grows `permit` (or makes it) to cover `bytes`, waiting up to the
    /// budget's wait for the extra room: for a buffer whose size is only
    /// known as it arrives (a proxy request body, RE-4.1 review F1). Each
    /// call asks for the growth alone (at most one received frame), so a
    /// large reservation never waits at the head of the queue for room it
    /// does not yet need. `false`: no room in time (nothing changed).
    pub(crate) async fn grow(
        &self,
        permit: &mut Option<BufferPermit>,
        kind: BufferKind,
        bytes: u64,
    ) -> bool {
        let held = permit.as_ref().map_or(0, |p| p.permit.num_permits() as u64);
        let wanted = u64::from(self.units_for(bytes));
        if wanted <= held {
            if let Some(p) = permit.as_mut() {
                let more = bytes.saturating_sub(p.bytes);
                self.reserved_total[kind.index()].fetch_add(more, Ordering::Relaxed);
                p.bytes = p.bytes.max(bytes);
            }
            return true;
        }
        let extra = (wanted - held) as u32;
        let acquired = match self.permits.clone().try_acquire_many_owned(extra) {
            Ok(acquired) => acquired,
            Err(_) => {
                self.waits[kind.index()].fetch_add(1, Ordering::Relaxed);
                match tokio::time::timeout(
                    self.wait,
                    self.permits.clone().acquire_many_owned(extra),
                )
                .await
                {
                    Ok(Ok(acquired)) => acquired,
                    _ => {
                        self.refused[kind.index()].fetch_add(1, Ordering::Relaxed);
                        return false;
                    }
                }
            }
        };
        let before = permit.as_ref().map_or(0, |p| p.bytes);
        match permit {
            Some(p) => {
                p.permit.merge(acquired);
                p.bytes = bytes;
            }
            None => {
                *permit = Some(BufferPermit {
                    permit: acquired,
                    bytes,
                })
            }
        }
        let in_use = self.units - self.permits.available_permits() as u64;
        self.peak_units.fetch_max(in_use, Ordering::Relaxed);
        self.reserved_total[kind.index()].fetch_add(bytes - before, Ordering::Relaxed);
        true
    }

    pub fn total_bytes(&self) -> u64 {
        self.units * UNIT
    }

    pub fn reserved_bytes(&self) -> u64 {
        (self.units - self.permits.available_permits() as u64) * UNIT
    }

    /// Prometheus text format.
    pub fn to_prometheus(&self) -> String {
        use std::fmt::Write;
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let mut out = format!(
            "# HELP nextrs_native_buffer_budget_bytes Budget for native buffers held whole (static files, proxy request bodies).\n\
             # TYPE nextrs_native_buffer_budget_bytes gauge\n\
             nextrs_native_buffer_budget_bytes {}\n\
             # HELP nextrs_native_buffer_reserved_bytes Native buffer bytes reserved now.\n\
             # TYPE nextrs_native_buffer_reserved_bytes gauge\n\
             nextrs_native_buffer_reserved_bytes {}\n\
             # HELP nextrs_native_buffer_peak_bytes Highest native buffer reservation.\n\
             # TYPE nextrs_native_buffer_peak_bytes gauge\n\
             nextrs_native_buffer_peak_bytes {}\n",
            self.total_bytes(),
            self.reserved_bytes(),
            get(&self.peak_units) * UNIT,
        );
        for (name, help, values) in [
            (
                "nextrs_native_buffer_reserved_bytes_total",
                "Bytes reserved, by buffer kind.",
                &self.reserved_total,
            ),
            (
                "nextrs_native_buffer_waits_total",
                "Reservations that had to wait for room, by buffer kind.",
                &self.waits,
            ),
            (
                "nextrs_native_buffer_refused_total",
                "Reservations refused after the wait (503), by buffer kind.",
                &self.refused,
            ),
        ] {
            let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} counter");
            for kind in BufferKind::ALL {
                let _ = writeln!(
                    out,
                    "{name}{{kind=\"{}\"}} {}",
                    kind.as_str(),
                    get(&values[kind.index()])
                );
            }
        }
        out
    }
}

/// 413 for a proxy-buffered request body that can never fit in the
/// budget. Not a 503: a retry would be refused again.
pub(crate) fn body_exceeds_budget(budget: u64) -> Response {
    let mut response = crate::dynamic::error_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        "native-buffer-budget-exceeded",
        &format!("request body exceeds the {budget}-byte native buffer budget"),
    );
    // The rest of the body is not read; the connection cannot be reused.
    response
        .headers_mut()
        .insert(header::CONNECTION, HeaderValue::from_static("close"));
    response
}

type ChunkRead = Pin<Box<dyn Future<Output = std::io::Result<(std::fs::File, Bytes)>> + Send>>;

/// A static file (or range) streamed in budgeted chunks. The handle is
/// positioned at the first byte to send; `remaining` bytes follow. Each
/// chunk is reserved, then read on the blocking pool, and its `Bytes` own
/// the reservation. hyper polls for the next chunk only when it can buffer
/// it (backpressure), so a slow client holds at most what hyper queues.
/// A file that ends early (truncated while it is sent) fails the body, and
/// hyper closes the connection: the client sees a short body against its
/// Content-Length, never other bytes. Dropping the body (cancellation,
/// disconnect, timeout) drops the handle; a read in flight finishes on the
/// blocking pool and then frees its chunk and reservation.
pub(crate) struct FileStream {
    budget: Arc<NativeBufferBudget>,
    file: Option<std::fs::File>,
    remaining: u64,
    /// The first chunk's reservation, made before the response started.
    first: Option<BufferPermit>,
    reading: Option<ChunkRead>,
    /// How long a later chunk may wait for room.
    wait: Duration,
}

impl FileStream {
    pub fn body(
        budget: Arc<NativeBufferBudget>,
        file: std::fs::File,
        length: u64,
        first: Option<BufferPermit>,
    ) -> Body {
        Body::new(Self {
            budget,
            file: Some(file),
            remaining: length,
            first,
            reading: None,
            wait: STREAM_WAIT,
        })
    }

    #[cfg(test)]
    fn with_wait(
        budget: Arc<NativeBufferBudget>,
        file: std::fs::File,
        length: u64,
        wait: Duration,
    ) -> Body {
        Body::new(Self {
            budget,
            file: Some(file),
            remaining: length,
            first: None,
            reading: None,
            wait,
        })
    }
}

/// Reads exactly `wanted` bytes (blocking) into budgeted `Bytes`.
fn read_chunk(
    mut file: std::fs::File,
    wanted: u64,
    permit: BufferPermit,
) -> std::io::Result<(std::fs::File, Bytes)> {
    let mut bytes = Vec::with_capacity(usize::try_from(wanted).unwrap_or(usize::MAX));
    (&mut file).take(wanted).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != wanted {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "the file is shorter than when its response started",
        ));
    }
    Ok((file, permit.own(bytes)))
}

impl HttpBody for FileStream {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = &mut *self;
        if let Some(reading) = this.reading.as_mut() {
            let result = std::task::ready!(reading.as_mut().poll(cx));
            this.reading = None;
            return Poll::Ready(Some(match result {
                Ok((file, bytes)) => {
                    this.remaining -= bytes.len() as u64;
                    this.file = Some(file);
                    Ok(Frame::data(bytes))
                }
                Err(error) => {
                    this.remaining = 0;
                    Err(axum::Error::new(error))
                }
            }));
        }
        if this.remaining == 0 {
            return Poll::Ready(None);
        }
        let Some(file) = this.file.take() else {
            return Poll::Ready(None);
        };
        let wanted = this.remaining.min(this.budget.stream_chunk());
        let budget = this.budget.clone();
        let first = this.first.take().filter(|p| p.covers(wanted));
        let wait = this.wait;
        this.reading = Some(Box::pin(async move {
            let permit = match first {
                Some(permit) => permit,
                None => budget.reserve_chunk(wanted, wait).await.ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "no room in the native buffer budget for the next chunk",
                    )
                })?,
            };
            tokio::task::spawn_blocking(move || read_chunk(file, wanted, permit))
                .await
                .map_err(std::io::Error::other)?
        }));
        // Poll the new read now (it registers the waker).
        self.poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0 && self.reading.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reservations_wait_for_room_then_give_up() {
        let budget = NativeBufferBudget::new(64 * 1024, Duration::from_millis(50));
        let first = budget
            .try_reserve(BufferKind::StaticFile, 48 * 1024)
            .unwrap();
        assert!(
            budget
                .try_reserve(BufferKind::StaticFile, 32 * 1024)
                .is_none()
        );
        assert!(
            budget
                .reserve(BufferKind::StaticFile, 32 * 1024)
                .await
                .is_none()
        );
        let waiter = {
            let budget = budget.clone();
            tokio::spawn(async move {
                budget
                    .reserve(BufferKind::ProxyBody, 32 * 1024)
                    .await
                    .is_some()
            })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        drop(first);
        assert!(waiter.await.unwrap(), "room freed within the wait");
        assert_eq!(budget.reserved_bytes(), 0, "every permit given back");
        let text = budget.to_prometheus();
        assert!(
            text.contains("nextrs_native_buffer_refused_total{kind=\"static_file\"} 1"),
            "{text}"
        );
        assert!(
            text.contains("nextrs_native_buffer_waits_total{kind=\"proxy_body\"} 1"),
            "{text}"
        );
    }

    #[test]
    fn chunks_and_whole_reads_fit_a_quarter_of_the_budget() {
        let limits = |bytes: u64| {
            let budget = NativeBufferBudget::new(bytes, Duration::ZERO);
            (budget.stream_chunk(), budget.whole_read_limit())
        };
        const KIB: u64 = 1024;
        assert_eq!(limits(32 * 1024 * KIB), (64 * KIB, 4 * 1024 * KIB));
        assert_eq!(limits(1024 * KIB), (64 * KIB, 256 * KIB));
        assert_eq!(limits(40 * KIB), (10 * KIB, 10 * KIB));
        assert_eq!(limits(4 * KIB), (KIB, KIB));
        // Below 4 KiB a chunk is still 1 KiB, which still fits.
        assert_eq!(limits(2 * KIB), (KIB, KIB));
    }

    #[tokio::test]
    async fn a_growing_reservation_takes_only_its_growth_and_gives_all_back() {
        let budget = NativeBufferBudget::new(16 * 1024, Duration::from_millis(50));
        let mut permit = None;
        assert!(budget.grow(&mut permit, BufferKind::ProxyBody, 100).await);
        assert_eq!(budget.reserved_bytes(), 1024);
        assert!(budget.grow(&mut permit, BufferKind::ProxyBody, 900).await);
        assert_eq!(budget.reserved_bytes(), 1024, "still within its unit");
        assert!(
            budget
                .grow(&mut permit, BufferKind::ProxyBody, 10 * 1024)
                .await
        );
        assert_eq!(budget.reserved_bytes(), 10 * 1024);
        // Another holder takes the rest: growth past it waits, then fails,
        // and changes nothing.
        let other = budget
            .try_reserve(BufferKind::StaticFile, 6 * 1024)
            .unwrap();
        assert!(
            !budget
                .grow(&mut permit, BufferKind::ProxyBody, 11 * 1024)
                .await
        );
        assert_eq!(budget.reserved_bytes(), 16 * 1024);
        // While it waited, smaller requests were not blocked behind a
        // reservation of the whole budget: only the 1 KiB growth waited.
        drop(other);
        let bytes = permit.take().unwrap().own(vec![1u8; 10 * 1024]);
        assert_eq!(budget.reserved_bytes(), 10 * 1024);
        drop(bytes);
        assert_eq!(budget.reserved_bytes(), 0);
    }

    #[tokio::test]
    async fn owned_bytes_give_the_reservation_back_with_their_last_clone() {
        let budget = NativeBufferBudget::new(64 * 1024, Duration::from_millis(20));
        let permit = budget
            .try_reserve(BufferKind::StaticFile, 32 * 1024)
            .unwrap();
        let bytes = permit.own(vec![7u8; 1000]);
        let body = Body::from(bytes.clone());
        assert_eq!(body.size_hint().exact(), Some(1000), "length kept");
        let tail = bytes.slice(500..);
        drop(bytes);
        assert_eq!(budget.reserved_bytes(), 32 * 1024);
        let read = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        assert_eq!(read.len(), 1000);
        drop(read);
        assert_eq!(budget.reserved_bytes(), 32 * 1024, "a slice is a clone");
        drop(tail);
        assert_eq!(budget.reserved_bytes(), 0);
    }

    /// A file of `len` bytes (byte i is i % 251) in a fresh directory.
    fn file(name: &str, len: usize) -> (std::path::PathBuf, Vec<u8>) {
        let dir = std::env::temp_dir().join(format!(
            "nextrs-native-budget-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("file.bin");
        let bytes: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        (path, bytes)
    }

    async fn next(body: &mut Body) -> Option<Result<Bytes, axum::Error>> {
        std::future::poll_fn(|cx| Pin::new(&mut *body).poll_frame(cx))
            .await
            .map(|f| f.map(|f| f.into_data().expect("data frames only")))
    }

    /// Waits (bounded) for in-flight blocking reads to finish and free
    /// their chunks.
    async fn settles_at(budget: &NativeBufferBudget, bytes: u64) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while budget.reserved_bytes() != bytes && tokio::time::Instant::now() < deadline {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(budget.reserved_bytes(), bytes);
    }

    #[tokio::test]
    async fn a_streamed_file_is_sent_whole_one_reserved_chunk_at_a_time() {
        // 16 KiB budget: 4 KiB chunks. 100 KiB + 7 bytes of file.
        let budget = NativeBufferBudget::new(16 * 1024, Duration::from_millis(20));
        let (path, expected) = file("whole", 100 * 1024 + 7);
        let mut body = FileStream::body(
            budget.clone(),
            std::fs::File::open(&path).unwrap(),
            expected.len() as u64,
            None,
        );
        assert_eq!(body.size_hint().exact(), Some(expected.len() as u64));
        let mut sent = Vec::new();
        while let Some(chunk) = next(&mut body).await {
            let chunk = chunk.unwrap();
            assert!(chunk.len() as u64 <= budget.stream_chunk());
            // One chunk in hand (in whole KiB: the last one is 7 bytes).
            let units = (chunk.len() as u64).div_ceil(1024) * 1024;
            assert_eq!(budget.reserved_bytes(), units);
            sent.extend_from_slice(&chunk);
        }
        assert_eq!(sent, expected);
        assert_eq!(budget.reserved_bytes(), 0);
        let text = budget.to_prometheus();
        assert!(
            text.contains(
                "nextrs_native_buffer_reserved_bytes_total{kind=\"static_stream\"} 102407"
            ),
            "{text}"
        );
    }

    #[tokio::test]
    async fn chunks_still_held_downstream_hold_back_the_next_one() {
        let budget = NativeBufferBudget::new(16 * 1024, Duration::from_millis(20));
        let (path, expected) = file("held", 64 * 1024);
        let mut body = FileStream::body(
            budget.clone(),
            std::fs::File::open(&path).unwrap(),
            expected.len() as u64,
            None,
        );
        // As if hyper queued four chunks without writing them.
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(next(&mut body).await.unwrap().unwrap());
        }
        assert_eq!(budget.reserved_bytes(), 16 * 1024);
        let blocked = tokio::time::timeout(Duration::from_millis(100), next(&mut body)).await;
        assert!(
            blocked.is_err(),
            "the budget is full: the fifth chunk waits"
        );
        // One chunk written (dropped): the next one is read.
        held.remove(0);
        let fifth = next(&mut body).await.unwrap().unwrap();
        assert_eq!(&fifth[..], &expected[16 * 1024..20 * 1024]);
        assert!(budget.reserved_bytes() <= 16 * 1024);
        drop((held, fifth, body));
        settles_at(&budget, 0).await;
    }

    #[tokio::test]
    async fn a_consumer_that_keeps_every_chunk_fails_instead_of_stalling() {
        // Collecting a stream (as `to_bytes` does) keeps every chunk: with
        // the budget full of them, the next one can never get room.
        let budget = NativeBufferBudget::new(16 * 1024, Duration::from_millis(20));
        let (path, _) = file("collector", 64 * 1024);
        let body = FileStream::with_wait(
            budget.clone(),
            std::fs::File::open(&path).unwrap(),
            64 * 1024,
            Duration::from_millis(200),
        );
        assert!(axum::body::to_bytes(body, usize::MAX).await.is_err());
        assert_eq!(budget.reserved_bytes(), 0);
        assert!(
            budget
                .to_prometheus()
                .contains("nextrs_native_buffer_refused_total{kind=\"static_stream\"} 1")
        );
    }

    #[tokio::test]
    async fn a_file_truncated_while_streaming_fails_the_body() {
        let budget = NativeBufferBudget::new(16 * 1024, Duration::from_millis(20));
        let (path, _) = file("truncated", 40 * 1024);
        let mut body = FileStream::body(
            budget.clone(),
            std::fs::File::open(&path).unwrap(),
            40 * 1024,
            None,
        );
        let first = next(&mut body).await.unwrap().unwrap();
        assert_eq!(first.len(), 4 * 1024);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(6 * 1024)
            .unwrap();
        let second = next(&mut body).await.unwrap();
        assert!(second.is_err(), "2 KiB left of a 4 KiB chunk");
        assert!(next(&mut body).await.is_none(), "nothing after the error");
        drop((first, body));
        assert_eq!(budget.reserved_bytes(), 0);
    }

    #[tokio::test]
    async fn a_file_that_grows_while_streaming_sends_its_declared_length() {
        let budget = NativeBufferBudget::new(16 * 1024, Duration::from_millis(20));
        let (path, expected) = file("grown", 10 * 1024);
        let mut body = FileStream::body(
            budget.clone(),
            std::fs::File::open(&path).unwrap(),
            expected.len() as u64,
            None,
        );
        let mut sent = next(&mut body).await.unwrap().unwrap().to_vec();
        {
            use std::io::Write;
            let mut append = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap();
            append.write_all(&[0xff; 8192]).unwrap();
        }
        while let Some(chunk) = next(&mut body).await {
            sent.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(sent, expected, "the appended bytes are not sent");
        assert_eq!(budget.reserved_bytes(), 0);
    }

    #[tokio::test]
    async fn dropping_a_stream_mid_read_gives_everything_back() {
        let budget = NativeBufferBudget::new(16 * 1024, Duration::from_millis(20));
        let (path, _) = file("dropped", 1024 * 1024);
        for _ in 0..50 {
            let mut body = FileStream::body(
                budget.clone(),
                std::fs::File::open(&path).unwrap(),
                1024 * 1024,
                None,
            );
            let first = next(&mut body).await.unwrap().unwrap();
            // Start the next read, then drop the body while it may be on
            // the blocking pool.
            let _ = tokio::time::timeout(Duration::ZERO, next(&mut body)).await;
            drop(body);
            drop(first);
        }
        settles_at(&budget, 0).await;
    }
}
