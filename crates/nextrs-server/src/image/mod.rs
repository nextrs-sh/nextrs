//! `/_next/image` without JavaScript: next start's `handleNextImageRequest`
//! (parameter validation, cache, `imageOptimizer`, `sendResponse`), with the
//! transformation in Rust (`nextrs-image`) on a bounded pool of blocking
//! threads, never on a Tokio worker or a V8 isolate.

mod accept;
mod budget;
mod cache;
#[cfg(feature = "negative-controls")]
mod controls;
mod fetch;
mod params;
mod patterns;
mod response;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, Method};
use axum::response::Response;
use bytes::Bytes;
use nextrs_adapter::ImagesConfig;
use nextrs_image::ImageType;
use tokio::sync::{Notify, Semaphore, watch};

pub(crate) use cache::Entry;
pub use cache::ImageCacheOptions;
use fetch::{Failure, Source};

/// Image optimization settings beyond the build's `images` config.
#[derive(Debug, Clone)]
pub struct ImageOptions {
    /// Remote images are fetched under this policy (`--net-allow`), with
    /// a client of their own. `None`: remote images are refused.
    pub network: Option<nextrs_runtime::NetworkConfig>,
    /// Transformations running at once (each on its own blocking thread).
    pub concurrency: usize,
    /// Transformations waiting for a thread; more answer 503.
    pub max_queued: usize,
    /// NextRS's own bound on source pixels, applied below
    /// `experimental.imgOptMaxInputPixels`. Larger sources are served
    /// unchanged, as next start does when sharp refuses them.
    pub max_input_pixels: u64,
    /// Bytes one decode may allocate.
    pub max_decode_bytes: u64,
    /// Overrides `experimental.imgOptTimeoutInSeconds`.
    pub timeout: Option<Duration>,
    /// Bytes transformations may hold at once, each reserving what its
    /// source's header says it will allocate (`nextrs_image::estimate`)
    /// before it starts. Work estimated above the whole budget is not
    /// attempted: the source is served unchanged, as for too many pixels.
    pub memory_budget: u64,
    /// Source bytes held in memory at once (downloading, waiting for a
    /// transformation, being transformed); beyond it, 503.
    pub source_budget: u64,
    pub cache: ImageCacheOptions,
    /// Test-only fault injection; never set by the CLI.
    #[doc(hidden)]
    pub faults: ImageFaults,
}

/// Faults injected into the transformation stage (tests only).
#[doc(hidden)]
#[derive(Clone, Default)]
pub struct ImageFaults {
    /// Called on the transformation's thread, with its reservations held,
    /// before the transformation; with the source's href.
    #[allow(clippy::type_complexity)]
    pub before_transform: Option<Arc<dyn Fn(&str) -> ImageFault + Send + Sync>>,
}

impl std::fmt::Debug for ImageFaults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.before_transform.is_some() {
            "ImageFaults(active)"
        } else {
            "ImageFaults(none)"
        })
    }
}

#[doc(hidden)]
#[derive(Debug, Clone)]
pub enum ImageFault {
    None,
    /// Blocks the thread this long, then transforms.
    Sleep(Duration),
    Panic,
    DecodeError,
    EncodeError,
}

impl Default for ImageOptions {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism().map_or(2, |n| n.get());
        Self {
            network: None,
            concurrency: (cpus / 2).max(1),
            max_queued: 64,
            max_input_pixels: 64_000_000,
            max_decode_bytes: 512 * 1024 * 1024,
            timeout: None,
            memory_budget: 1024 * 1024 * 1024,
            source_budget: 512 * 1024 * 1024,
            cache: ImageCacheOptions::default(),
            faults: ImageFaults::default(),
        }
    }
}

/// The build's `images` settings, compiled.
pub(crate) struct Config {
    pub sizes: Vec<u32>,
    pub qualities: Option<Vec<u32>>,
    pub formats: Vec<String>,
    pub base_path: String,
    pub patterns: patterns::Patterns,
    pub images: ImagesConfig,
}

type Outcome = Result<Arc<Entry>, Failure>;

struct Flight {
    done: watch::Sender<Option<Outcome>>,
    /// Requests waiting for this result. When the last one leaves (the
    /// clients disconnected), the work is cancelled.
    waiters: AtomicUsize,
    /// A background revalidation: nobody waits, never cancelled.
    background: bool,
    cancelled: Arc<AtomicBool>,
    cancel: Notify,
}

impl Flight {
    fn leave(&self) {
        if self.waiters.fetch_sub(1, Ordering::AcqRel) == 1 && !self.background {
            self.cancelled.store(true, Ordering::Release);
            self.cancel.notify_waiters();
        }
    }
}

/// One place in the transformation queue, released on drop.
struct Queued<'a>(&'a AtomicUsize);

impl Drop for Queued<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct Waiting(Arc<Flight>);

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0.leave();
    }
}

#[derive(Default)]
pub(crate) struct Counters {
    pub requests: AtomicU64,
    pub transforms: AtomicU64,
    pub transform_failures: AtomicU64,
    pub fallbacks: AtomicU64,
    pub overloaded: AtomicU64,
    pub cancelled: AtomicU64,
    pub coalesced: AtomicU64,
    /// Work whose estimate exceeds the whole memory budget (source served
    /// unchanged), and work that waited for memory.
    pub memory_rejected: AtomicU64,
    pub memory_waits: AtomicU64,
    pub decode_us: AtomicU64,
    pub resize_us: AtomicU64,
    pub encode_us: AtomicU64,
}

pub(crate) struct Optimizer {
    config: Config,
    /// `None` where next start would not cache (`maximumDiskCacheSize: 0`,
    /// `isrFlushToDisk: false`): every request is transformed.
    cache: Option<cache::ImageCache>,
    upstream: Option<nextrs_runtime::Upstream>,
    permits: Arc<Semaphore>,
    memory: budget::MemoryBudget,
    sources: Arc<budget::SourceBudget>,
    faults: ImageFaults,
    queued: AtomicUsize,
    max_queued: usize,
    max_flights: usize,
    limits: nextrs_image::Limits,
    timeout: Duration,
    flights: Mutex<HashMap<String, Arc<Flight>>>,
    /// The server, for internal image requests (set once it exists).
    app: OnceLock<Weak<crate::AppState>>,
    pub counters: Counters,
}

impl Optimizer {
    pub(crate) fn new(
        images: &ImagesConfig,
        base_path: &str,
        build_id: &str,
        options: &ImageOptions,
    ) -> Result<Self, String> {
        let patterns = patterns::Patterns::compile(images)?;
        let next_caches =
            images.optimizer.flush_to_disk && images.maximum_disk_cache_size != Some(0);
        let cache = if next_caches {
            let mut cache_options = options.cache.clone();
            if let Some(max) = images.maximum_disk_cache_size {
                cache_options.max_disk_bytes = cache_options.max_disk_bytes.min(max);
            }
            Some(
                cache::ImageCache::open(cache_options, build_id)
                    .map_err(|e| format!("image cache: {e}"))?,
            )
        } else {
            None
        };
        // Next refuses non-public addresses unless dangerouslyAllowLocalIP;
        // NextRS additionally requires them in --net-allow-addr. Both must
        // allow an address.
        let upstream = match &options.network {
            Some(network) => {
                let mut network = network.clone();
                if !images.dangerously_allow_local_ip {
                    network.policy.allow_addrs.clear();
                }
                Some(nextrs_runtime::Upstream::standalone(&network)?)
            }
            None => None,
        };
        let concurrency = options.concurrency.max(1);
        Ok(Self {
            config: Config {
                sizes: images.sizes().collect(),
                qualities: images.qualities.clone(),
                formats: images.formats.clone(),
                base_path: base_path.to_owned(),
                patterns,
                images: images.clone(),
            },
            cache,
            upstream,
            permits: Arc::new(Semaphore::new(concurrency)),
            memory: budget::MemoryBudget::new(options.memory_budget),
            sources: budget::SourceBudget::new(options.source_budget),
            faults: options.faults.clone(),
            queued: AtomicUsize::new(0),
            max_queued: options.max_queued,
            max_flights: concurrency + options.max_queued,
            limits: nextrs_image::Limits {
                max_input_pixels: options
                    .max_input_pixels
                    .min(images.optimizer.max_input_pixels),
                max_alloc_bytes: options.max_decode_bytes,
            },
            timeout: options
                .timeout
                .unwrap_or(Duration::from_secs(images.optimizer.timeout_seconds.max(1))),
            flights: Mutex::default(),
            app: OnceLock::new(),
            counters: Counters::default(),
        })
    }

    pub(crate) fn attach(&self, app: Weak<crate::AppState>) {
        let _ = self.app.set(app);
    }

    pub(crate) fn images(&self) -> &ImagesConfig {
        &self.config.images
    }

    /// One `/_next/image` request (after routing resolved it to the
    /// endpoint).
    pub(crate) async fn serve(
        self: &Arc<Self>,
        method: &Method,
        headers: &HeaderMap,
        query: &crate::routing::query::JsMap,
    ) -> Response {
        self.counters.requests.fetch_add(1, Ordering::Relaxed);
        if self.config.images.optimizer.export {
            return response::text(400, "Bad Request");
        }
        let accept = headers
            .get("accept")
            .map(|v| crate::resolve::latin1(v.as_bytes()));
        let params = match params::validate(query, accept.as_deref(), &self.config) {
            Ok(params) => params,
            Err(params::Invalid::Message(message)) => return response::text(400, &message),
            Err(params::Invalid::Throws) => return response::internal_error(),
        };
        let key = params::cache_key(&params);
        let cached = match &self.cache {
            Some(cache) => cache.get(&key).await,
            None => None,
        };
        let now = now_ms();
        let (entry, x_cache) = match cached {
            Some(entry) if now <= entry.expire_at_ms => (Ok(entry), "HIT"),
            Some(stale) => {
                // Served stale; regenerated in the background.
                let _ = self.join(&key, &params, Some(stale.clone()), true);
                (Ok(stale), "STALE")
            }
            None => (self.wait(&key, &params).await, "MISS"),
        };
        match entry {
            Ok(entry) => response::image(
                method,
                headers,
                &params,
                &entry,
                x_cache,
                &self.config.images,
            ),
            Err(failure) => response::failure(&failure),
        }
    }

    /// Joins (or starts) the work for `key` and waits for its outcome. A
    /// client that disconnects leaves; the last one to leave cancels it.
    async fn wait(self: &Arc<Self>, key: &str, params: &params::Params) -> Outcome {
        let (mut rx, guard) = match self.join(key, params, None, false) {
            Ok(joined) => joined,
            Err(failure) => return Err(failure),
        };
        let outcome = loop {
            if let Some(outcome) = rx.borrow().clone() {
                break outcome;
            }
            if rx.changed().await.is_err() {
                break Err(Failure::Internal);
            }
        };
        drop(guard);
        outcome
    }

    #[allow(clippy::type_complexity)]
    fn join(
        self: &Arc<Self>,
        key: &str,
        params: &params::Params,
        previous: Option<Arc<Entry>>,
        background: bool,
    ) -> Result<(watch::Receiver<Option<Outcome>>, Option<Waiting>), Failure> {
        let mut flights = self.flights.lock().expect("flights lock");
        if let Some(flight) = flights.get(key) {
            self.counters.coalesced.fetch_add(1, Ordering::Relaxed);
            let rx = flight.done.subscribe();
            if background {
                return Ok((rx, None));
            }
            flight.waiters.fetch_add(1, Ordering::AcqRel);
            return Ok((rx, Some(Waiting(flight.clone()))));
        }
        if flights.len() >= self.max_flights {
            self.counters.overloaded.fetch_add(1, Ordering::Relaxed);
            return Err(Failure::Overloaded);
        }
        let (done, rx) = watch::channel(None);
        let flight = Arc::new(Flight {
            done,
            waiters: AtomicUsize::new(usize::from(!background)),
            background,
            cancelled: Arc::new(AtomicBool::new(false)),
            cancel: Notify::new(),
        });
        flights.insert(key.to_owned(), flight.clone());
        drop(flights);
        let guard = (!background).then(|| Waiting(flight.clone()));
        let this = self.clone();
        let key = key.to_owned();
        let params = params.clone();
        // Runs on its own: a leaving client does not stop it unless it was
        // the last one waiting.
        tokio::spawn(async move {
            let outcome = tokio::select! {
                outcome = this.produce(&key, &params, previous, &flight) => outcome,
                () = cancelled(&flight) => {
                    this.counters.cancelled.fetch_add(1, Ordering::Relaxed);
                    Err(Failure::Internal)
                }
            };
            this.flights.lock().expect("flights lock").remove(&key);
            let _ = flight.done.send(Some(outcome));
        });
        Ok((rx, guard))
    }

    async fn produce(
        self: &Arc<Self>,
        key: &str,
        params: &params::Params,
        previous: Option<Arc<Entry>>,
        flight: &Flight,
    ) -> Outcome {
        let images = &self.config.images;
        let source = if params.is_absolute {
            fetch::external(
                self.upstream.as_ref(),
                &params.href,
                images.maximum_response_body,
                images.maximum_redirects,
                &self.sources,
            )
            .await?
        } else {
            let app = self
                .app
                .get()
                .and_then(Weak::upgrade)
                .ok_or(Failure::Internal)?;
            let request = fetch::internal_request(&params.href).ok_or(Failure::Internal)?;
            let response = crate::image_subrequest(app, request).await;
            fetch::internal(response, images.maximum_response_body, &self.sources).await?
        };
        let produced = self
            .transform(source, params, previous.as_deref(), flight)
            .await?;
        let entry = Arc::new(produced);
        if let Some(cache) = &self.cache {
            cache.set(key, entry.clone()).await;
        }
        Ok(entry)
    }

    /// `imageOptimizerTransform`.
    async fn transform(
        &self,
        source: Source,
        params: &params::Params,
        previous: Option<&Entry>,
        flight: &Flight,
    ) -> Result<Entry, Failure> {
        let images = &self.config.images;
        let min_ttl = images.minimum_cache_ttl;
        let max_age = min_ttl.max(fetch::max_age(source.cache_control.as_deref()));
        let entry = |body: Bytes, content_type: &str, max_age: u64, etag: String| Entry {
            body,
            content_type: content_type.to_owned(),
            etag,
            upstream_etag: source.etag.clone(),
            max_age,
            expire_at_ms: now_ms() + max_age.max(min_ttl) * 1000,
        };
        let upstream_type = match nextrs_image::detect_content_type(&source.buffer) {
            Err(_) => return Err(Failure::Internal),
            Ok(Some(t)) if t.mime().starts_with("image/") => t,
            Ok(_) => {
                tracing::error!(href = %params.href, "the requested resource isn't a valid image");
                return Err(Failure::Image(
                    400,
                    "The requested resource isn't a valid image.",
                ));
            }
        };
        if upstream_type == ImageType::Svg && !images.dangerously_allow_svg {
            return Err(Failure::Image(
                400,
                "\"url\" parameter is valid but image type is not allowed",
            ));
        }
        let unchanged = || {
            entry(
                source.buffer.clone(),
                upstream_type.mime(),
                max_age,
                source.etag.clone(),
            )
        };
        if upstream_type.animatable() {
            match nextrs_image::is_animated(&source.buffer) {
                Err(_) => return Err(Failure::Internal),
                Ok(true) => return Ok(unchanged()),
                Ok(false) => {}
            }
        }
        if upstream_type.bypassed() {
            return Ok(unchanged());
        }
        let output = match ImageType::from_mime(params.mime_type.split(';').next().unwrap_or("")) {
            Some(t) => t,
            None if matches!(upstream_type, ImageType::Webp | ImageType::Avif) => ImageType::Jpeg,
            None => upstream_type,
        };
        // A revalidation whose source did not change reuses the output.
        if let Some(previous) = previous
            && previous.upstream_etag != previous.etag
            && source.etag == previous.upstream_etag
        {
            let max_age = if previous.max_age > 0 {
                previous.max_age
            } else {
                max_age
            };
            return Ok(Entry {
                content_type: output.mime().to_owned(),
                expire_at_ms: now_ms() + max_age.max(min_ttl) * 1000,
                max_age,
                ..previous.clone()
            });
        }
        match self.optimize(&source.buffer, output, params, flight).await {
            Ok(bytes) => Ok(Entry {
                etag: fetch::image_etag(&bytes),
                ..entry(bytes, output.mime(), max_age, String::new())
            }),
            Err(Failure::Overloaded) => Err(Failure::Overloaded),
            Err(_) => {
                // Next serves the source unchanged when sharp fails.
                self.counters.fallbacks.fetch_add(1, Ordering::Relaxed);
                Ok(entry(
                    source.buffer.clone(),
                    upstream_type.mime(),
                    min_ttl,
                    source.etag.clone(),
                ))
            }
        }
    }

    /// Runs the transformation on a blocking thread once one of
    /// `concurrency` slots is free and the memory it is estimated to need
    /// is reserved.
    async fn optimize(
        &self,
        input: &Bytes,
        output: ImageType,
        params: &params::Params,
        flight: &Flight,
    ) -> Result<Bytes, Failure> {
        // From the header alone; a source whose header cannot be read
        // would fail to decode as well (served unchanged).
        let estimate = match nextrs_image::estimate(input, output, params.width) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.counters
                    .transform_failures
                    .fetch_add(1, Ordering::Relaxed);
                tracing::warn!(href = %params.href, %error, "image not optimized; serving the source");
                return Err(Failure::Internal);
            }
        };
        if !self.memory.fits(estimate) {
            self.counters
                .memory_rejected
                .fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                href = %params.href,
                estimate_mib = estimate / (1024 * 1024),
                budget_mib = self.memory.total_bytes() / (1024 * 1024),
                "image not optimized: it would need more than the whole image memory budget; serving the source"
            );
            return Err(Failure::Internal);
        }
        // A free slot and free memory are taken at once; only work that
        // must wait counts against the queue. Memory first, then a slot:
        // a slot is never held idle waiting for memory.
        #[allow(unused_mut)]
        let mut estimate = estimate;
        #[cfg(feature = "negative-controls")]
        if controls::skip_memory_budget() {
            estimate = 1;
        }
        let memory = self.memory.try_reserve(estimate);
        let immediate = match &memory {
            Some(_) => self.permits.clone().try_acquire_owned().ok(),
            None => None,
        };
        let (reservation, permit) = match (memory, immediate) {
            (Some(memory), Some(permit)) => (memory, permit),
            (memory, _) => {
                if self.permits.is_closed() {
                    return Err(Failure::Internal);
                }
                if self.queued.fetch_add(1, Ordering::AcqRel) >= self.max_queued {
                    self.queued.fetch_sub(1, Ordering::AcqRel);
                    self.counters.overloaded.fetch_add(1, Ordering::Relaxed);
                    return Err(Failure::Overloaded);
                }
                // Leaves the queue however the wait ends: a cancelled
                // flight drops this future while it waits, and with it
                // whatever it had reserved so far.
                let _waiting = Queued(&self.queued);
                let memory = match memory {
                    Some(memory) => memory,
                    None => {
                        self.counters.memory_waits.fetch_add(1, Ordering::Relaxed);
                        self.memory
                            .reserve(estimate)
                            .await
                            .ok_or(Failure::Internal)?
                    }
                };
                let permit = self
                    .permits
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| Failure::Internal)?;
                (memory, permit)
            }
        };
        self.counters.transforms.fetch_add(1, Ordering::Relaxed);
        let input = input.clone();
        let cancelled = flight.cancelled.clone();
        let limits = self.limits;
        let timeout = self.timeout;
        #[allow(unused_mut)]
        let mut width = params.width;
        #[cfg(feature = "negative-controls")]
        {
            width = controls::width(width);
        }
        let quality = params.quality;
        let started = Instant::now();
        let faults = self.faults.before_transform.clone();
        let href = params.href.clone();
        let task = tokio::task::spawn_blocking(move || {
            // Held until the work ends, even if nobody waits any more (a
            // timed-out encoder still runs and still holds its memory).
            let _permit = permit;
            #[allow(unused_mut)]
            let mut _reservation = Some(reservation);
            #[cfg(feature = "negative-controls")]
            if controls::leak_reservation() {
                std::mem::forget(_reservation.take());
            }
            if let Some(fault) = faults {
                match fault(&href) {
                    ImageFault::None => {}
                    ImageFault::Sleep(d) => std::thread::sleep(d),
                    ImageFault::Panic => panic!("injected image fault"),
                    ImageFault::DecodeError => {
                        return Err(nextrs_image::Error::Decode("injected".into()));
                    }
                    ImageFault::EncodeError => {
                        return Err(nextrs_image::Error::Encode("injected".into()));
                    }
                }
            }
            nextrs_image::optimize(&nextrs_image::Job {
                input: &input,
                output,
                width,
                quality,
                limits,
                deadline: Some(started + timeout),
                cancelled: Some(&cancelled),
            })
        });
        let result = match tokio::time::timeout(timeout, task).await {
            Ok(Ok(result)) => result,
            Ok(Err(_panicked)) => Err(nextrs_image::Error::Decode(
                "transformation panicked".into(),
            )),
            Err(_) => {
                flight.cancelled.store(true, Ordering::Release);
                Err(nextrs_image::Error::TimedOut)
            }
        };
        match result {
            Ok(done) => {
                let t = &done.timings;
                self.counters
                    .decode_us
                    .fetch_add(t.decode.as_micros() as u64, Ordering::Relaxed);
                self.counters
                    .resize_us
                    .fetch_add(t.resize.as_micros() as u64, Ordering::Relaxed);
                self.counters
                    .encode_us
                    .fetch_add(t.encode.as_micros() as u64, Ordering::Relaxed);
                #[allow(unused_mut)]
                let mut bytes = done.bytes;
                #[cfg(feature = "negative-controls")]
                controls::corrupt(&mut bytes);
                Ok(Bytes::from(bytes))
            }
            Err(error) => {
                self.counters
                    .transform_failures
                    .fetch_add(1, Ordering::Relaxed);
                tracing::warn!(href = %params.href, %error, "image not optimized; serving the source");
                Err(Failure::Internal)
            }
        }
    }

    /// Counters and cache sizes for `/metrics`.
    pub(crate) fn snapshot(&self) -> Vec<(&'static str, u64)> {
        let c = &self.counters;
        let get = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let mut out = vec![
            ("requests", get(&c.requests)),
            ("transforms", get(&c.transforms)),
            ("transform_failures", get(&c.transform_failures)),
            ("fallbacks", get(&c.fallbacks)),
            ("overloaded", get(&c.overloaded)),
            ("cancelled", get(&c.cancelled)),
            ("coalesced", get(&c.coalesced)),
            ("decode_us", get(&c.decode_us)),
            ("resize_us", get(&c.resize_us)),
            ("encode_us", get(&c.encode_us)),
            ("memory_rejected", get(&c.memory_rejected)),
            ("memory_waits", get(&c.memory_waits)),
            ("memory_budget_bytes", self.memory.total_bytes()),
            ("memory_reserved_bytes", self.memory.reserved_bytes()),
            ("source_budget_bytes", self.sources.max_bytes()),
            ("source_held_bytes", self.sources.held_bytes()),
            ("source_rejected", get(&self.sources.rejected)),
            ("queued", self.queued.load(Ordering::Relaxed) as u64),
            (
                "in_flight",
                self.flights.lock().map_or(0, |f| f.len() as u64),
            ),
        ];
        if let Some(cache) = &self.cache {
            let s = &cache.stats;
            out.extend([
                ("cache_memory_hits", get(&s.memory_hits)),
                ("cache_disk_hits", get(&s.disk_hits)),
                ("cache_misses", get(&s.misses)),
                ("cache_corrupt", get(&s.corrupt)),
                ("cache_write_errors", get(&s.write_errors)),
                ("cache_memory_bytes", cache.memory_bytes()),
                ("cache_disk_bytes", cache.disk_bytes()),
            ]);
        }
        out
    }
}

async fn cancelled(flight: &Flight) {
    loop {
        let notified = flight.cancel.notified();
        if flight.cancelled.load(Ordering::Acquire) && !flight.background {
            // Wait for the last waiter's leave(), not a timeout's flag.
            if flight.waiters.load(Ordering::Acquire) == 0 {
                return;
            }
        }
        notified.await;
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// An image source request that routing sent back to `/_next/image`.
pub(crate) fn loop_detected() -> Response {
    response::text(508, "\"url\" parameter cannot be recursive")
}
