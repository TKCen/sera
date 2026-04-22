//! Party mode HTTP handler — bead `sera-8d1.2` (GH#145).
//!
//! Mounts `POST /api/circles/{id}/party`. The handler is intentionally
//! generic over a [`PartyAppState`] trait rather than tied to the gateway
//! binary's DB-backed `AppState`, so it compiles as part of `sera_gateway`
//! (the library) and unit tests can exercise the full axum flow without
//! standing up Postgres.
//!
//! See `routes::circles` for the DB-backed circle CRUD handlers.
//!
//! ## Production wiring (follow-up)
//!
//! Binding [`PartyAppState::resolve_party_members`] to real LLM-backed
//! [`sera_workflow::coordination::PartyMember`] implementations is a
//! follow-up bead — the trait exists so the API surface can land ahead of
//! the runtime seam.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;

use sera_types::circle::{PartyConfig, PartyOutcome};
use sera_workflow::coordination::{
    ConcurrencyPolicy, CoordinationError, CoordinationPolicy, Coordinator, FirstSuccess,
    PartyMember,
};

/// Request body for `POST /api/circles/{id}/party`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartPartyRequest {
    /// Prompt broadcast to every party member for each round.
    pub prompt: String,
    /// Optional override for the circle's default party config. When absent,
    /// a default [`PartyConfig`] is constructed from `synthesizer` below.
    #[serde(default)]
    pub config_override: Option<PartyConfig>,
    /// Fallback synthesizer id used when no `config_override` is supplied.
    /// Required in that case — handler responds 400 if both are missing.
    #[serde(default)]
    pub synthesizer: Option<String>,
}

/// Abstraction over the gateway's `AppState` for the party-mode handler,
/// allowing unit-level tests to stand up a router without the full gateway
/// state.
///
/// Implementations return the handler's dependencies: an optional API key
/// (for `Authorization: Bearer` gating) and a resolver that maps a circle
/// name/id to a list of [`PartyMember`] implementations. A `None` from the
/// resolver signals that the circle does not exist (handler replies 404).
pub trait PartyAppState: Send + Sync + 'static {
    /// Optional API key for `Authorization: Bearer <key>` checks. `None`
    /// leaves the route open (autonomous mode).
    fn api_key(&self) -> &Option<String>;

    /// Resolve the party members for a circle. Implementations own the
    /// member collection so gateway-side registration, auth, and LLM wiring
    /// stay out of the handler. Returning `None` signals that the circle
    /// does not exist (handler replies 404).
    fn resolve_party_members(&self, circle_id: &str) -> Option<Vec<Arc<dyn PartyMember>>>;
}

fn check_party_auth(api_key: &Option<String>, headers: &HeaderMap) -> Result<(), StatusCode> {
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

/// `POST /api/circles/{id}/party` — kick off a Party mode discussion.
///
/// Generic over a [`PartyAppState`] so tests can supply a lightweight
/// resolver without standing up the full gateway `AppState`.
pub async fn start_party<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<StartPartyRequest>,
) -> Result<Json<PartyOutcome>, StatusCode>
where
    S: PartyAppState,
{
    check_party_auth(state.api_key(), &headers)?;

    let members = state
        .resolve_party_members(&id)
        .ok_or(StatusCode::NOT_FOUND)?;
    if members.is_empty() {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let config = match body.config_override {
        Some(cfg) => cfg,
        None => {
            let synth = body.synthesizer.ok_or(StatusCode::BAD_REQUEST)?;
            PartyConfig::new(synth)
        }
    };

    let coord = Coordinator::new(
        CoordinationPolicy::Party { config },
        ConcurrencyPolicy::Sequential,
        Box::new(FirstSuccess),
    );

    let refs: Vec<&dyn PartyMember> = members.iter().map(|m| m.as_ref()).collect();

    let outcome = coord.run_party(&body.prompt, &refs).map_err(|e| {
        tracing::warn!(error = %e, "party run failed");
        match e {
            CoordinationError::NoParticipants | CoordinationError::PartyConfig(_) => {
                StatusCode::BAD_REQUEST
            }
            CoordinationError::TerminatedByCondition => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    })?;

    Ok(Json(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::post,
    };
    use sera_types::circle::BlackboardEntry;
    use std::collections::HashMap;
    use tower::ServiceExt;

    /// Echo member that responds with `"echo:<id>"` regardless of input.
    struct EchoMember(String);
    impl PartyMember for EchoMember {
        fn id(&self) -> &str {
            &self.0
        }
        fn respond(&self, _prompt: &str, _transcript: &[BlackboardEntry]) -> String {
            format!("echo:{}", self.0)
        }
    }

    struct TestState {
        api_key: Option<String>,
        circles: HashMap<String, Vec<Arc<dyn PartyMember>>>,
    }

    impl TestState {
        fn new(api_key: Option<&str>) -> Arc<Self> {
            let mut circles = HashMap::new();
            let members: Vec<Arc<dyn PartyMember>> = vec![
                Arc::new(EchoMember("alice".into())),
                Arc::new(EchoMember("bob".into())),
                Arc::new(EchoMember("lead".into())),
            ];
            circles.insert("team-eng".to_string(), members);
            Arc::new(Self {
                api_key: api_key.map(|k| k.to_string()),
                circles,
            })
        }
    }

    impl PartyAppState for TestState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn resolve_party_members(&self, circle_id: &str) -> Option<Vec<Arc<dyn PartyMember>>> {
            self.circles.get(circle_id).cloned()
        }
    }

    fn test_router(state: Arc<TestState>) -> Router {
        Router::new()
            .route("/api/circles/{id}/party", post(start_party::<TestState>))
            .with_state(state)
    }

    #[tokio::test]
    async fn start_party_happy_path_returns_outcome() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "prompt": "Plan the launch",
            "synthesizer": "lead"
        });
        let req = Request::post("/api/circles/team-eng/party")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let outcome: PartyOutcome = serde_json::from_slice(&bytes).unwrap();
        // Default is 3 rounds × 3 members = 9 responses.
        assert_eq!(outcome.rounds.len(), 3);
        assert_eq!(outcome.rounds[0].responses.len(), 3);
        assert_eq!(outcome.synthesis, "echo:lead");
    }

    #[tokio::test]
    async fn start_party_with_config_override() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({
            "prompt": "Decide the roadmap",
            "configOverride": {
                "max_rounds": 1,
                "ordering": "round_robin",
                "synthesizer": "lead"
            }
        });
        let req = Request::post("/api/circles/team-eng/party")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let outcome: PartyOutcome = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(outcome.rounds.len(), 1);
    }

    #[tokio::test]
    async fn start_party_auth_denied_without_bearer() {
        let app = test_router(TestState::new(Some("secret")));
        let body = serde_json::json!({"prompt": "x", "synthesizer": "lead"});
        let req = Request::post("/api/circles/team-eng/party")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn start_party_returns_404_for_unknown_circle() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({"prompt": "x", "synthesizer": "lead"});
        let req = Request::post("/api/circles/unknown/party")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn start_party_bad_request_without_synthesizer() {
        let app = test_router(TestState::new(None));
        let body = serde_json::json!({"prompt": "x"});
        let req = Request::post("/api/circles/team-eng/party")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
