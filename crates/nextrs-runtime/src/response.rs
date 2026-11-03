//! Response flow control between the isolate and the HTTP connection.
//!
//! Every body chunk JavaScript writes is charged to a per-response byte
//! budget, and stays charged until the HTTP layer drops the `Bytes`: that
//! is, after hyper has written it to the socket, not merely when it left the
//! channel. JavaScript may only write while the response is under budget;
//! once a write reaches it, `ServerResponse` holds the write callback until
//! the consumer drains, so Node's own `write()`/`'drain'` backpressure
//! reaches the producer (Next's `pipeToNodeResponse` waits for `'drain'`).
//!
//! So per response, process memory held for body bytes on the Rust side is
//! at most `budget + one chunk`. The event channel is an unbounded mpsc only
//! in type: chunks cannot be sent while the budget is exhausted, and there
//! are at most three control events per response.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bytes::Bytes;
use tokio::sync::{Notify, mpsc};

use crate::metrics::{self, Counters};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseEvent {
    Head {
        status: u16,
        headers: Vec<(String, String)>,
    },
    Chunk(Bytes),
    End,
    /// The handler failed after the response started, or the executor
    /// failed. The body is incomplete.
    Error(String),
}

/// Result of offering one chunk to the response (`op_nextrs_response_write`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteStatus {
    /// Queued; the response is still under budget.
    Accepted = 0,
    /// Queued, and the budget is now used up: wait for `drained()` before
    /// reporting the write as complete.
    Full = 1,
    /// Not queued: the budget was already used up. Wait, then retry.
    Busy = 2,
    /// The consumer is gone (client disconnected, or the request was
    /// cancelled). The chunk was dropped.
    Closed = 3,
}

struct Flow {
    budget: usize,
    buffered: AtomicUsize,
    /// The consumer (`ResponseStream`) was dropped.
    closed: AtomicBool,
    /// JavaScript took the request from the executor's inbox.
    delivered: AtomicBool,
    /// `End` or `Error` was sent.
    finished: AtomicBool,
    /// A waiting writer: woken when bytes are released or the stream closes.
    drained: Notify,
    /// Woken (all waiters) when the stream closes.
    closing: Notify,
    counters: Arc<Counters>,
}

impl Flow {
    fn release(&self, len: usize) {
        let before = self.buffered.fetch_sub(len, Ordering::AcqRel);
        if before - len < self.budget {
            self.drained.notify_one();
        }
    }
}

/// A body chunk that returns its bytes to the budget when the HTTP layer
/// drops it.
struct Charged {
    data: Vec<u8>,
    flow: Arc<Flow>,
}

impl AsRef<[u8]> for Charged {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl Drop for Charged {
    fn drop(&mut self) {
        self.flow.release(self.data.len());
    }
}

pub(crate) fn channel(budget: usize, counters: Arc<Counters>) -> (ResponseSender, ResponseStream) {
    let (events, receiver) = mpsc::unbounded_channel();
    let flow = Arc::new(Flow {
        budget: budget.max(1),
        buffered: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
        delivered: AtomicBool::new(false),
        finished: AtomicBool::new(false),
        drained: Notify::new(),
        closing: Notify::new(),
        counters,
    });
    (
        ResponseSender {
            events,
            flow: flow.clone(),
        },
        ResponseStream {
            events: receiver,
            flow,
        },
    )
}

/// The isolate's end of one response.
pub(crate) struct ResponseSender {
    events: mpsc::UnboundedSender<ResponseEvent>,
    flow: Arc<Flow>,
}

impl ResponseSender {
    pub fn mark_delivered(&self) {
        self.flow.delivered.store(true, Ordering::Release);
    }

    pub fn is_closed(&self) -> bool {
        self.flow.closed.load(Ordering::Acquire) || self.events.is_closed()
    }

    pub fn head(&self, status: u16, headers: Vec<(String, String)>) -> bool {
        self.events
            .send(ResponseEvent::Head { status, headers })
            .is_ok()
    }

    pub fn write(&self, chunk: &[u8]) -> WriteStatus {
        if self.is_closed() {
            return WriteStatus::Closed;
        }
        let flow = &self.flow;
        let buffered = flow.buffered.load(Ordering::Acquire);
        if buffered >= flow.budget {
            return WriteStatus::Busy;
        }
        // Charge before sending, so the consumer can never release more
        // than was charged.
        flow.buffered.fetch_add(chunk.len(), Ordering::AcqRel);
        let charged = Charged {
            data: chunk.to_vec(),
            flow: flow.clone(),
        };
        if self
            .events
            .send(ResponseEvent::Chunk(Bytes::from_owner(charged)))
            .is_err()
        {
            return WriteStatus::Closed;
        }
        if buffered + chunk.len() >= flow.budget {
            WriteStatus::Full
        } else {
            WriteStatus::Accepted
        }
    }

    /// Ends the response. Returns false if nobody was listening.
    pub fn finish(&self, event: ResponseEvent) -> bool {
        self.flow.finished.store(true, Ordering::Release);
        self.events.send(event).is_ok()
    }

    pub fn flow(&self) -> FlowHandle {
        FlowHandle(self.flow.clone())
    }
}

/// What async ops hold while they wait: no sender, so waiting never keeps
/// the event channel open.
#[derive(Clone)]
pub(crate) struct FlowHandle(Arc<Flow>);

impl FlowHandle {
    /// Resolves true once the response is back under budget, false if the
    /// consumer went away first.
    pub async fn drained(&self) -> bool {
        let flow = &self.0;
        loop {
            if flow.closed.load(Ordering::Acquire) {
                return false;
            }
            if flow.buffered.load(Ordering::Acquire) < flow.budget {
                return true;
            }
            // notify_one stores a permit when nobody waits, so a release
            // between the checks above and this await is not lost.
            flow.drained.notified().await;
        }
    }

    /// Resolves when the consumer has dropped the response: after the body
    /// was delivered, or when the client went away.
    pub async fn closed(&self) {
        let flow = &self.0;
        let closing = flow.closing.notified();
        tokio::pin!(closing);
        closing.as_mut().enable();
        if flow.closed.load(Ordering::Acquire) {
            return;
        }
        closing.await;
    }
}

/// The HTTP side of one response. Events arrive in order: one `Head`, any
/// number of `Chunk`s, then `End` or `Error`. `recv()` returning `None`
/// without `End`/`Error` means the executor failed (see
/// [`ResponseStream::was_delivered`]). Dropping the stream cancels the
/// request: JavaScript sees `close` on `req`/`res`.
pub struct ResponseStream {
    events: mpsc::UnboundedReceiver<ResponseEvent>,
    flow: Arc<Flow>,
}

impl ResponseStream {
    pub async fn recv(&mut self) -> Option<ResponseEvent> {
        self.events.recv().await
    }

    /// Whether JavaScript ever received the request. A request that was
    /// never delivered had no side effects and is safe for the client to
    /// retry; one that was delivered may have run partially.
    pub fn was_delivered(&self) -> bool {
        self.flow.delivered.load(Ordering::Acquire)
    }

    /// Body bytes charged to this response and not yet written out.
    pub fn buffered_bytes(&self) -> usize {
        self.flow.buffered.load(Ordering::Acquire)
    }

    pub fn budget(&self) -> usize {
        self.flow.budget
    }
}

impl std::fmt::Debug for ResponseStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseStream")
            .field("buffered", &self.buffered_bytes())
            .field("budget", &self.flow.budget)
            .finish()
    }
}

impl Drop for ResponseStream {
    fn drop(&mut self) {
        let flow = &self.flow;
        flow.closed.store(true, Ordering::Release);
        if flow.delivered.load(Ordering::Acquire) && !flow.finished.load(Ordering::Acquire) {
            metrics::add(&flow.counters.responses_cancelled, 1);
        }
        flow.drained.notify_one();
        flow.closing.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(budget: usize) -> (ResponseSender, ResponseStream) {
        channel(budget, Arc::new(Counters::default()))
    }

    #[test]
    fn writes_stop_at_the_budget_until_bytes_are_released() {
        let (sender, mut stream) = pair(10);
        assert_eq!(sender.write(b"1234"), WriteStatus::Accepted);
        assert_eq!(sender.write(b"123456"), WriteStatus::Full);
        assert_eq!(sender.write(b"x"), WriteStatus::Busy);
        assert_eq!(stream.buffered_bytes(), 10);
        let Some(ResponseEvent::Chunk(first)) = stream.events.try_recv().ok() else {
            panic!("expected a chunk");
        };
        // Still charged while the HTTP layer holds the bytes.
        assert_eq!(stream.buffered_bytes(), 10);
        drop(first);
        assert_eq!(stream.buffered_bytes(), 6);
        assert_eq!(sender.write(b"x"), WriteStatus::Accepted);
    }

    #[test]
    fn an_oversized_chunk_is_accepted_alone() {
        let (sender, stream) = pair(4);
        assert_eq!(sender.write(&[0; 100]), WriteStatus::Full);
        assert_eq!(sender.write(b"y"), WriteStatus::Busy);
        assert_eq!(stream.buffered_bytes(), 100);
    }

    #[tokio::test]
    async fn drained_wakes_on_release_and_on_close() {
        let (sender, mut stream) = pair(4);
        assert_eq!(sender.write(b"abcd"), WriteStatus::Full);
        let flow = sender.flow();
        let waiter = tokio::spawn(async move { flow.drained().await });
        tokio::task::yield_now().await;
        let chunk = stream.recv().await;
        drop(chunk);
        assert!(waiter.await.unwrap());

        assert_eq!(sender.write(b"abcd"), WriteStatus::Full);
        let flow = sender.flow();
        let waiter = tokio::spawn(async move { flow.drained().await });
        tokio::task::yield_now().await;
        // The client goes away while the writer waits for capacity.
        drop(stream);
        assert!(!waiter.await.unwrap());
        assert_eq!(sender.write(b"z"), WriteStatus::Closed);
    }

    #[tokio::test]
    async fn closed_resolves_when_the_stream_is_dropped() {
        let (sender, stream) = pair(4);
        let flow = sender.flow();
        let waiter = tokio::spawn(async move { flow.closed().await });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());
        drop(stream);
        waiter.await.unwrap();
        // And immediately when it is already gone.
        sender.flow().closed().await;
    }
}
