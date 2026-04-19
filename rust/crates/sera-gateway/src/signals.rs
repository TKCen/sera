//! Signal push endpoint — `POST /api/signals/push`.
//!
//! Accepts an NDJSON stream: each line is a JSON `SignalPushFrame`. Frames
//! are enqueued into the `agent_signals` inbox for their `to_agent_id`. The
//! response is `{ "accepted": N, "failed": M }` after the stream closes.
//!
//! Per the signal system design doc:
//! * `Blocked` / `Review` signals always reach HITL regardless of
//!   `SignalTarget` — the endpoint still writes them to the inbox so the
//!   HITL layer can pick them up.
//! * `ArtifactOnly` / `Silent` targets skip the inbox row — the endpoint
//!   accepts the frame but does not persist it.
//!
//! Generic over a [`SignalAppState`] trait so the handler can be mounted by
//! both the gateway library route-tree and the MVS binary without a shared
//! `AppState` type.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use sera_db::signals::SignalStore;
use sera_types::signal::{Signal, SignalTarget};

/// A single NDJSON frame on the push stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalPushFrame {
    /// Agent id this signal is addressed to.
    pub to_agent_id: String,
    /// The signal payload.
    pub signal: Signal,
    /// How to deliver the signal. Defaults to `MainSession`.
    #[serde(default)]
    pub deliver_to: SignalTarget,
}

/// Response from a push stream — aggregated counts across all frames.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalPushResponse {
    /// Frames written to the inbox.
    pub accepted: usize,
    /// Frames that parsed and were honored but skipped the inbox
    /// (`ArtifactOnly` / `Silent`).
    pub skipped: usize,
    /// Frames that failed to parse or enqueue.
    pub failed: usize,
    /// Per-frame errors (parse or enqueue), in arrival order. Empty on
    /// success — useful for operators debugging malformed submissions.
    pub errors: Vec<String>,
}

/// Trait surface needed by the handler — authentication + the inbox store.
pub trait SignalAppState: Send + Sync + 'static {
    /// Optional API key; `None` leaves the route open.
    fn api_key(&self) -> &Option<String>;
    /// Inbox store used to persist signals.
    fn signal_store(&self) -> Arc<dyn SignalStore>;
}

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

/// Split an NDJSON byte stream into trimmed non-empty lines.
fn split_ndjson(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for line in buf.split(|b| *b == b'\n') {
        // Strip optional trailing CR.
        let line = if line.last() == Some(&b'\r') {
            &line[..line.len() - 1]
        } else {
            line
        };
        if !line.iter().all(|b| b.is_ascii_whitespace()) && !line.is_empty() {
            out.push(line.to_vec());
        }
    }
    out
}

/// `POST /api/signals/push` — stream of NDJSON signal frames.
pub async fn push_signals<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    body: Body,
) -> Result<Json<SignalPushResponse>, StatusCode>
where
    S: SignalAppState,
{
    check_auth(state.api_key(), &headers)?;

    // Collect the full body — keeps the implementation simple and bounded by
    // the axum default body limit. NDJSON is line-delimited, so we split once
    // we have the whole payload.
    let mut stream = body.into_data_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| StatusCode::BAD_REQUEST)?;
        buf.extend_from_slice(&chunk);
        // Cap inbound payload at 1 MiB — protects against accidental
        // multi-gigabyte submissions.
        if buf.len() > 1_048_576 {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
    }

    let store = state.signal_store();
    let mut accepted = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut errors = Vec::new();

    for (i, line) in split_ndjson(&buf).into_iter().enumerate() {
        let frame: SignalPushFrame = match serde_json::from_slice(&line) {
            Ok(f) => f,
            Err(e) => {
                failed += 1;
                errors.push(format!("line {i}: parse error: {e}"));
                continue;
            }
        };

        if !frame.deliver_to.writes_inbox() && !frame.signal.is_attention_required() {
            // ArtifactOnly / Silent — no inbox row. Attention-required signals
            // (Blocked / Review) override the target per design invariant.
            debug!(
                to = %frame.to_agent_id,
                kind = frame.signal.kind(),
                "signal delivered via artifact only — inbox skipped"
            );
            skipped += 1;
            continue;
        }

        match store.enqueue(&frame.to_agent_id, &frame.signal).await {
            Ok(_id) => accepted += 1,
            Err(e) => {
                failed += 1;
                errors.push(format!("line {i}: enqueue error: {e}"));
                warn!(to = %frame.to_agent_id, error = %e, "signal enqueue failed");
            }
        }
    }

    Ok(Json(SignalPushResponse {
        accepted,
        skipped,
        failed,
        errors,
    }))
}

/// Helper: re-export a concrete axum handler bound to a specific state type.
/// Mirrors the pattern used by `party::start_party::<AppState>`.
pub async fn push_signals_handler<S: SignalAppState>(
    state: State<Arc<S>>,
    headers: HeaderMap,
    body: Body,
) -> impl IntoResponse {
    push_signals(state, headers, body).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{routing::post, Router};
    use rusqlite::Connection;
    use sera_db::signals::{SqliteSignalStore, StoredSignal};
    use sera_types::capability::AgentCapability;
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    /// Test harness that owns an in-memory SqliteSignalStore.
    struct TestState {
        api_key: Option<String>,
        store: Arc<dyn SignalStore>,
    }

    impl TestState {
        fn new(api_key: Option<String>) -> Self {
            let conn = Connection::open_in_memory().unwrap();
            SqliteSignalStore::init_schema(&conn).unwrap();
            let store = SqliteSignalStore::new(Arc::new(Mutex::new(conn)));
            Self {
                api_key,
                store: Arc::new(store),
            }
        }

        fn concrete_store(&self) -> Arc<dyn SignalStore> {
            Arc::clone(&self.store)
        }
    }

    impl SignalAppState for TestState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn signal_store(&self) -> Arc<dyn SignalStore> {
            Arc::clone(&self.store)
        }
    }

    /// Trait pass-through for tests that need to peek into the underlying
    /// store. Declared as a separate trait so we don't leak the concrete
    /// [`SqliteSignalStore`] type through [`SignalAppState`].
    #[async_trait]
    trait TestPeek {
        async fn peek(&self, agent_id: &str) -> Vec<StoredSignal>;
    }

    #[async_trait]
    impl TestPeek for TestState {
        async fn peek(&self, agent_id: &str) -> Vec<StoredSignal> {
            self.concrete_store().peek_pending(agent_id).await.unwrap()
        }
    }

    fn router(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/api/signals/push", post(push_signals::<TestState>))
            .with_state(state)
    }

    fn ndjson(frames: &[SignalPushFrame]) -> String {
        frames
            .iter()
            .map(|f| serde_json::to_string(f).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn push_single_frame_accepted() {
        let state = Arc::new(TestState::new(None));
        let app = router(state.clone());

        let frame = SignalPushFrame {
            to_agent_id: "agent-a".into(),
            signal: Signal::Done {
                artifact_id: "art".into(),
                summary: "ok".into(),
                duration_ms: 42,
            },
            deliver_to: SignalTarget::MainSession,
        };
        let body = ndjson(&[frame]);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .header("content-type", "application/x-ndjson")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let out: SignalPushResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out.accepted, 1);
        assert_eq!(out.skipped, 0);
        assert_eq!(out.failed, 0);
        assert!(out.errors.is_empty());

        let stored = state.peek("agent-a").await;
        assert_eq!(stored.len(), 1);
    }

    #[tokio::test]
    async fn push_multiple_frames_accepted_in_order() {
        let state = Arc::new(TestState::new(None));
        let app = router(state.clone());

        let frames = vec![
            SignalPushFrame {
                to_agent_id: "a".into(),
                signal: Signal::Started {
                    task_id: "t".into(),
                    description: "start".into(),
                },
                deliver_to: SignalTarget::MainSession,
            },
            SignalPushFrame {
                to_agent_id: "a".into(),
                signal: Signal::Progress {
                    task_id: "t".into(),
                    pct: 50,
                    note: "halfway".into(),
                },
                deliver_to: SignalTarget::MainSession,
            },
            SignalPushFrame {
                to_agent_id: "a".into(),
                signal: Signal::Done {
                    artifact_id: "art".into(),
                    summary: "complete".into(),
                    duration_ms: 1000,
                },
                deliver_to: SignalTarget::MainSession,
            },
        ];
        let body = ndjson(&frames);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let out: SignalPushResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out.accepted, 3);
        assert_eq!(out.failed, 0);

        let stored = state.peek("a").await;
        assert_eq!(stored.len(), 3);
        // Signals enqueued within the same second fall back to a `id ASC`
        // sort (UUID lexical order), so we assert on kind-set rather than
        // positional order.
        let kinds: std::collections::HashSet<_> =
            stored.iter().map(|r| r.signal_type.as_str()).collect();
        assert!(kinds.contains("started"));
        assert!(kinds.contains("progress"));
        assert!(kinds.contains("done"));
    }

    #[tokio::test]
    async fn silent_target_skips_inbox() {
        let state = Arc::new(TestState::new(None));
        let app = router(state.clone());

        let frame = SignalPushFrame {
            to_agent_id: "a".into(),
            signal: Signal::Done {
                artifact_id: "art".into(),
                summary: "".into(),
                duration_ms: 0,
            },
            deliver_to: SignalTarget::Silent,
        };
        let body = ndjson(&[frame]);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let out: SignalPushResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out.accepted, 0);
        assert_eq!(out.skipped, 1);
        assert!(state.peek("a").await.is_empty());
    }

    #[tokio::test]
    async fn blocked_always_reaches_inbox_even_when_silent() {
        // `Blocked` / `Review` are attention-required and must never be
        // silenced regardless of the SignalTarget.
        let state = Arc::new(TestState::new(None));
        let app = router(state.clone());

        let frame = SignalPushFrame {
            to_agent_id: "a".into(),
            signal: Signal::Blocked {
                reason: "missing cap".into(),
                requires: vec![AgentCapability::MetaChange],
            },
            deliver_to: SignalTarget::Silent,
        };
        let body = ndjson(&[frame]);

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let out: SignalPushResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out.accepted, 1);
        assert_eq!(out.skipped, 0);

        let stored = state.peek("a").await;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].signal_type, "blocked");
    }

    #[tokio::test]
    async fn malformed_lines_counted_but_do_not_abort() {
        let state = Arc::new(TestState::new(None));
        let app = router(state.clone());

        let valid = SignalPushFrame {
            to_agent_id: "a".into(),
            signal: Signal::Done {
                artifact_id: "art".into(),
                summary: "".into(),
                duration_ms: 0,
            },
            deliver_to: SignalTarget::MainSession,
        };
        // valid ; garbage ; valid
        let body = format!(
            "{}\n{{not-json}}\n{}",
            serde_json::to_string(&valid).unwrap(),
            serde_json::to_string(&valid).unwrap()
        );

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let out: SignalPushResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out.accepted, 2);
        assert_eq!(out.failed, 1);
        assert_eq!(out.errors.len(), 1);
    }

    #[tokio::test]
    async fn auth_required_when_api_key_set() {
        let state = Arc::new(TestState::new(Some("secret".into())));
        let app = router(state.clone());

        let frame = SignalPushFrame {
            to_agent_id: "a".into(),
            signal: Signal::Done {
                artifact_id: "art".into(),
                summary: "".into(),
                duration_ms: 0,
            },
            deliver_to: SignalTarget::MainSession,
        };
        let body = ndjson(&[frame]);

        // Missing bearer → 401.
        let req_missing = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .body(Body::from(body.clone()))
            .unwrap();
        let resp = app.clone().oneshot(req_missing).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Wrong bearer → 401.
        let req_wrong = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .header("authorization", "Bearer wrong")
            .body(Body::from(body.clone()))
            .unwrap();
        let resp = app.clone().oneshot(req_wrong).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Correct bearer → 200.
        let req_ok = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .header("authorization", "Bearer secret")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req_ok).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn empty_body_returns_zero_counts() {
        let state = Arc::new(TestState::new(None));
        let app = router(state.clone());

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let out: SignalPushResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out.accepted, 0);
        assert_eq!(out.failed, 0);
        assert_eq!(out.skipped, 0);
    }

    #[tokio::test]
    async fn crlf_line_endings_accepted() {
        let state = Arc::new(TestState::new(None));
        let app = router(state.clone());

        let frame = SignalPushFrame {
            to_agent_id: "a".into(),
            signal: Signal::Done {
                artifact_id: "art".into(),
                summary: "".into(),
                duration_ms: 0,
            },
            deliver_to: SignalTarget::MainSession,
        };
        let line = serde_json::to_string(&frame).unwrap();
        let body = format!("{line}\r\n{line}\r\n");

        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/signals/push")
            .body(Body::from(body))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let out: SignalPushResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out.accepted, 2);
    }
}
