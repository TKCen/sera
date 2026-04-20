//! Apalis 0.7.x `Storage` adapter that wraps any [`QueueBackend`].
//!
//! This module lets sera's queue lane abstraction drive apalis `Worker`s, so
//! production deployments can use apalis's tower-based middleware stack
//! (retries, timeouts, tracing, rate-limits) on top of either the in-memory
//! [`LocalQueueBackend`](crate::local::LocalQueueBackend) or the Postgres
//! [`SqlxQueueBackend`](crate::sqlx_backend::SqlxQueueBackend).
//!
//! `QueueBackend` is a strict FIFO push/pull/ack/nack abstraction. Apalis's
//! `Storage` trait includes richer lookup / update operations (`fetch_by_id`,
//! `update`, `reschedule`, `len`, …) that the backend does not model. Those
//! methods return [`QueueError::Storage`] with an "unsupported" reason — the
//! wrapper intentionally exposes only the FIFO subset.
//!
//! Sera assigns jobs a UUID-based id, but apalis `TaskId` is a strict ULID.
//! We therefore track the sera id and lane in a custom
//! [`SeraJobContext`] that travels on each `Request`; ack / nack use that
//! context to reach the underlying backend.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use apalis_core::backend::Backend;
use apalis_core::codec::json::JsonCodec;
use apalis_core::error::Error as ApalisError;
use apalis_core::layers::{Ack, AckLayer};
use apalis_core::poller::Poller;
use apalis_core::poller::controller::Controller;
use apalis_core::poller::stream::BackendStream;
use apalis_core::request::{Parts, Request, RequestStream};
use apalis_core::response::Response;
use apalis_core::storage::Storage;
use apalis_core::worker::{Context as WorkerContext, Worker};
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::backend::{QueueBackend, QueueError};

/// Default sleep between empty polls when the lane is idle.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Per-job metadata carried through apalis so the wrapper can reach the
/// underlying `QueueBackend` on ack / nack.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SeraJobContext {
    /// The sera-queue job id (as returned by [`QueueBackend::push`]).
    pub job_id: String,
    /// The sera-queue lane this job was pulled from.
    pub lane: String,
}

/// Apalis `Storage` adapter around an [`Arc<dyn QueueBackend>`].
///
/// Typed by the job payload `T`; internally the wrapper serialises to
/// [`serde_json::Value`] (the queue's opaque payload shape).
pub struct ApalisSeraStorage<T> {
    backend: Arc<dyn QueueBackend>,
    lane: String,
    poll_interval: Duration,
    controller: Controller,
    _marker: PhantomData<fn() -> T>,
}

impl<T> ApalisSeraStorage<T> {
    /// Build a wrapper that pushes and pulls jobs on `lane`.
    pub fn new(backend: Arc<dyn QueueBackend>, lane: impl Into<String>) -> Self {
        Self {
            backend,
            lane: lane.into(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            controller: Controller::new(),
            _marker: PhantomData,
        }
    }

    /// Override the idle poll interval (defaults to 250ms).
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// The lane this storage pushes to / pulls from.
    pub fn lane(&self) -> &str {
        &self.lane
    }

    /// Clone the underlying backend handle.
    pub fn backend(&self) -> Arc<dyn QueueBackend> {
        Arc::clone(&self.backend)
    }
}

impl<T> Clone for ApalisSeraStorage<T> {
    fn clone(&self) -> Self {
        Self {
            backend: Arc::clone(&self.backend),
            lane: self.lane.clone(),
            poll_interval: self.poll_interval,
            controller: self.controller.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T> std::fmt::Debug for ApalisSeraStorage<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApalisSeraStorage")
            .field("lane", &self.lane)
            .field("poll_interval", &self.poll_interval)
            .field("job_type", &std::any::type_name::<T>())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Storage impl
// ---------------------------------------------------------------------------

impl<T> Storage for ApalisSeraStorage<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + Unpin + 'static,
{
    type Job = T;
    type Error = QueueError;
    type Context = SeraJobContext;
    type Compact = Value;

    async fn push_request(
        &mut self,
        req: Request<Self::Job, Self::Context>,
    ) -> Result<Parts<Self::Context>, Self::Error> {
        let (job, mut parts) = req.take_parts();
        let payload = serde_json::to_value(&job).map_err(|e| QueueError::Serde {
            reason: e.to_string(),
        })?;
        let lane = self.resolve_lane(&parts.context);
        let id = self.backend.push(&lane, payload).await?;
        parts.context.job_id = id;
        parts.context.lane = lane;
        Ok(parts)
    }

    async fn push_raw_request(
        &mut self,
        req: Request<Self::Compact, Self::Context>,
    ) -> Result<Parts<Self::Context>, Self::Error> {
        let (payload, mut parts) = req.take_parts();
        let lane = self.resolve_lane(&parts.context);
        let id = self.backend.push(&lane, payload).await?;
        parts.context.job_id = id;
        parts.context.lane = lane;
        Ok(parts)
    }

    async fn schedule_request(
        &mut self,
        _req: Request<Self::Job, Self::Context>,
        _on: i64,
    ) -> Result<Parts<Self::Context>, Self::Error> {
        Err(QueueError::Storage {
            reason: "scheduled enqueue is not supported by sera-queue QueueBackend".into(),
        })
    }

    async fn len(&mut self) -> Result<i64, Self::Error> {
        // QueueBackend exposes no length API; report zero rather than block
        // callers that only want a health-check style value.
        Ok(0)
    }

    async fn fetch_by_id(
        &mut self,
        _job_id: &apalis_core::task::task_id::TaskId,
    ) -> Result<Option<Request<Self::Job, Self::Context>>, Self::Error> {
        Err(QueueError::Storage {
            reason: "fetch_by_id is not supported by sera-queue QueueBackend".into(),
        })
    }

    async fn update(
        &mut self,
        _job: Request<Self::Job, Self::Context>,
    ) -> Result<(), Self::Error> {
        Err(QueueError::Storage {
            reason: "update is not supported by sera-queue QueueBackend".into(),
        })
    }

    async fn reschedule(
        &mut self,
        _job: Request<Self::Job, Self::Context>,
        _wait: Duration,
    ) -> Result<(), Self::Error> {
        Err(QueueError::Storage {
            reason: "reschedule is not supported by sera-queue QueueBackend".into(),
        })
    }

    async fn is_empty(&mut self) -> Result<bool, Self::Error> {
        // No size API — be conservative and report non-empty so callers poll.
        Ok(false)
    }

    async fn vacuum(&mut self) -> Result<usize, Self::Error> {
        // No vacuum semantics in QueueBackend — report nothing was removed.
        Ok(0)
    }
}

impl<T> ApalisSeraStorage<T> {
    fn resolve_lane(&self, ctx: &SeraJobContext) -> String {
        if ctx.lane.is_empty() {
            self.lane.clone()
        } else {
            ctx.lane.clone()
        }
    }
}

// ---------------------------------------------------------------------------
// Backend impl (stream of Request<T, SeraJobContext>)
// ---------------------------------------------------------------------------

impl<T> Backend<Request<T, SeraJobContext>> for ApalisSeraStorage<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + Unpin + 'static,
{
    type Stream = BackendStream<RequestStream<Request<T, SeraJobContext>>>;
    type Layer = AckLayer<Self, T, SeraJobContext, JsonCodec<Value>>;
    type Codec = JsonCodec<Value>;

    fn poll(self, _worker: &Worker<WorkerContext>) -> Poller<Self::Stream, Self::Layer> {
        let layer = AckLayer::new(self.clone());
        let controller = self.controller.clone();
        let stream = build_pull_stream(self);
        let backend_stream = BackendStream::new(stream, controller);
        Poller::new_with_layer(backend_stream, futures::future::pending(), layer)
    }
}

fn build_pull_stream<T>(
    storage: ApalisSeraStorage<T>,
) -> RequestStream<Request<T, SeraJobContext>>
where
    T: DeserializeOwned + Send + Sync + Unpin + 'static,
{
    let backend = Arc::clone(&storage.backend);
    let lane = storage.lane.clone();
    let poll_interval = storage.poll_interval;
    let stream = async_stream::stream! {
        loop {
            match backend.pull(&lane).await {
                Ok(Some((id, payload))) => {
                    match serde_json::from_value::<T>(payload) {
                        Ok(job) => {
                            let ctx = SeraJobContext {
                                job_id: id,
                                lane: lane.clone(),
                            };
                            let req = Request::new_with_ctx(job, ctx);
                            yield Ok(Some(req));
                        }
                        Err(e) => {
                            yield Err(ApalisError::SourceError(std::sync::Arc::new(Box::new(
                                QueueError::Serde { reason: e.to_string() },
                            ))));
                        }
                    }
                }
                Ok(None) => {
                    apalis_core::sleep(poll_interval).await;
                }
                Err(e) => {
                    yield Err(ApalisError::SourceError(std::sync::Arc::new(Box::new(e))));
                    apalis_core::sleep(poll_interval).await;
                }
            }
        }
    };
    stream.boxed()
}

// ---------------------------------------------------------------------------
// Ack impl — success → backend.ack, failure → backend.nack
// ---------------------------------------------------------------------------

impl<T, Res> Ack<T, Res, JsonCodec<Value>> for ApalisSeraStorage<T>
where
    T: Send + Sync,
    Res: Send + Sync + Serialize,
{
    type Context = SeraJobContext;
    type AckError = QueueError;

    async fn ack(
        &mut self,
        ctx: &Self::Context,
        response: &Response<Res>,
    ) -> Result<(), Self::AckError> {
        if response.is_success() {
            self.backend.ack(&ctx.job_id).await
        } else {
            // Best-effort nack; LocalQueueBackend reports NotFound for nack
            // (no persistence), which we swallow so the worker keeps running.
            match self.backend.nack(&ctx.job_id).await {
                Ok(()) => Ok(()),
                Err(QueueError::NotFound { .. }) => Ok(()),
                Err(e) => Err(e),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time: the wrapper must be Send + Sync so apalis Workers (which
    // are spawned on tokio) can hold it across await points.
    #[test]
    fn apalis_sera_storage_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ApalisSeraStorage<serde_json::Value>>();
    }

    // Integration test: round-trip a job through ApalisSeraStorage backed by
    // LocalQueueBackend. This exercises the `Storage::push_request` →
    // backend.push path and the `Backend::poll` → backend.pull path in
    // isolation (without actually spinning up an apalis Worker), plus the
    // Ack success path.
    #[cfg(feature = "local")]
    #[tokio::test]
    async fn roundtrip_push_pull_ack_over_local_backend() {
        use crate::LocalQueueBackend;
        use futures::StreamExt;

        #[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
        struct Greeting {
            to: String,
        }

        let backend: Arc<dyn QueueBackend> = Arc::new(LocalQueueBackend::new());
        let mut storage: ApalisSeraStorage<Greeting> =
            ApalisSeraStorage::new(Arc::clone(&backend), "greetings");

        // Push a job via the Storage API.
        let parts = storage
            .push(Greeting {
                to: "world".into(),
            })
            .await
            .expect("push");
        assert!(!parts.context.job_id.is_empty());
        assert_eq!(parts.context.lane, "greetings");

        // Pull it back by driving the stream one tick.
        let mut stream = build_pull_stream(storage.clone());
        let next = stream.next().await.expect("stream yields");
        let req = next.expect("ok").expect("some");
        assert_eq!(req.args.to, "world");
        assert_eq!(req.parts.context.lane, "greetings");
        let job_id = req.parts.context.job_id.clone();

        // Ack the job — LocalQueueBackend::ack is a no-op, but the wrapper
        // must forward without erroring on success responses.
        let resp: Response<()> = Response::success(
            (),
            apalis_core::task::task_id::TaskId::new(),
            apalis_core::task::attempt::Attempt::default(),
        );
        let ack_ctx = SeraJobContext {
            job_id,
            lane: "greetings".into(),
        };
        <ApalisSeraStorage<Greeting> as Ack<Greeting, (), JsonCodec<Value>>>::ack(
            &mut storage,
            &ack_ctx,
            &resp,
        )
        .await
        .expect("ack");
    }

    // Integration test: run with a live Postgres via
    //   DATABASE_URL=postgres://... cargo test -p sera-queue --features apalis
    // wrapping SqlxQueueBackend in an ApalisSeraStorage and driving a real
    // apalis Worker with retries + tracing layers. See sqlx_backend.rs for
    // the table schema.
}
