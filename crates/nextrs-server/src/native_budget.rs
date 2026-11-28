//! A byte budget for native buffers the server holds whole (RE-4C),
//! optional (`--native-buffer-budget`): static and `public/` files read
//! into memory for their response, and request bodies buffered for the
//! proxy (middleware). Each such buffer reserves its size before it is
//! allocated and gives it back when the last owner is dropped (RAII
//! permit): the response body once hyper has written or abandoned it, the
//! request body once both the proxy's and the route's copy are gone. That
//! covers normal completion, errors, cancellation, timeouts and client
//! disconnects alike, since all of them drop the body.
//!
//! When the budget is full, a request waits (first come, first served, as
//! the image budget does) up to `wait`, then gets a 503 with Retry-After.
//! A buffer larger than the whole budget takes all of it, so it is served
//! alone instead of never: the aggregate bound is the larger of the budget
//! and the largest single buffer. Other native buffers keep their own
//! limits (RE-4-NATIVE-BUFFER-BUDGETS).

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use http_body::{Body as HttpBody, Frame, SizeHint};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Reservations are counted in KiB (a semaphore's permits are u32).
const UNIT: u64 = 1024;

/// Which kind of buffer asked, for metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BufferKind {
    StaticFile,
    ProxyBody,
}

impl BufferKind {
    const ALL: [Self; 2] = [Self::StaticFile, Self::ProxyBody];

    fn index(self) -> usize {
        match self {
            Self::StaticFile => 0,
            Self::ProxyBody => 1,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::StaticFile => "static_file",
            Self::ProxyBody => "proxy_body",
        }
    }
}

pub struct NativeBufferBudget {
    permits: Arc<Semaphore>,
    units: u64,
    wait: Duration,
    peak_units: AtomicU64,
    reserved_total: [AtomicU64; 2],
    waits: [AtomicU64; 2],
    refused: [AtomicU64; 2],
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
    _permit: OwnedSemaphorePermit,
    bytes: u64,
}

impl BufferPermit {
    /// Whether this permit already covers `bytes`.
    pub fn covers(&self, bytes: u64) -> bool {
        self.bytes >= bytes
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

    /// Units for `bytes`, at most the whole budget.
    fn units_for(&self, bytes: u64) -> u32 {
        bytes.div_ceil(UNIT).clamp(1, self.units) as u32
    }

    fn granted(&self, permit: OwnedSemaphorePermit, kind: BufferKind, bytes: u64) -> BufferPermit {
        let in_use = self.units - self.permits.available_permits() as u64;
        self.peak_units.fetch_max(in_use, Ordering::Relaxed);
        self.reserved_total[kind.index()].fetch_add(bytes, Ordering::Relaxed);
        BufferPermit {
            _permit: permit,
            bytes,
        }
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

/// A body that holds a reservation until it is dropped.
pub(crate) struct PermitBody {
    inner: Body,
    _permit: Arc<BufferPermit>,
}

impl PermitBody {
    pub fn wrap(inner: Body, permit: Arc<BufferPermit>) -> Body {
        Body::new(Self {
            inner,
            _permit: permit,
        })
    }
}

impl HttpBody for PermitBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.inner).poll_frame(cx)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
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

    #[tokio::test]
    async fn a_buffer_larger_than_the_budget_takes_all_of_it() {
        let budget = NativeBufferBudget::new(64 * 1024, Duration::from_millis(20));
        let big = budget
            .try_reserve(BufferKind::StaticFile, 10 * 1024 * 1024)
            .unwrap();
        assert_eq!(budget.reserved_bytes(), 64 * 1024);
        assert!(big.covers(10 * 1024 * 1024));
        assert!(budget.try_reserve(BufferKind::StaticFile, 1).is_none());
        drop(big);
        assert!(budget.try_reserve(BufferKind::StaticFile, 1).is_some());
    }

    #[tokio::test]
    async fn a_permit_body_releases_its_bytes_when_dropped() {
        let budget = NativeBufferBudget::new(64 * 1024, Duration::from_millis(20));
        let permit = Arc::new(
            budget
                .try_reserve(BufferKind::StaticFile, 32 * 1024)
                .unwrap(),
        );
        let body = PermitBody::wrap(Body::from(vec![7u8; 1000]), permit);
        assert_eq!(body.size_hint().exact(), Some(1000), "length kept");
        assert_eq!(budget.reserved_bytes(), 32 * 1024);
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        assert_eq!(bytes.len(), 1000);
        assert_eq!(budget.reserved_bytes(), 0);
    }
}
