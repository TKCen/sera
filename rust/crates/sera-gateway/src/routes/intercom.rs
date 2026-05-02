//! Intercom HTTP routes (sera-nd4v).
//!
//! Routes:
//!   POST /api/intercom/publish            — publish a message to a topic
//!   GET  /api/intercom/subscribe/{topic}  — SSE stream of messages on `topic`
//!   GET  /api/intercom/topics             — list known topic names
//!
//! Backed by an in-process [`IntercomBroker`] (Tier-1, local-first). The
//! Centrifugo path remains available for enterprise deployments behind a
//! separate transport — types live in `sera_types::intercom`.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, RwLock};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

use sera_types::intercom::{IntercomMessage, MessageSource};

// ── In-process broker ────────────────────────────────────────────────────────

/// Default broadcast channel capacity per topic. Lossy beyond this point.
const DEFAULT_TOPIC_CAPACITY: usize = 256;

/// In-process pub/sub broker keyed by topic (channel) name.
///
/// Each topic backs one [`tokio::sync::broadcast::Sender`]; subscribers get
/// an independent receiver. Lossy on slow consumers — matches the legacy
/// Centrifugo semantics (pub/sub is best-effort; durability is the caller's
/// problem). The Centrifugo-backed transport remains available as
/// enterprise-only infrastructure (project memory: Centrifugo is optional).
/// Local setups use this in-process broker so Scenario 4.5 ("agent A
/// publishes, agent B receives") works without external services.
#[derive(Debug, Default)]
pub struct IntercomBroker {
    topics: RwLock<HashMap<String, broadcast::Sender<IntercomMessage>>>,
}

impl IntercomBroker {
    /// Build a fresh broker with no topics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish `msg` to its `channel`. Returns the number of receivers the
    /// message was delivered to. A topic with no subscribers returns 0 and
    /// the message is dropped.
    pub fn publish(&self, msg: IntercomMessage) -> usize {
        let topic = msg.channel.clone();
        let sender = {
            let topics = self.topics.read().expect("intercom topics rwlock poisoned");
            topics.get(&topic).cloned()
        };
        match sender {
            Some(tx) => tx.send(msg).unwrap_or(0),
            None => 0,
        }
    }

    /// Subscribe to `topic`. Lazily creates the broadcast channel if no
    /// publisher or other subscriber has touched it yet.
    pub fn subscribe(&self, topic: &str) -> broadcast::Receiver<IntercomMessage> {
        {
            let topics = self.topics.read().expect("intercom topics rwlock poisoned");
            if let Some(tx) = topics.get(topic) {
                return tx.subscribe();
            }
        }
        let mut topics = self.topics.write().expect("intercom topics rwlock poisoned");
        let tx = topics
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(DEFAULT_TOPIC_CAPACITY).0);
        tx.subscribe()
    }

    /// List currently known topic names. A topic appears here once it has
    /// had at least one subscribe call.
    pub fn topics(&self) -> Vec<String> {
        let topics = self.topics.read().expect("intercom topics rwlock poisoned");
        let mut names: Vec<String> = topics.keys().cloned().collect();
        names.sort();
        names
    }
}

/// Shared handle type used by [`AppState`] and route handlers.
pub type SharedIntercomBroker = Arc<IntercomBroker>;

// ── AppState abstraction ─────────────────────────────────────────────────────

/// Abstraction over AppState for intercom handlers.
pub trait IntercomAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn intercom_broker(&self) -> SharedIntercomBroker;
}

// ── Auth ─────────────────────────────────────────────────────────────────────

fn check_auth(api_key: &Option<String>, headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = match api_key {
        None => return Ok(()),
        Some(k) => k,
    };
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match provided {
        Some(k) if k == expected => Ok(()),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// ── Request / response shapes ────────────────────────────────────────────────

/// Request body for `POST /api/intercom/publish`.
#[derive(Debug, Deserialize)]
pub struct PublishRequest {
    pub channel: String,
    pub data: serde_json::Value,
    #[serde(default)]
    pub source: Option<MessageSource>,
}

/// Response body for `POST /api/intercom/publish`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PublishResponse {
    pub channel: String,
    /// Number of subscribers the message was delivered to.
    pub delivered: usize,
}

/// Response body for `GET /api/intercom/topics`.
#[derive(Debug, Serialize, Deserialize)]
pub struct TopicsResponse {
    pub topics: Vec<String>,
    pub count: usize,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// POST /api/intercom/publish
pub async fn publish<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Json(body): Json<PublishRequest>,
) -> Result<Json<PublishResponse>, StatusCode>
where
    S: IntercomAppState,
{
    check_auth(state.api_key(), &headers)?;
    if body.channel.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let msg = IntercomMessage {
        channel: body.channel.clone(),
        data: body.data,
        source: body.source,
    };
    let delivered = state.intercom_broker().publish(msg);
    Ok(Json(PublishResponse {
        channel: body.channel,
        delivered,
    }))
}

/// GET /api/intercom/subscribe/{topic} — SSE stream of `IntercomMessage`s.
///
/// Each SSE event uses the event name `intercom` and a JSON-serialized
/// [`IntercomMessage`] as data. Lossy: if the subscriber falls behind the
/// broadcast channel capacity, lagged messages are dropped silently
/// (matches Centrifugo best-effort semantics).
pub async fn subscribe<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(topic): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode>
where
    S: IntercomAppState,
{
    check_auth(state.api_key(), &headers)?;
    if topic.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let rx = state.intercom_broker().subscribe(&topic);

    let stream = BroadcastStream::new(rx).filter_map(|res| match res {
        Ok(msg) => {
            let data = serde_json::to_string(&msg).unwrap_or_else(|_| "{}".into());
            Some(Ok::<Event, Infallible>(
                Event::default().event("intercom").data(data),
            ))
        }
        // Drop lagged messages silently — matches lossy pub/sub semantics.
        Err(_) => None,
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// GET /api/intercom/topics
pub async fn list_topics<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
) -> Result<Json<TopicsResponse>, StatusCode>
where
    S: IntercomAppState,
{
    check_auth(state.api_key(), &headers)?;
    let topics = state.intercom_broker().topics();
    let count = topics.len();
    Ok(Json(TopicsResponse { topics, count }))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
    };
    use serde_json::json;
    use tower::ServiceExt;

    fn msg(topic: &str, body: &str) -> IntercomMessage {
        IntercomMessage {
            channel: topic.to_string(),
            data: json!({ "body": body }),
            source: None,
        }
    }

    // ── Broker unit tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn broker_publish_with_no_subscribers_is_zero() {
        let broker = IntercomBroker::new();
        assert_eq!(broker.publish(msg("alpha", "hi")), 0);
    }

    #[tokio::test]
    async fn broker_subscribe_then_publish_delivers_one() {
        let broker = IntercomBroker::new();
        let mut rx = broker.subscribe("alpha");
        assert_eq!(broker.publish(msg("alpha", "hi")), 1);
        let received = rx.recv().await.unwrap();
        assert_eq!(received.channel, "alpha");
        assert_eq!(received.data["body"], "hi");
    }

    #[tokio::test]
    async fn broker_topics_isolated() {
        let broker = IntercomBroker::new();
        let mut rx_a = broker.subscribe("alpha");
        let mut rx_b = broker.subscribe("beta");
        broker.publish(msg("alpha", "for-a"));
        broker.publish(msg("beta", "for-b"));
        assert_eq!(rx_a.recv().await.unwrap().data["body"], "for-a");
        assert_eq!(rx_b.recv().await.unwrap().data["body"], "for-b");
    }

    // ── HTTP handler tests ───────────────────────────────────────────────────

    struct TestState {
        api_key: Option<String>,
        broker: SharedIntercomBroker,
    }

    impl TestState {
        fn new(key: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                api_key: key.map(|k| k.to_owned()),
                broker: Arc::new(IntercomBroker::new()),
            })
        }
    }

    impl IntercomAppState for TestState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn intercom_broker(&self) -> SharedIntercomBroker {
            Arc::clone(&self.broker)
        }
    }

    fn router(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/api/intercom/publish", post(publish::<TestState>))
            .route(
                "/api/intercom/subscribe/{topic}",
                get(subscribe::<TestState>),
            )
            .route("/api/intercom/topics", get(list_topics::<TestState>))
            .with_state(state)
    }

    #[tokio::test]
    async fn http_publish_without_subscribers_returns_zero() {
        let app = router(TestState::new(None));
        let body = serde_json::json!({
            "channel": "alpha",
            "data": { "body": "hi" }
        });
        let resp = app
            .oneshot(
                Request::post("/api/intercom/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: PublishResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(view.channel, "alpha");
        assert_eq!(view.delivered, 0);
    }

    #[tokio::test]
    async fn http_publish_subscribe_round_trip() {
        let state = TestState::new(None);
        let mut rx = state.broker.subscribe("alpha");

        let app = router(Arc::clone(&state));
        let body = serde_json::json!({
            "channel": "alpha",
            "data": { "body": "scenario-4.5" },
            "source": { "agent_id": "agent-a", "agent_name": "A" }
        });
        let resp = app
            .oneshot(
                Request::post("/api/intercom/publish")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: PublishResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(view.delivered, 1);

        let received = rx.recv().await.unwrap();
        assert_eq!(received.channel, "alpha");
        assert_eq!(received.data["body"], "scenario-4.5");
    }
}
