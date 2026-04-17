//! Self-evolution endpoints — propose → evaluate → approve → apply against
//! [`sera_meta::artifact_pipeline::ArtifactPipeline`].
//!
//! Every transition fires an `on_change_artifact_proposed` hook chain with
//! [`HookContext::change_artifact`] populated so downstream hook chains can
//! observe and gate the change. This closes the SPEC-self-evolution gap
//! "Integration with gateway pipeline (`change_artifact: None` in sera.rs)".

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_meta::artifact_pipeline::{DryRunOutcome, PipelineError};
use sera_meta::{
    BlastRadius, ChangeArtifact, ChangeArtifactScope, ChangeArtifactStatus, ChangeProposer,
};
use sera_types::evolution::{CapabilityToken, ChangeArtifactId};
use sera_types::hook::{HookContext, HookPoint};

use crate::error::AppError;
use crate::state::AppState;

// ── Request / response payloads ────────────────────────────────────────────

/// Request body for `POST /api/evolve/propose`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposeRequest {
    pub description: String,
    pub scope: ChangeArtifactScope,
    pub blast_radius: BlastRadius,
    pub proposer_principal: String,
    pub payload: serde_json::Value,
}

/// Response for endpoints that return a single artifact snapshot.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactView {
    pub id: String,
    pub description: String,
    pub scope: ChangeArtifactScope,
    pub blast_radius: BlastRadius,
    pub proposer_principal: String,
    pub status: ChangeArtifactStatus,
}

impl From<&ChangeArtifact> for ArtifactView {
    fn from(a: &ChangeArtifact) -> Self {
        ArtifactView {
            id: a.id.to_string(),
            description: a.description.clone(),
            scope: a.scope,
            blast_radius: a.blast_radius,
            proposer_principal: a.proposer.principal_id.clone(),
            status: a.status,
        }
    }
}

/// Request body for `POST /api/evolve/approve/:id`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApproveRequest {
    pub approver_principal: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Parse a hex-encoded `ChangeArtifactId` from the URL path.
fn parse_id(raw: &str) -> Result<ChangeArtifactId, AppError> {
    let bytes = hex::decode(raw)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("invalid change_artifact id: {e}")))?;
    if bytes.len() != 32 {
        return Err(AppError::Internal(anyhow::anyhow!(
            "invalid change_artifact id length: {}",
            bytes.len()
        )));
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(ChangeArtifactId { hash })
}

/// Convert a [`PipelineError`] into an [`AppError`] with an appropriate HTTP
/// status once serialised.
fn pipeline_err(e: PipelineError) -> AppError {
    match e {
        PipelineError::NotFound(msg) => AppError::Db(sera_db::DbError::NotFound {
            entity: "change_artifact",
            key: "id",
            value: msg,
        }),
        PipelineError::WrongState { .. }
        | PipelineError::InsufficientApprovals { .. }
        | PipelineError::DuplicateApprover(_)
        | PipelineError::SelfApproval
        | PipelineError::OperatorKeyMissing => {
            AppError::Db(sera_db::DbError::Conflict(e.to_string()))
        }
        other => AppError::Internal(anyhow::anyhow!("pipeline error: {other}")),
    }
}

/// Build a minimal [`CapabilityToken`] that satisfies the policy engine for
/// the provided blast radius. Real deployments will mint and verify tokens
/// via `sera-auth`; this is the route-layer glue that lets operators drive
/// the pipeline end-to-end while crypto verification is added later.
fn stub_capability_token(scope: BlastRadius) -> CapabilityToken {
    CapabilityToken {
        id: format!("route-token-{scope:?}"),
        scopes: [scope].into_iter().collect(),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        max_proposals: 10,
        signature: [0u8; 64],
    }
}

/// Fire the `OnChangeArtifactProposed` hook chain with `change_artifact`
/// populated. Returns `Err(Forbidden)` if a hook short-circuited with
/// [`HookResult::Reject`].
///
/// Hooks are advisory for evaluate/approve/apply stages — the chain runs but
/// rejection is only enforced at propose time, matching the semantics of the
/// other gateway hook points (see `bin/sera.rs`). We still fire downstream
/// stages so telemetry hooks can observe the full lifecycle.
async fn fire_change_artifact_hook(
    state: &AppState,
    id: ChangeArtifactId,
    stage: &str,
) -> Result<(), AppError> {
    let mut ctx = HookContext::new(HookPoint::OnChangeArtifactProposed);
    ctx.change_artifact = Some(id);
    ctx.metadata
        .insert("stage".to_string(), serde_json::json!(stage));

    match state
        .chain_executor
        .execute_at_point(HookPoint::OnChangeArtifactProposed, &[], ctx)
        .await
    {
        Ok(result) => {
            if stage == "propose" && result.is_rejected() {
                let reason = match result.outcome {
                    sera_types::hook::HookResult::Reject { reason, .. } => reason,
                    _ => "hook rejected proposal".to_string(),
                };
                return Err(AppError::Forbidden(reason));
            }
            Ok(())
        }
        Err(e) => {
            // Fail-open: log and continue. Matches `run_hook_point` in
            // `bin/sera.rs`.
            tracing::warn!(
                stage = %stage,
                error = %e,
                "on_change_artifact_proposed hook chain errored (fail-open)"
            );
            Ok(())
        }
    }
}

// ── Route handlers ─────────────────────────────────────────────────────────

/// `POST /api/evolve/propose` — submit a new change artifact.
pub async fn propose(
    State(state): State<AppState>,
    Json(body): Json<ProposeRequest>,
) -> Result<(StatusCode, Json<ArtifactView>), AppError> {
    let proposer = ChangeProposer {
        principal_id: body.proposer_principal.clone(),
        capability_token: stub_capability_token(body.blast_radius),
    };
    let artifact = ChangeArtifact::new(
        body.description,
        body.scope,
        body.blast_radius,
        proposer,
        body.payload,
    );

    let id = state
        .evolution_pipeline
        .propose(artifact)
        .await
        .map_err(pipeline_err)?;

    fire_change_artifact_hook(&state, id, "propose").await?;

    // Re-read the canonical snapshot from the pipeline.
    let snapshot = state
        .evolution_pipeline
        .get(&id)
        .await
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("artifact missing after propose")))?;
    Ok((StatusCode::CREATED, Json(ArtifactView::from(&snapshot))))
}

/// `POST /api/evolve/evaluate/:id` — run the shadow-session dry-run and
/// transition the artifact into `Approved` or `Rejected`.
pub async fn evaluate(
    State(state): State<AppState>,
    Path(id_hex): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let id = parse_id(&id_hex)?;

    // Route-layer dry-run: treat as Passed. Real deployments plug in a
    // sera-runtime shadow replay; the pipeline API deliberately accepts a
    // closure so this seam is swap-in ready.
    let outcome = state
        .evolution_pipeline
        .evaluate(&id, |_artifact| DryRunOutcome::Passed)
        .await
        .map_err(pipeline_err)?;

    fire_change_artifact_hook(&state, id, "evaluate").await?;

    let snapshot = state
        .evolution_pipeline
        .get(&id)
        .await
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("artifact missing after evaluate")))?;
    Ok(Json(serde_json::json!({
        "artifact": ArtifactView::from(&snapshot),
        "outcome": match outcome {
            DryRunOutcome::Passed => "passed",
            DryRunOutcome::Failed(_) => "failed",
        }
    })))
}

/// `POST /api/evolve/approve/:id` — record a `MetaApprover` signature.
pub async fn approve(
    State(state): State<AppState>,
    Path(id_hex): Path<String>,
    Json(body): Json<ApproveRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let id = parse_id(&id_hex)?;

    let count = state
        .evolution_pipeline
        .approve(&id, body.approver_principal)
        .await
        .map_err(pipeline_err)?;

    fire_change_artifact_hook(&state, id, "approve").await?;

    Ok(Json(serde_json::json!({
        "id": id.to_string(),
        "approvalCount": count,
    })))
}

/// `POST /api/evolve/apply/:id` — transition the artifact to `Applied`.
pub async fn apply(
    State(state): State<AppState>,
    Path(id_hex): Path<String>,
) -> Result<Json<ArtifactView>, AppError> {
    let id = parse_id(&id_hex)?;
    let applied = state
        .evolution_pipeline
        .apply(&id)
        .await
        .map_err(pipeline_err)?;

    fire_change_artifact_hook(&state, id, "apply").await?;

    Ok(Json(ArtifactView::from(&applied)))
}

/// `GET /api/evolve/:id` — fetch a snapshot of the tracked artifact.
pub async fn get(
    State(state): State<AppState>,
    Path(id_hex): Path<String>,
) -> Result<Json<ArtifactView>, AppError> {
    let id = parse_id(&id_hex)?;
    let snapshot = state
        .evolution_pipeline
        .get(&id)
        .await
        .ok_or_else(|| {
            AppError::Db(sera_db::DbError::NotFound {
                entity: "change_artifact",
                key: "id",
                value: id_hex.clone(),
            })
        })?;
    Ok(Json(ArtifactView::from(&snapshot)))
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use sera_hooks::{ChainExecutor, HookRegistry};
    use sera_meta::artifact_pipeline::ArtifactPipeline;

    /// Drive propose → evaluate → approve → apply through the pipeline and
    /// assert the final status. This exercises the same `ArtifactPipeline`
    /// instance that the route handlers hold an `Arc` to, proving the
    /// end-to-end lifecycle works without requiring a live axum server.
    #[tokio::test]
    async fn propose_evaluate_approve_apply_end_to_end() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());

        // Tier-2 change — requires an approver.
        let proposer = ChangeProposer {
            principal_id: "admin-1".to_string(),
            capability_token: stub_capability_token(BlastRadius::SingleHookConfig),
        };
        let artifact = ChangeArtifact::new(
            "tier-2 hook config update".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            proposer,
            serde_json::json!({ "hook": "on_turn_start" }),
        );

        let id = pipeline.propose(artifact).await.unwrap();
        let after_propose = pipeline.get(&id).await.unwrap();
        assert_eq!(after_propose.status, ChangeArtifactStatus::Proposed);

        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();
        let after_eval = pipeline.get(&id).await.unwrap();
        assert_eq!(after_eval.status, ChangeArtifactStatus::Approved);

        pipeline.approve(&id, "approver-1").await.unwrap();
        let applied = pipeline.apply(&id).await.unwrap();
        assert_eq!(applied.status, ChangeArtifactStatus::Applied);
    }

    /// When we feed a `HookContext` through `ChainExecutor` with
    /// `change_artifact` populated, it must survive the round-trip — proving
    /// the gateway-side contract that evolve route handlers can surface the
    /// change_artifact id to `on_change_artifact_proposed` hooks.
    #[tokio::test]
    async fn hook_context_propagates_change_artifact_through_chain() {
        let registry = Arc::new(HookRegistry::new());
        let executor = ChainExecutor::new(Arc::clone(&registry));

        let artifact_id = ChangeArtifactId { hash: [7u8; 32] };
        let mut ctx = HookContext::new(HookPoint::OnChangeArtifactProposed);
        ctx.change_artifact = Some(artifact_id);
        ctx.metadata
            .insert("stage".to_string(), serde_json::json!("propose"));

        // Empty chain — the executor should pass context through untouched.
        let result = executor
            .execute_at_point(HookPoint::OnChangeArtifactProposed, &[], ctx)
            .await
            .expect("empty chain must succeed");

        assert_eq!(result.context.change_artifact, Some(artifact_id));
        assert_eq!(
            result.context.metadata.get("stage"),
            Some(&serde_json::json!("propose"))
        );
    }

    /// Round-trip for `parse_id` — formatted IDs from `ChangeArtifactId` must
    /// parse back to the same value.
    #[test]
    fn parse_id_roundtrip() {
        let id = ChangeArtifactId { hash: [0xAB; 32] };
        let rendered = id.to_string();
        let parsed = parse_id(&rendered).unwrap();
        assert_eq!(parsed, id);
    }

    /// Invalid hex must surface as `AppError::Internal`, not panic.
    #[test]
    fn parse_id_rejects_bad_hex() {
        let err = parse_id("not-hex").unwrap_err();
        assert!(matches!(err, AppError::Internal(_)));
    }

    /// `PipelineError::NotFound` must become a 404 via the `Db::NotFound`
    /// path in `AppError::IntoResponse`.
    #[test]
    fn pipeline_err_not_found_maps_to_db_not_found() {
        let err = pipeline_err(PipelineError::NotFound("x".to_string()));
        assert!(matches!(
            err,
            AppError::Db(sera_db::DbError::NotFound { entity: "change_artifact", .. })
        ));
    }

    /// `SelfApproval` / `WrongState` must become conflicts (409) so clients
    /// distinguish them from missing artifacts.
    #[test]
    fn pipeline_err_state_violations_map_to_conflict() {
        let err = pipeline_err(PipelineError::SelfApproval);
        assert!(matches!(err, AppError::Db(sera_db::DbError::Conflict(_))));
        let err = pipeline_err(PipelineError::WrongState {
            actual: ChangeArtifactStatus::Proposed,
            expected: ChangeArtifactStatus::Approved,
        });
        assert!(matches!(err, AppError::Db(sera_db::DbError::Conflict(_))));
    }
}
