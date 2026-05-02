//! Evolution proposal HTTP routes (sera-b1gb).
//!
//! Routes:
//!   POST /api/evolution/proposals              — propose a change artifact
//!   GET  /api/evolution/proposals              — list all proposals
//!   GET  /api/evolution/proposals/{id}         — get a single proposal
//!   POST /api/evolution/proposals/{id}/approve — record an approver signature
//!   POST /api/evolution/proposals/{id}/reject  — reject a proposal
//!   POST /api/evolution/proposals/{id}/apply   — apply an approved proposal
//!   POST /api/evolution/proposals/{id}/operator-key — supply operator offline key (Tier 3)
//!
//! The pipeline is provided via the [`EvolutionAppState`] trait so callers
//! can inject any backend (in-memory for tests, persisted for production).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use sera_meta::{
    ChangeArtifact, ChangeArtifactScope, ChangeArtifactStatus, ChangeProposer,
    artifact_pipeline::{ArtifactPipeline, PipelineError},
};
#[cfg(test)]
use sera_meta::artifact_pipeline::DryRunOutcome;
use sera_types::evolution::{BlastRadius, ChangeArtifactId};

// ── AppState abstraction ─────────────────────────────────────────────────────

/// Abstraction over AppState for evolution handlers.
pub trait EvolutionAppState: Send + Sync + 'static {
    fn api_key(&self) -> &Option<String>;
    fn artifact_pipeline(&self) -> Arc<ArtifactPipeline>;
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

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse a 64-character hex string into a [`ChangeArtifactId`].
fn parse_artifact_id(s: &str) -> Result<ChangeArtifactId, StatusCode> {
    let bytes = hex::decode(s).map_err(|_| StatusCode::BAD_REQUEST)?;
    let hash: [u8; 32] = bytes.try_into().map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(ChangeArtifactId { hash })
}

fn pipeline_err_to_status(err: &PipelineError) -> StatusCode {
    match err {
        PipelineError::NotFound(_) => StatusCode::NOT_FOUND,
        PipelineError::WrongState { .. } => StatusCode::CONFLICT,
        PipelineError::PolicyRejected(_) => StatusCode::FORBIDDEN,
        PipelineError::InsufficientApprovals { .. } => StatusCode::CONFLICT,
        PipelineError::DuplicateApprover(_) => StatusCode::CONFLICT,
        PipelineError::SelfApproval => StatusCode::UNPROCESSABLE_ENTITY,
        PipelineError::OperatorKeyMissing => StatusCode::CONFLICT,
        PipelineError::DryRunFailed(_) => StatusCode::UNPROCESSABLE_ENTITY,
    }
}

// ── Request / response shapes ─────────────────────────────────────────────────

/// Request body for `POST /api/evolution/proposals`.
#[derive(Debug, Deserialize)]
pub struct CreateProposalRequest {
    pub description: String,
    pub scope: ChangeArtifactScope,
    pub blast_radius: BlastRadius,
    pub proposer: ChangeProposer,
    pub payload: serde_json::Value,
}

/// Request body for `POST /api/evolution/proposals/{id}/approve`.
#[derive(Debug, Deserialize)]
pub struct ApproveRequest {
    pub approver_principal: String,
}

/// JSON projection of a [`ChangeArtifact`] returned by every route.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProposalView {
    pub id: String,
    pub description: String,
    pub scope: ChangeArtifactScope,
    pub blast_radius: BlastRadius,
    pub proposer_principal: String,
    pub status: ChangeArtifactStatus,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ChangeArtifact> for ProposalView {
    fn from(a: ChangeArtifact) -> Self {
        Self {
            id: a.id.to_string(),
            description: a.description,
            scope: a.scope,
            blast_radius: a.blast_radius,
            proposer_principal: a.proposer.principal_id,
            status: a.status,
            payload: a.payload,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListProposalsResponse {
    pub proposals: Vec<ProposalView>,
    pub count: usize,
}

/// Response body for the approve endpoint — reports accumulated signature count.
#[derive(Debug, Serialize)]
pub struct ApproveResponse {
    pub proposal_id: String,
    pub signature_count: u8,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// POST /api/evolution/proposals
pub async fn create_proposal<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Json(body): Json<CreateProposalRequest>,
) -> Result<(StatusCode, Json<ProposalView>), StatusCode>
where
    S: EvolutionAppState,
{
    check_auth(state.api_key(), &headers)?;

    let artifact = ChangeArtifact::new(
        body.description,
        body.scope,
        body.blast_radius,
        body.proposer,
        body.payload,
    );

    let pipeline = state.artifact_pipeline();
    let id = pipeline.propose(artifact).await.map_err(|e| pipeline_err_to_status(&e))?;
    let artifact = pipeline.get(&id).await.ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::CREATED, Json(artifact.into())))
}

/// GET /api/evolution/proposals
pub async fn list_proposals<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
) -> Result<Json<ListProposalsResponse>, StatusCode>
where
    S: EvolutionAppState,
{
    check_auth(state.api_key(), &headers)?;

    let pipeline = state.artifact_pipeline();
    // Collect all statuses by reading from the internal artifacts map.
    // We use the pipeline's get-by-snapshot approach: list each status bucket.
    let statuses = [
        ChangeArtifactStatus::Proposed,
        ChangeArtifactStatus::Evaluating,
        ChangeArtifactStatus::Approved,
        ChangeArtifactStatus::Rejected,
        ChangeArtifactStatus::Applied,
        ChangeArtifactStatus::RolledBack,
    ];

    let mut proposals = Vec::new();
    for status in &statuses {
        let mut batch = pipeline.list_by_status(*status).await;
        proposals.append(&mut batch);
    }

    let count = proposals.len();
    Ok(Json(ListProposalsResponse {
        proposals: proposals.into_iter().map(Into::into).collect(),
        count,
    }))
}

/// GET /api/evolution/proposals/{id}
pub async fn get_proposal<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ProposalView>, StatusCode>
where
    S: EvolutionAppState,
{
    check_auth(state.api_key(), &headers)?;

    let artifact_id = parse_artifact_id(&id)?;
    let artifact = state
        .artifact_pipeline()
        .get(&artifact_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(artifact.into()))
}

/// POST /api/evolution/proposals/{id}/approve
pub async fn approve_proposal<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ApproveRequest>,
) -> Result<Json<ApproveResponse>, StatusCode>
where
    S: EvolutionAppState,
{
    check_auth(state.api_key(), &headers)?;

    let artifact_id = parse_artifact_id(&id)?;
    let count = state
        .artifact_pipeline()
        .approve(&artifact_id, &body.approver_principal)
        .await
        .map_err(|e| pipeline_err_to_status(&e))?;

    Ok(Json(ApproveResponse {
        proposal_id: id,
        signature_count: count,
    }))
}

/// POST /api/evolution/proposals/{id}/reject
pub async fn reject_proposal<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ProposalView>, StatusCode>
where
    S: EvolutionAppState,
{
    check_auth(state.api_key(), &headers)?;

    let artifact_id = parse_artifact_id(&id)?;
    state
        .artifact_pipeline()
        .reject(&artifact_id)
        .await
        .map_err(|e| pipeline_err_to_status(&e))?;

    let artifact = state
        .artifact_pipeline()
        .get(&artifact_id)
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(artifact.into()))
}

/// POST /api/evolution/proposals/{id}/apply
pub async fn apply_proposal<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ProposalView>, StatusCode>
where
    S: EvolutionAppState,
{
    check_auth(state.api_key(), &headers)?;

    let artifact_id = parse_artifact_id(&id)?;
    let artifact = state
        .artifact_pipeline()
        .apply(&artifact_id)
        .await
        .map_err(|e| pipeline_err_to_status(&e))?;

    Ok(Json(artifact.into()))
}

/// POST /api/evolution/proposals/{id}/operator-key
///
/// Supplies the operator offline key required for Tier-3 meta-change proposals
/// (e.g. `BlastRadius::ConstitutionalRuleSet`). Must be called before `apply`
/// for artifacts that require it.
pub async fn supply_operator_key<S>(
    State(state): State<Arc<S>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode>
where
    S: EvolutionAppState,
{
    check_auth(state.api_key(), &headers)?;

    let artifact_id = parse_artifact_id(&id)?;
    state
        .artifact_pipeline()
        .supply_operator_key(&artifact_id)
        .await
        .map_err(|e| pipeline_err_to_status(&e))?;

    Ok(StatusCode::NO_CONTENT)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
    };
    use sera_auth::{CapabilityToken, ChangeProposer};
    use tower::ServiceExt;

    struct TestState {
        api_key: Option<String>,
        pipeline: Arc<ArtifactPipeline>,
    }

    impl TestState {
        fn new(key: Option<&str>) -> Arc<Self> {
            Arc::new(Self {
                api_key: key.map(|k| k.to_owned()),
                pipeline: Arc::new(ArtifactPipeline::with_defaults()),
            })
        }
    }

    impl EvolutionAppState for TestState {
        fn api_key(&self) -> &Option<String> {
            &self.api_key
        }
        fn artifact_pipeline(&self) -> Arc<ArtifactPipeline> {
            Arc::clone(&self.pipeline)
        }
    }

    fn test_router(state: Arc<TestState>) -> Router {
        Router::new()
            .route(
                "/api/evolution/proposals",
                post(create_proposal::<TestState>).get(list_proposals::<TestState>),
            )
            .route(
                "/api/evolution/proposals/{id}",
                get(get_proposal::<TestState>),
            )
            .route(
                "/api/evolution/proposals/{id}/approve",
                post(approve_proposal::<TestState>),
            )
            .route(
                "/api/evolution/proposals/{id}/reject",
                post(reject_proposal::<TestState>),
            )
            .route(
                "/api/evolution/proposals/{id}/apply",
                post(apply_proposal::<TestState>),
            )
            .route(
                "/api/evolution/proposals/{id}/operator-key",
                post(supply_operator_key::<TestState>),
            )
            .with_state(state)
    }

    fn make_proposer(principal: &str, scopes: Vec<BlastRadius>) -> ChangeProposer {
        ChangeProposer {
            principal_id: principal.to_string(),
            capability_token: CapabilityToken {
                id: format!("tok-{principal}"),
                scopes: scopes.into_iter().collect(),
                expires_at: Utc::now() + chrono::Duration::hours(1),
                max_proposals: 10,
                signature: [0u8; 64],
                instance_id: None,
                parent_id: None,
                delegated_by: None,
                delegation_depth: 0,
            },
        }
    }

    fn tier1_body() -> serde_json::Value {
        let sig: Vec<u8> = vec![0u8; 64];
        serde_json::json!({
            "description": "improve agent memory",
            "scope": "agent_improvement",
            "blast_radius": "agent_memory",
            "proposer": {
                "principal_id": "agent-1",
                "capability_token": {
                    "id": "tok-agent-1",
                    "scopes": ["agent_memory"],
                    "expires_at": (Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                    "max_proposals": 10,
                    "signature": sig,
                }
            },
            "payload": {"key": "value"}
        })
    }

    #[tokio::test]
    async fn create_proposal_returns_201() {
        let app = test_router(TestState::new(None));
        let resp = app
            .oneshot(
                Request::post("/api/evolution/proposals")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&tier1_body()).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: ProposalView = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(view.description, "improve agent memory");
        assert_eq!(view.proposer_principal, "agent-1");
        // Deserialize status to check
        let status_json = serde_json::to_string(&view.status).unwrap();
        assert_eq!(status_json, "\"proposed\"");
    }

    #[tokio::test]
    async fn list_proposals_empty() {
        let app = test_router(TestState::new(None));
        let resp = app
            .oneshot(
                Request::get("/api/evolution/proposals")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: ListProposalsResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result.count, 0);
    }

    #[tokio::test]
    async fn list_proposals_returns_created_proposal() {
        let state = TestState::new(None);
        let app = test_router(Arc::clone(&state));

        // Create a proposal first.
        app.clone()
            .oneshot(
                Request::post("/api/evolution/proposals")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&tier1_body()).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let resp = app
            .oneshot(
                Request::get("/api/evolution/proposals")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: ListProposalsResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result.count, 1);
    }

    #[tokio::test]
    async fn get_unknown_proposal_is_404() {
        let app = test_router(TestState::new(None));
        let fake_id = "00".repeat(32);
        let resp = app
            .oneshot(
                Request::get(format!("/api/evolution/proposals/{fake_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn approve_self_approval_is_unprocessable() {
        let state = TestState::new(None);

        // Manually propose a tier-2 artifact so we can approve it.
        let proposer = make_proposer("admin-1", vec![BlastRadius::SingleHookConfig]);
        let artifact = ChangeArtifact::new(
            "hook change".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            proposer,
            serde_json::json!({}),
        );
        let id = state.pipeline.propose(artifact).await.unwrap();
        // Evaluate to move to Approved.
        state
            .pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();

        let app = test_router(Arc::clone(&state));
        let body = serde_json::json!({ "approver_principal": "admin-1" });
        let resp = app
            .oneshot(
                Request::post(format!("/api/evolution/proposals/{id}/approve"))
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn tier1_full_lifecycle_via_http() {
        let state = TestState::new(None);

        // Step 1: propose.
        let app = test_router(Arc::clone(&state));
        let resp = app
            .clone()
            .oneshot(
                Request::post("/api/evolution/proposals")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&tier1_body()).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: ProposalView = serde_json::from_slice(&bytes).unwrap();
        let id = view.id.clone();

        // Step 2: evaluate via pipeline directly (HTTP evaluate endpoint is
        // not exposed — callers drive dry-run logic in their own context).
        let artifact_id = parse_artifact_id(&id).unwrap();
        state
            .pipeline
            .evaluate(&artifact_id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();

        // Tier 1 needs 0 approvers; Step 3: apply via HTTP.
        let resp = app
            .oneshot(
                Request::post(format!("/api/evolution/proposals/{id}/apply"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let applied: ProposalView = serde_json::from_slice(&bytes).unwrap();
        let status_json = serde_json::to_string(&applied.status).unwrap();
        assert_eq!(status_json, "\"applied\"");
    }

    #[tokio::test]
    async fn reject_proposal_returns_rejected_view() {
        let state = TestState::new(None);
        // Propose a tier-2 artifact.
        let proposer = make_proposer("admin-1", vec![BlastRadius::SingleHookConfig]);
        let artifact = ChangeArtifact::new(
            "hook change".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            proposer,
            serde_json::json!({}),
        );
        let id = state.pipeline.propose(artifact).await.unwrap();

        let app = test_router(Arc::clone(&state));
        let resp = app
            .oneshot(
                Request::post(format!("/api/evolution/proposals/{id}/reject"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let view: ProposalView = serde_json::from_slice(&bytes).unwrap();
        let status_json = serde_json::to_string(&view.status).unwrap();
        assert_eq!(status_json, "\"rejected\"");
    }

    #[tokio::test]
    async fn auth_denied_without_key() {
        let app = test_router(TestState::new(Some("secret")));
        let resp = app
            .oneshot(
                Request::get("/api/evolution/proposals")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_id_returns_bad_request() {
        let app = test_router(TestState::new(None));
        let resp = app
            .oneshot(
                Request::get("/api/evolution/proposals/not-hex")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
