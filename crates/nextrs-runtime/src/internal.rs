//! Next.js fetching its own server. Two Server Action paths do it, with
//! `fetch(url, { next: { internal: 1 } })` to the request's own origin:
//!
//! * a fetch action that calls `redirect()` fetches the target's RSC
//!   payload, so the browser gets it in the action response;
//! * an action whose module is not in the requested page's worker is
//!   forwarded (re-POSTed) to a page that has it.
//!
//! next start answers these over loopback. NextRS hands them to the HTTP
//! layer in-process (`InternalDispatch`, installed by the server), so the
//! application gets no network access for it. A request goes the normal
//! way (routing, admission, limits, deadlines). Only
//! `http://localhost` URLs (the origin Next builds `initURL` from when it
//! runs behind an adapter) with Next's internal marker take this path,
//! and a process-wide semaphore bounds how many run at once, so a
//! route that fetches itself recursively fails instead of growing.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use bytes::Bytes;
use futures_util::stream::BoxStream;
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::net::{BodyItem, FetchError, FetchHead, FetchTask, OutBody};

/// Internal requests in flight across the process.
pub const MAX_INTERNAL_REQUESTS: usize = 64;

/// A request to the application itself, as Next.js sent it.
pub struct InternalRequest {
    pub method: String,
    /// Path and query of the target URL.
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body: InternalBody,
}

pub enum InternalBody {
    Empty,
    Bytes(Bytes),
    /// Chunks written by JavaScript as it reads the upload.
    Stream(mpsc::Receiver<Bytes>),
}

pub struct InternalResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: BoxStream<'static, Result<Bytes, String>>,
}

/// Serves internal requests; implemented by the HTTP layer.
pub trait InternalDispatch: Send + Sync + 'static {
    fn dispatch(
        &self,
        request: InternalRequest,
    ) -> Pin<Box<dyn Future<Output = InternalResponse> + Send + 'static>>;
}

struct Installed {
    dispatch: Arc<dyn InternalDispatch>,
    runtime: tokio::runtime::Handle,
    permits: Arc<Semaphore>,
}

/// Shared by the pool and every isolate; empty until the server installs
/// a dispatcher.
#[derive(Clone, Default)]
pub(crate) struct InternalRoute(Arc<RwLock<Option<Arc<Installed>>>>);

impl InternalRoute {
    /// Installs the dispatcher (replacing an earlier one); its futures run
    /// on the calling runtime.
    pub fn install(&self, dispatch: Arc<dyn InternalDispatch>) {
        let installed = Arc::new(Installed {
            dispatch,
            runtime: tokio::runtime::Handle::current(),
            permits: Arc::new(Semaphore::new(MAX_INTERNAL_REQUESTS)),
        });
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = Some(installed);
    }

    /// Whether `url` is one Next.js addresses its own server with.
    pub fn handles(url: &url::Url) -> bool {
        url.scheme() == "http" && url.host_str() == Some("localhost") && url.port().is_none()
    }

    pub fn start(
        &self,
        method: String,
        url: &url::Url,
        headers: Vec<(String, String)>,
        body: OutBody,
    ) -> Result<FetchTask, FetchError> {
        let installed = self.0.read().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(installed) = installed else {
            return Err(FetchError::Invalid(
                "internal requests are not available in this process".into(),
            ));
        };
        let Ok(permit) = installed.permits.clone().try_acquire_owned() else {
            return Err(FetchError::Network(format!(
                "more than {MAX_INTERNAL_REQUESTS} internal requests in flight"
            )));
        };
        let path_and_query = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_owned(),
        };
        let request = InternalRequest {
            method,
            path_and_query,
            headers,
            body: match body {
                OutBody::None => InternalBody::Empty,
                OutBody::Bytes(bytes) => InternalBody::Bytes(bytes),
                OutBody::Stream(rx) => InternalBody::Stream(rx),
            },
        };
        let (head_tx, head) = oneshot::channel();
        let (body_tx, body) = mpsc::channel(1);
        let href = url.to_string();
        let dispatch = installed.dispatch.clone();
        let task = installed.runtime.spawn(async move {
            let _permit = permit;
            let response = dispatch.dispatch(request).await;
            let head = FetchHead {
                status: response.status,
                status_text: String::new(),
                headers: response.headers,
                url: href,
                redirected: false,
            };
            if head_tx.send(Ok(head)).is_err() {
                return;
            }
            let mut stream = response.body;
            use futures_util::StreamExt;
            while let Some(item) = stream.next().await {
                let item = match item {
                    Ok(chunk) => BodyItem::Chunk(chunk),
                    Err(error) => {
                        let _ = body_tx
                            .send(BodyItem::Error(FetchError::Network(error)))
                            .await;
                        return;
                    }
                };
                if body_tx.send(item).await.is_err() {
                    return;
                }
            }
            let _ = body_tx.send(BodyItem::End).await;
        });
        Ok(FetchTask {
            head,
            body,
            abort: task.abort_handle(),
        })
    }
}
