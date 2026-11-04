//! The HTTP/1 accept loop. `axum::serve` exposes none of the limits a
//! streaming server needs, so connections are served with hyper directly:
//!
//! * at most `max_connections` at once: beyond that, new connections wait
//!   in the kernel's accept backlog;
//! * hyper's per-connection buffer is capped (`max_buf_size`; hyper's
//!   default is ~400 KiB), which also caps request header size;
//! * a connection whose client stops reading for `write_timeout` is closed.
//!   Its response is dropped, which cancels the request in JavaScript and
//!   releases the response budget. Response backpressure would otherwise
//!   let such a client hold a request open forever;
//! * graceful shutdown: stop accepting, let in-flight requests finish for
//!   up to `shutdown_grace`, then close what remains.
//!
//! The peer address is attached to each request as [`ClientAddr`].

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;
use tokio::time::Sleep;
use tower::Service;

/// The connected peer, in each request's extensions.
#[derive(Debug, Clone, Copy)]
pub struct ClientAddr(pub SocketAddr);

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub max_connections: usize,
    /// Close a connection whose client has not accepted any response bytes
    /// for this long. `None` waits forever.
    pub write_timeout: Option<Duration>,
    /// Time allowed to receive a request's headers (also idle keep-alive).
    pub header_read_timeout: Duration,
    /// hyper's read and write buffer limit per connection.
    pub max_buf_size: usize,
    pub shutdown_grace: Duration,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            max_connections: 1024,
            write_timeout: Some(Duration::from_secs(30)),
            header_read_timeout: Duration::from_secs(30),
            max_buf_size: 64 * 1024,
            shutdown_grace: Duration::from_secs(10),
        }
    }
}

/// Serves `app` on `listener` until `shutdown` resolves, then drains.
pub async fn serve(
    listener: TcpListener,
    app: Router,
    options: ServeOptions,
    shutdown: impl Future<Output = ()>,
) -> io::Result<()> {
    let permits = Arc::new(Semaphore::new(options.max_connections.max(1)));
    let (closing, closed) = watch::channel(false);
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        // Finished connections keep their slot in the set until joined.
        while connections.try_join_next().is_some() {}
        let permit = tokio::select! {
            () = &mut shutdown => break,
            permit = permits.clone().acquire_owned() => permit.expect("never closed"),
        };
        let (stream, peer) = tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(error) => {
                    // EMFILE and friends: back off instead of spinning.
                    tracing::warn!(%error, "accept failed");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
        };
        let _ = stream.set_nodelay(true);
        let app = app.clone();
        let options = options.clone();
        let closed = closed.clone();
        connections.spawn(async move {
            serve_connection(stream, peer, app, &options, closed).await;
            drop(permit);
        });
    }

    // Refuse new connections at once instead of leaving them in the
    // kernel backlog, unanswered, for the whole grace period.
    drop(listener);
    let _ = closing.send(true);
    let drained = tokio::time::timeout(options.shutdown_grace, async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    if drained.is_err() {
        tracing::warn!(
            remaining = connections.len(),
            "shutdown grace expired; closing remaining connections"
        );
        connections.shutdown().await;
    }
    Ok(())
}

async fn serve_connection(
    stream: TcpStream,
    peer: SocketAddr,
    app: Router,
    options: &ServeOptions,
    mut closing: watch::Receiver<bool>,
) {
    let io = TokioIo::new(WriteTimeout::new(stream, options.write_timeout));
    let service = hyper::service::service_fn(move |mut request: hyper::Request<Incoming>| {
        request.extensions_mut().insert(ClientAddr(peer));
        let mut app = app.clone();
        async move { app.call(request).await }
    });
    let connection = http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(options.header_read_timeout)
        .max_buf_size(options.max_buf_size.max(8192))
        .keep_alive(true)
        .serve_connection(io, service);
    tokio::pin!(connection);
    let result = tokio::select! {
        result = connection.as_mut() => result,
        () = async {
            // Drop the watch guard before awaiting the connection.
            let _ = closing.wait_for(|closing| *closing).await;
        } => {
            connection.as_mut().graceful_shutdown();
            connection.as_mut().await
        }
    };
    if let Err(error) = result {
        tracing::debug!(%peer, %error, "connection ended with an error");
    }
}

/// Fails writes that make no progress for `timeout`.
struct WriteTimeout<S> {
    inner: S,
    timeout: Option<Duration>,
    stalled: Option<Pin<Box<Sleep>>>,
}

impl<S> WriteTimeout<S> {
    fn new(inner: S, timeout: Option<Duration>) -> Self {
        Self {
            inner,
            timeout,
            stalled: None,
        }
    }

    fn on_pending<T>(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<T>> {
        let Some(timeout) = self.timeout else {
            return Poll::Pending;
        };
        let sleep = self
            .stalled
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(timeout)));
        match sleep.as_mut().poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the client stopped reading the response",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn on_ready<T>(&mut self, result: io::Result<T>) -> Poll<io::Result<T>> {
        self.stalled = None;
        Poll::Ready(result)
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for WriteTimeout<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for WriteTimeout<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write(cx, buf) {
            Poll::Ready(result) => self.on_ready(result),
            Poll::Pending => self.on_pending(cx),
        }
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write_vectored(cx, bufs) {
            Poll::Ready(result) => self.on_ready(result),
            Poll::Pending => self.on_pending(cx),
        }
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.inner).poll_flush(cx) {
            Poll::Ready(result) => self.on_ready(result),
            Poll::Pending => self.on_pending(cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
