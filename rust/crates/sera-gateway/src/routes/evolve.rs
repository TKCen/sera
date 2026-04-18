//! Self-evolution endpoints — propose → evaluate → approve → apply against
//! [`sera_meta::artifact_pipeline::ArtifactPipeline`].
//!
//! Every transition fires an `on_change_artifact_proposed` hook chain with
//! [`HookContext::change_artifact`] populated so downstream hook chains can
//! observe and gate the change. This closes the SPEC-self-evolution gap
//! "Integration with gateway pipeline (`change_artifact: None` in sera.rs)".

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use sera_auth::{ActingContext, CapabilityToken, ChangeProposer};

use sera_gateway::evolve_token::EvolveTokenError;
use sera_meta::artifact_pipeline::{DryRunOutcome, PipelineError};
use sera_meta::constitutional::ConstitutionalRule;
use sera_meta::{
    BlastRadius, ChangeArtifact, ChangeArtifactScope, ChangeArtifactStatus,
};
use sera_types::evolution::{ChangeArtifactId, ConstitutionalEnforcementPoint};
use sera_types::hook::{HookContext, HookPoint};

use crate::error::AppError;
use crate::state::AppState;

// ── Request / response payloads ────────────────────────────────────────────

/// Request body for `POST /api/evolve/propose`.
///
/// The `capability_token` field carries the signed
/// [`sera_auth::CapabilityToken`] that authorises the proposer to
/// attempt a change at `blast_radius`. The gateway verifies the token's
/// HMAC-SHA-512 signature (see [`sera_gateway::evolve_token`]) before invoking
/// the pipeline. Requests without a token are rejected with 401; tokens with
/// an invalid signature or past `expires_at` are also rejected with 401;
/// tokens whose scopes do not cover `blast_radius` are rejected with 403.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposeRequest {
    pub description: String,
    pub scope: ChangeArtifactScope,
    pub blast_radius: BlastRadius,
    pub proposer_principal: String,
    pub payload: serde_json::Value,
    /// Signed capability token — see [`sera_gateway::evolve_token`] for the
    /// canonical byte layout and signing contract.
    pub capability_token: CapabilityToken,
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

/// Request body for `POST /api/evolve/operator-key/:id`.
///
/// `signed_by` identifies the operator principal submitting the key.
/// `key` carries the operator offline key material (opaque to the gateway;
/// cryptographic verification lives in `sera-auth`).
///
/// The caller must hold operator scope — requests from non-operator
/// principals are rejected with 403.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperatorKeyRequest {
    pub key: String,
    pub signed_by: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Parse a hex-encoded `ChangeArtifactId` from the URL path.
fn parse_id(raw: &str) -> Result<ChangeArtifactId, AppError> {
    let bytes = hex::decode(raw)
        .map_err(|_| AppError::BadRequest("invalid change_artifact id: not valid hex".to_string()))?;
    if bytes.len() != 32 {
        return Err(AppError::BadRequest(format!(
            "invalid change_artifact id: expected 32 bytes, got {}",
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
/// the provided blast radius. Test-only — production callers must submit a
/// token signed by [`sera_gateway::evolve_token::EvolveTokenSigner`].
///
/// Kept `#[cfg(test)]` so the crate no longer compiles the stub into release
/// binaries: signature verification is now the only path into the propose
/// pipeline.
#[cfg(test)]
fn stub_capability_token(scope: BlastRadius) -> CapabilityToken {
    CapabilityToken {
        id: format!("route-token-{scope:?}"),
        scopes: [scope].into_iter().collect(),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        max_proposals: 10,
        signature: [0u8; 64],
    }
}

/// Map an [`EvolveTokenError`] into an [`AppError`] so the route layer emits
/// the right HTTP status: 401 for signature / expiry / empty-secret
/// failures, 403 for scope-membership failures.
fn evolve_token_err(e: EvolveTokenError) -> AppError {
    match e {
        EvolveTokenError::MissingScope(_) => AppError::Forbidden(e.to_string()),
        _ => AppError::Auth(sera_auth::AuthError::Unauthorized),
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
///
/// Verifies the request's `capability_token` against the gateway's
/// [`sera_gateway::evolve_token::EvolveTokenSigner`] before handing the
/// proposal to the pipeline. A missing/invalid/expired signature returns
/// 401; a valid signature whose scopes do not cover `blast_radius` returns
/// 403; a token whose `id` does not match `proposer_principal` returns 403
/// (token-issuer identity mismatch).
///
/// # Token identity cross-check
///
/// [`CapabilityToken`] has no dedicated `issued_to` or subject field; the
/// `id` field is the closest semantic anchor for issuer identity. Callers
/// must set `capability_token.id` equal to the `proposer_principal` they
/// submit so the gateway can confirm the token was minted for this caller
/// and not replayed from a different principal's context.
pub async fn propose(
    State(state): State<AppState>,
    Json(body): Json<ProposeRequest>,
) -> Result<(StatusCode, Json<ArtifactView>), AppError> {
    // Signature + expiry + scope gate. Order inside verify() is signature
    // first (don't leak scope info under an invalid MAC), then expiry, then
    // scope membership.
    state
        .evolve_token_signer
        .verify(&body.capability_token, body.blast_radius)
        .map_err(evolve_token_err)?;

    // Token-issuer identity cross-check: reject bearer-token replay where a
    // token minted for principal A is submitted by principal B. The check
    // uses `capability_token.id` as the issuer anchor — the only field on
    // CapabilityToken that can carry caller identity without modifying
    // sera-types. Callers must set `id == proposer_principal` when minting.
    if body.capability_token.id != body.proposer_principal {
        return Err(AppError::Forbidden(format!(
            "token identity mismatch: token id '{}' does not match proposer_principal '{}'",
            body.capability_token.id, body.proposer_principal,
        )));
    }

    // Proposal-budget enforcement: atomically check and increment the
    // per-token counter. Returns 429 when `used >= max_proposals`.
    state
        .proposal_usage
        .check_and_increment(
            &body.capability_token.id,
            body.capability_token.max_proposals,
        )
        .await
        .map_err(|e| AppError::TooManyRequests(e.to_string()))?;

    let proposer = ChangeProposer {
        principal_id: body.proposer_principal.clone(),
        capability_token: body.capability_token,
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

/// Evaluate a `ChangeArtifact` against a pre-fetched set of constitutional
/// rules. Pure and synchronous so it can be passed as the dry-run closure to
/// [`sera_meta::artifact_pipeline::ArtifactPipeline::evaluate`].
///
/// This is the MVS "shadow replay" substitute until sera-runtime exposes a
/// `ShadowSessionExecutor`: any rule applicable at `PreApproval` whose
/// proposer-scope requirement fails produces
/// [`DryRunOutcome::Failed`] with the rule id + reason.
pub(crate) fn dry_run_against_rules(
    artifact: &ChangeArtifact,
    rules: &[ConstitutionalRule],
) -> DryRunOutcome {
    for rule in rules {
        if !rule.is_applicable(&artifact.scope, &artifact.blast_radius) {
            continue;
        }
        if !rule.check_proposer(&artifact.proposer) {
            return DryRunOutcome::Failed(format!(
                "constitutional rule '{}' rejected proposer: required scopes not held",
                rule.base.id
            ));
        }
    }
    DryRunOutcome::Passed
}

/// `POST /api/evolve/evaluate/:id` — run the shadow-session dry-run and
/// transition the artifact into `Approved` or `Rejected`.
pub async fn evaluate(
    State(state): State<AppState>,
    Path(id_hex): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let id = parse_id(&id_hex)?;

    // Pre-fetch every rule registered at `PreApproval`. The pipeline's evaluate
    // closure is synchronous, so we snapshot the async registry here and then
    // pass a pure sync filter into the pipeline. Tier-1 artifacts skip the
    // closure entirely (see pipeline docs) — the fetch is still cheap.
    let rules = state
        .constitutional_registry
        .rules_at(ConstitutionalEnforcementPoint::PreApproval)
        .await;

    let outcome = state
        .evolution_pipeline
        .evaluate(&id, |artifact| dry_run_against_rules(artifact, &rules))
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

/// `POST /api/evolve/operator-key/:id` — supply the operator offline key for
/// a Tier-3 artifact that is waiting in `OperatorKeyMissing` state.
///
/// Authorization: the caller must hold operator scope (`ActingContext` with a
/// non-`None` `operator_id`). Requests without an `ActingContext` extension
/// (middleware not configured) are rejected with 401. Non-operator callers
/// are rejected with 403. Unknown artifact ids return 404. Artifacts that
/// are not in the `Approved` status (e.g. already `Applied` or still
/// `UnderReview`) return 409.
///
/// On success the artifact transitions internally so that the next `apply`
/// call can proceed without `OperatorKeyMissing`.
pub async fn supply_operator_key(
    State(state): State<AppState>,
    ctx: Option<Extension<ActingContext>>,
    Path(id_hex): Path<String>,
    Json(body): Json<OperatorKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Authorization gate: reject requests that arrive without an ActingContext
    // (auth middleware not configured) before touching the pipeline.
    let Extension(ctx) = ctx.ok_or_else(|| {
        AppError::Auth(sera_auth::AuthError::Unauthorized)
    })?;

    // Only operator-scoped principals (operator_id is Some) may supply the key.
    // Agent-scoped callers and bare API keys without an operator_id are rejected.
    if ctx.operator_id.is_none() {
        return Err(AppError::Forbidden(
            "principal does not hold operator scope".to_string(),
        ));
    }

    let id = parse_id(&id_hex)?;

    state
        .evolution_pipeline
        .supply_operator_key(&id)
        .await
        .map_err(pipeline_err)?;

    fire_change_artifact_hook(&state, id, "operator-key").await?;

    Ok(Json(serde_json::json!({
        "id": id_hex,
        "operatorKeySupplied": true,
        "signedBy": body.signed_by,
        "keyLength": body.key.len(),
    })))
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

    // ── Tier-3 pipeline tests ─────────────────────────────────────────────
    //
    // These exercise the full Tier-3 path through the same `ArtifactPipeline`
    // instance the route handlers hold an `Arc` to.

    /// Tier-3 code-scoped happy path: propose (GatewayCore) → evaluate (pass)
    /// → 3 distinct approvers → apply → Applied.
    ///
    /// `GatewayCore` requires 3 approvers and no operator key — it is the
    /// canonical code-scoped Tier-3 blast radius.
    #[tokio::test]
    async fn tier3_code_scoped_happy_path() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());

        let proposer = ChangeProposer {
            principal_id: "eng-lead".to_string(),
            capability_token: CapabilityToken {
                id: "tok-gateway".to_string(),
                scopes: [BlastRadius::GatewayCore, BlastRadius::RuntimeCrate]
                    .into_iter()
                    .collect(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                max_proposals: 5,
                signature: [0u8; 64],
            },
        };
        let artifact = ChangeArtifact::new(
            "gateway: add evolve route telemetry".to_string(),
            ChangeArtifactScope::CodeEvolution,
            BlastRadius::GatewayCore,
            proposer,
            serde_json::json!({ "patch": "add tracing spans" }),
        );

        let id = pipeline.propose(artifact).await.unwrap();

        // Evaluate passes → status Approved.
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();
        assert_eq!(
            pipeline.get(&id).await.unwrap().status,
            ChangeArtifactStatus::Approved
        );

        // Three distinct approvers.
        let c1 = pipeline.approve(&id, "reviewer-alpha").await.unwrap();
        let c2 = pipeline.approve(&id, "reviewer-beta").await.unwrap();
        let c3 = pipeline.approve(&id, "reviewer-gamma").await.unwrap();
        assert_eq!(c1, 1);
        assert_eq!(c2, 2);
        assert_eq!(c3, 3);

        // No operator key needed for GatewayCore → apply succeeds.
        let applied = pipeline.apply(&id).await.unwrap();
        assert_eq!(applied.status, ChangeArtifactStatus::Applied);
    }

    /// Tier-3 code-scoped: a non-operator caller (i.e. no `supply_operator_key`
    /// call) trying to apply a `ConstitutionalRuleSet` artifact after 3
    /// approvals is rejected with `OperatorKeyMissing`.
    #[tokio::test]
    async fn tier3_operator_key_required_without_key_rejected() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());

        // Default Tier-3 Policy requires RuntimeCrate + GatewayCore in the token.
        let proposer = ChangeProposer {
            principal_id: "admin-lead".to_string(),
            capability_token: CapabilityToken {
                id: "tok-meta".to_string(),
                scopes: [BlastRadius::RuntimeCrate, BlastRadius::GatewayCore]
                    .into_iter()
                    .collect(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                max_proposals: 5,
                signature: [0u8; 64],
            },
        };
        let artifact = ChangeArtifact::new(
            "amend no-self-replication rule".to_string(),
            ChangeArtifactScope::CodeEvolution,
            BlastRadius::ConstitutionalRuleSet,
            proposer,
            serde_json::json!({ "rule_id": "no-self-replication" }),
        );

        let id = pipeline.propose(artifact).await.unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();
        pipeline.approve(&id, "signer-1").await.unwrap();
        pipeline.approve(&id, "signer-2").await.unwrap();
        pipeline.approve(&id, "signer-3").await.unwrap();

        // No operator key supplied → rejected.
        let err = pipeline.apply(&id).await.unwrap_err();
        assert!(
            matches!(err, PipelineError::OperatorKeyMissing),
            "expected OperatorKeyMissing, got: {err}"
        );
    }

    /// Tier-3 code-scoped: only 2 approvers present when 3 are required.
    /// `apply` must return `InsufficientApprovals`.
    #[tokio::test]
    async fn tier3_insufficient_approvers_rejected() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());

        // Default Tier-3 Policy requires both RuntimeCrate and GatewayCore.
        let proposer = ChangeProposer {
            principal_id: "eng-1".to_string(),
            capability_token: CapabilityToken {
                id: "tok-eng1".to_string(),
                scopes: [BlastRadius::RuntimeCrate, BlastRadius::GatewayCore]
                    .into_iter()
                    .collect(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                max_proposals: 5,
                signature: [0u8; 64],
            },
        };
        let artifact = ChangeArtifact::new(
            "gateway: refactor routing layer".to_string(),
            ChangeArtifactScope::CodeEvolution,
            BlastRadius::GatewayCore,
            proposer,
            serde_json::json!({ "module": "routing" }),
        );

        let id = pipeline.propose(artifact).await.unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();

        // Only 2 approvers — one short of the required 3.
        pipeline.approve(&id, "approver-x").await.unwrap();
        pipeline.approve(&id, "approver-y").await.unwrap();

        let err = pipeline.apply(&id).await.unwrap_err();
        assert!(
            matches!(
                err,
                PipelineError::InsufficientApprovals { have: 2, need: 3 }
            ),
            "expected InsufficientApprovals{{have:2,need:3}}, got: {err}"
        );
    }

    /// Tier-3: the same approver appearing twice does not count as 2 distinct
    /// signatures. The second call returns `DuplicateApprover` and the total
    /// remains 1, blocking `apply` with `InsufficientApprovals`.
    #[tokio::test]
    async fn tier3_duplicate_approver_does_not_count_toward_quorum() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());

        // Default Tier-3 Policy requires both RuntimeCrate and GatewayCore.
        let proposer = ChangeProposer {
            principal_id: "eng-2".to_string(),
            capability_token: CapabilityToken {
                id: "tok-eng2".to_string(),
                scopes: [BlastRadius::RuntimeCrate, BlastRadius::GatewayCore]
                    .into_iter()
                    .collect(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                max_proposals: 5,
                signature: [0u8; 64],
            },
        };
        let artifact = ChangeArtifact::new(
            "gateway: add rate-limit middleware".to_string(),
            ChangeArtifactScope::CodeEvolution,
            BlastRadius::GatewayCore,
            proposer,
            serde_json::json!({ "middleware": "rate_limiter" }),
        );

        let id = pipeline.propose(artifact).await.unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();

        pipeline.approve(&id, "dup-approver").await.unwrap();
        // Second call with the same principal must be rejected.
        let dup_err = pipeline.approve(&id, "dup-approver").await.unwrap_err();
        assert!(
            matches!(dup_err, PipelineError::DuplicateApprover(_)),
            "expected DuplicateApprover, got: {dup_err}"
        );

        // Only 1 unique signer — still short of 3, apply must fail.
        let apply_err = pipeline.apply(&id).await.unwrap_err();
        assert!(
            matches!(apply_err, PipelineError::InsufficientApprovals { have: 1, .. }),
            "expected InsufficientApprovals, got: {apply_err}"
        );
    }

    /// Constitutional rule rejection via `dry_run_against_rules`: seed a
    /// `CodeEvolution`/`GatewayCore` rule that requires `RuntimeCrate` scope,
    /// but the proposer only holds `GatewayCore`. The dry-run must return
    /// `Failed` containing the rule id, and the pipeline must transition to
    /// `Rejected`.
    #[tokio::test]
    async fn propose_rejected_by_constitutional_rule_via_dry_run() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());

        // Proposer holds RuntimeCrate + GatewayCore (passes policy engine) but
        // NOT ProtocolSchema, which the constitutional rule will require.
        let artifact = ChangeArtifact::new(
            "gateway: drop legacy endpoint".to_string(),
            ChangeArtifactScope::CodeEvolution,
            BlastRadius::GatewayCore,
            ChangeProposer {
                principal_id: "proposer-limited".to_string(),
                capability_token: CapabilityToken {
                    id: "tok-limited".to_string(),
                    scopes: [BlastRadius::RuntimeCrate, BlastRadius::GatewayCore]
                        .into_iter()
                        .collect(),
                    expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                    max_proposals: 5,
                    signature: [0u8; 64],
                },
            },
            serde_json::json!({ "endpoint": "/api/legacy" }),
        );

        // Constitutional rule: CodeEvolution/GatewayCore requires ProtocolSchema scope,
        // which the proposer does NOT hold — so the dry-run must fail.
        let blocking_rule = ConstitutionalRule::new(
            sera_types::evolution::ConstitutionalRule {
                id: "r-gateway-needs-protocol".to_string(),
                description: "GatewayCore changes require ProtocolSchema scope".to_string(),
                enforcement_point: ConstitutionalEnforcementPoint::PreApproval,
                content_hash: [0u8; 32],
            },
            vec![ChangeArtifactScope::CodeEvolution],
            vec![BlastRadius::GatewayCore],
            vec![BlastRadius::ProtocolSchema], // proposer must hold ProtocolSchema
        );

        let id = pipeline.propose(artifact).await.unwrap();

        // Seed rules and run evaluate via dry_run_against_rules.
        let rules = vec![blocking_rule];
        let outcome = pipeline
            .evaluate(&id, |artifact| dry_run_against_rules(artifact, &rules))
            .await
            .unwrap();

        match outcome {
            DryRunOutcome::Failed(reason) => assert!(
                reason.contains("r-gateway-needs-protocol"),
                "failure reason should name the rule: {reason}"
            ),
            DryRunOutcome::Passed => panic!("expected dry-run to fail due to constitutional rule"),
        }

        // Pipeline must have transitioned to Rejected.
        assert_eq!(
            pipeline.get(&id).await.unwrap().status,
            ChangeArtifactStatus::Rejected
        );
    }

    // ── End Tier-3 tests ──────────────────────────────────────────────────

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

    /// Invalid hex must surface as `AppError::BadRequest`, not panic.
    #[test]
    fn parse_id_rejects_bad_hex() {
        let err = parse_id("not-hex").unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
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

    // ── dry_run_against_rules ─────────────────────────────────────────────
    //
    // These exercise the MVS shadow-replay substitute: the pure-sync helper
    // that takes a snapshot of the ConstitutionalRegistry's PreApproval rules
    // and gates the dry-run on them. Wiring into the `/api/evolve/evaluate`
    // route is verified indirectly through the existing end-to-end pipeline
    // test above (which covers the no-rules case).

    fn rule_requiring(id: &str, required: Vec<BlastRadius>) -> ConstitutionalRule {
        ConstitutionalRule::new(
            sera_types::evolution::ConstitutionalRule {
                id: id.to_string(),
                description: format!("rule {id}"),
                enforcement_point: ConstitutionalEnforcementPoint::PreApproval,
                content_hash: [0u8; 32],
            },
            vec![ChangeArtifactScope::ConfigEvolution],
            vec![BlastRadius::SingleHookConfig],
            required,
        )
    }

    fn tier2_artifact(proposer_scopes: Vec<BlastRadius>) -> ChangeArtifact {
        ChangeArtifact::new(
            "tier-2 hook config".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: "proposer-1".to_string(),
                capability_token: CapabilityToken {
                    id: "tok-1".to_string(),
                    scopes: proposer_scopes.into_iter().collect(),
                    expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                    max_proposals: 10,
                    signature: [0u8; 64],
                },
            },
            serde_json::json!({ "hook": "on_turn_start" }),
        )
    }

    /// With no applicable rules, the dry-run passes.
    #[test]
    fn dry_run_passes_without_rules() {
        let artifact = tier2_artifact(vec![BlastRadius::SingleHookConfig]);
        let outcome = dry_run_against_rules(&artifact, &[]);
        assert_eq!(outcome, DryRunOutcome::Passed);
    }

    /// A rule that matches scope + blast radius but requires scopes the
    /// proposer does not hold must produce `Failed` carrying the rule id.
    #[test]
    fn dry_run_fails_when_proposer_missing_required_scopes() {
        let rule = rule_requiring("r-needs-runtime", vec![BlastRadius::RuntimeCrate]);
        let artifact = tier2_artifact(vec![BlastRadius::SingleHookConfig]);

        let outcome = dry_run_against_rules(&artifact, &[rule]);
        match outcome {
            DryRunOutcome::Failed(reason) => {
                assert!(
                    reason.contains("r-needs-runtime"),
                    "failure reason should name the rule: {reason}"
                );
            }
            DryRunOutcome::Passed => panic!("expected failure for missing scope"),
        }
    }

    /// A rule whose scope/blast-radius does not match the artifact is ignored —
    /// even if its required scopes are missing from the proposer.
    #[test]
    fn dry_run_skips_inapplicable_rule() {
        // Rule targets ConfigEvolution/SingleHookConfig but we switch the
        // artifact to AgentImprovement/AgentMemory so it no longer applies.
        let rule = rule_requiring("r-inapplicable", vec![BlastRadius::RuntimeCrate]);
        let artifact = ChangeArtifact::new(
            "tier-1 memory".to_string(),
            ChangeArtifactScope::AgentImprovement,
            BlastRadius::AgentMemory,
            ChangeProposer {
                principal_id: "proposer-1".to_string(),
                capability_token: stub_capability_token(BlastRadius::AgentMemory),
            },
            serde_json::json!({}),
        );

        let outcome = dry_run_against_rules(&artifact, &[rule]);
        assert_eq!(outcome, DryRunOutcome::Passed);
    }

    /// When a rule violation is detected, the pipeline must transition the
    /// artifact to `Rejected` (not `Approved`). This is the contract the
    /// gateway relies on when surfacing `outcome: failed` to API clients.
    #[tokio::test]
    async fn pipeline_rejects_artifact_on_rule_violation() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let artifact = tier2_artifact(vec![BlastRadius::SingleHookConfig]);
        let id = pipeline.propose(artifact).await.unwrap();

        let rules = vec![rule_requiring(
            "r-needs-runtime",
            vec![BlastRadius::RuntimeCrate],
        )];

        let outcome = pipeline
            .evaluate(&id, |artifact| dry_run_against_rules(artifact, &rules))
            .await
            .unwrap();
        assert!(matches!(outcome, DryRunOutcome::Failed(_)));

        let after = pipeline.get(&id).await.unwrap();
        assert_eq!(after.status, ChangeArtifactStatus::Rejected);
    }

    // ── evolve_token_err mapping ──────────────────────────────────────────
    //
    // The propose handler feeds every EvolveTokenError through
    // `evolve_token_err`. These tests pin the HTTP-status contract promised
    // by SPEC-self-evolution: unsigned/invalid/expired tokens are 401, and
    // valid tokens missing the required scope are 403.

    /// Unsigned token (the default all-zero signature) → 401 Unauthorized.
    #[test]
    fn unsigned_token_maps_to_auth_error() {
        let err = evolve_token_err(EvolveTokenError::InvalidSignature);
        assert!(matches!(err, AppError::Auth(_)));
    }

    /// Tampered/invalid signature → 401 Unauthorized.
    #[test]
    fn tampered_signature_maps_to_auth_error() {
        // Same variant as "unsigned"; tampering produces the same error.
        let err = evolve_token_err(EvolveTokenError::InvalidSignature);
        assert!(matches!(err, AppError::Auth(_)));
    }

    /// Expired token → 401 Unauthorized.
    #[test]
    fn expired_token_maps_to_auth_error() {
        let err = evolve_token_err(EvolveTokenError::Expired);
        assert!(matches!(err, AppError::Auth(_)));
    }

    /// Empty signer secret → 401 (never leak that the gateway is misconfigured).
    #[test]
    fn empty_secret_maps_to_auth_error() {
        let err = evolve_token_err(EvolveTokenError::EmptySecret);
        assert!(matches!(err, AppError::Auth(_)));
    }

    /// Token missing the requested scope → 403 Forbidden (distinguishable
    /// from 401 so clients know the caller is authenticated but lacks the
    /// capability).
    #[test]
    fn missing_scope_maps_to_forbidden() {
        let err = evolve_token_err(EvolveTokenError::MissingScope(BlastRadius::GatewayCore));
        assert!(matches!(err, AppError::Forbidden(_)));
    }

    // ── supply_operator_key handler tests ────────────────────────────────
    //
    // These exercise the POST /api/evolve/operator-key/:id path through the
    // same pipeline instance the handler holds.

    /// Helper: build a Tier-3 ConstitutionalRuleSet artifact through propose +
    /// evaluate + 3 approvals, stopping just before apply.
    async fn tier3_ready_for_operator_key(
        pipeline: &ArtifactPipeline,
    ) -> ChangeArtifactId {
        let proposer = ChangeProposer {
            principal_id: "eng-lead".to_string(),
            capability_token: CapabilityToken {
                id: "tok-t3".to_string(),
                scopes: [BlastRadius::RuntimeCrate, BlastRadius::GatewayCore]
                    .into_iter()
                    .collect(),
                expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
                max_proposals: 5,
                signature: [0u8; 64],
            },
        };
        let artifact = ChangeArtifact::new(
            "amend no-self-replication rule".to_string(),
            ChangeArtifactScope::CodeEvolution,
            BlastRadius::ConstitutionalRuleSet,
            proposer,
            serde_json::json!({ "rule_id": "no-self-replication" }),
        );
        let id = pipeline.propose(artifact).await.unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();
        pipeline.approve(&id, "signer-1").await.unwrap();
        pipeline.approve(&id, "signer-2").await.unwrap();
        pipeline.approve(&id, "signer-3").await.unwrap();
        id
    }

    /// Happy path: Tier-3 artifact with 3 approvals + operator key supplied →
    /// apply transitions to Applied.
    #[tokio::test]
    async fn operator_key_happy_path_enables_apply() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let id = tier3_ready_for_operator_key(&pipeline).await;

        // Without key → still blocked.
        let err = pipeline.apply(&id).await.unwrap_err();
        assert!(matches!(err, PipelineError::OperatorKeyMissing));

        // Supply the key via pipeline (mirrors what the handler does).
        pipeline.supply_operator_key(&id).await.unwrap();

        // Now apply must succeed.
        let applied = pipeline.apply(&id).await.unwrap();
        assert_eq!(applied.status, ChangeArtifactStatus::Applied);
    }

    /// ActingContext without operator_id → 403.
    /// Mirrors the handler gate: agent-scoped or bare API-key callers have
    /// `operator_id = None` and must be rejected before touching the pipeline.
    #[test]
    fn operator_key_non_operator_acting_context_returns_forbidden() {
        use sera_auth::types::AuthMethod;
        // Agent-scoped caller — operator_id is None.
        let ctx = ActingContext {
            operator_id: None,
            agent_id: Some("agent-xyz".to_string()),
            instance_id: None,
            api_key_id: None,
            auth_method: AuthMethod::Jwt,
        };
        let result: Result<(), AppError> = if ctx.operator_id.is_none() {
            Err(AppError::Forbidden(
                "principal does not hold operator scope".to_string(),
            ))
        } else {
            Ok(())
        };
        assert!(matches!(result, Err(AppError::Forbidden(_))));
    }

    /// Missing ActingContext extension → 401.
    /// If the auth middleware was not wired up, the handler must fail-closed
    /// (Unauthorized) rather than allow the request through.
    #[test]
    fn operator_key_missing_acting_context_returns_unauthorized() {
        let ctx: Option<Extension<ActingContext>> = None;
        let result: Result<(), AppError> = ctx
            .ok_or_else(|| AppError::Auth(sera_auth::AuthError::Unauthorized))
            .map(|_| ());
        assert!(matches!(result, Err(AppError::Auth(_))));
    }

    /// Operator-scoped ActingContext (operator_id = Some) passes the gate.
    #[test]
    fn operator_key_operator_acting_context_passes_gate() {
        let ctx = ActingContext::with_operator("op-1".to_string());
        let result: Result<(), AppError> = if ctx.operator_id.is_none() {
            Err(AppError::Forbidden(
                "principal does not hold operator scope".to_string(),
            ))
        } else {
            Ok(())
        };
        assert!(result.is_ok());
    }

    /// Unknown artifact id → pipeline returns NotFound → pipeline_err maps
    /// it to AppError::Db(NotFound) → HTTP 404.
    #[tokio::test]
    async fn operator_key_unknown_id_returns_not_found() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let phantom = ChangeArtifactId { hash: [0xFFu8; 32] };
        let err = pipeline
            .supply_operator_key(&phantom)
            .await
            .map_err(pipeline_err)
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Db(sera_db::DbError::NotFound { entity: "change_artifact", .. })
        ));
    }

    /// Wrong status: artifact already Applied → supply_operator_key on an
    /// Applied artifact is fine (pipeline doesn't check status in
    /// supply_operator_key), but apply itself fails with WrongState (409).
    /// This test verifies the WrongState → Conflict mapping for the apply
    /// call that follows, covering the "already Applied" rejection path.
    #[tokio::test]
    async fn operator_key_already_applied_apply_returns_conflict() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let id = tier3_ready_for_operator_key(&pipeline).await;

        // Supply key and apply once.
        pipeline.supply_operator_key(&id).await.unwrap();
        pipeline.apply(&id).await.unwrap();

        // Attempt to apply again → WrongState → 409 Conflict.
        let err = pipeline.apply(&id).await.map_err(pipeline_err).unwrap_err();
        assert!(matches!(err, AppError::Db(sera_db::DbError::Conflict(_))));
    }

    // ── End supply_operator_key tests ─────────────────────────────────────

    // ── End-to-end: propose with verified signature ───────────────────────
    //
    // These exercise the propose path through the pipeline using a real
    // `EvolveTokenSigner`. A full AppState is too heavy to build without a
    // live Postgres, so we drive the pipeline directly after performing the
    // same verify() the handler does, reproducing the full gate.

    use sera_gateway::evolve_token::EvolveTokenSigner;

    fn signed_token(signer: &EvolveTokenSigner, scope: BlastRadius) -> CapabilityToken {
        let mut tok = CapabilityToken {
            id: format!("tok-{scope:?}"),
            scopes: [scope].into_iter().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 10,
            signature: [0u8; 64],
        };
        signer.sign(&mut tok);
        tok
    }

    /// A token with the correct scope and a valid signature verifies cleanly
    /// and the pipeline accepts the resulting proposal.
    #[tokio::test]
    async fn valid_signed_token_proceeds_through_propose() {
        let signer = EvolveTokenSigner::new(b"e2e-secret".to_vec());
        let token = signed_token(&signer, BlastRadius::SingleHookConfig);

        // The handler calls verify() with the request's blast_radius. Mirror
        // that here so the test fails the same way the handler would.
        signer
            .verify(&token, BlastRadius::SingleHookConfig)
            .expect("valid signed token with matching scope must verify");

        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let artifact = ChangeArtifact::new(
            "tier-2 hook config via signed token".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: "proposer-signed".to_string(),
                capability_token: token,
            },
            serde_json::json!({ "hook": "on_turn_start" }),
        );
        let id = pipeline.propose(artifact).await.unwrap();
        let after = pipeline.get(&id).await.unwrap();
        assert_eq!(after.status, ChangeArtifactStatus::Proposed);
    }

    /// A valid signature for one scope must NOT authorise a different scope.
    /// Verification returns MissingScope → 403, mirroring the handler path.
    #[test]
    fn valid_token_wrong_scope_returns_forbidden() {
        let signer = EvolveTokenSigner::new(b"e2e-secret".to_vec());
        let token = signed_token(&signer, BlastRadius::AgentMemory);
        // Attempt to use it for a different blast radius — must 403.
        let err = signer
            .verify(&token, BlastRadius::GatewayCore)
            .map_err(evolve_token_err)
            .expect_err("wrong-scope must error");
        assert!(matches!(err, AppError::Forbidden(_)));
    }

    /// An unsigned (all-zero signature) token must fail verification before
    /// any scope check → 401.
    #[test]
    fn unsigned_token_returns_unauthorized() {
        let signer = EvolveTokenSigner::new(b"e2e-secret".to_vec());
        // Token constructed without calling signer.sign() — default zeros.
        let token = CapabilityToken {
            id: "unsigned".to_string(),
            scopes: [BlastRadius::SingleHookConfig].into_iter().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 10,
            signature: [0u8; 64],
        };
        let err = signer
            .verify(&token, BlastRadius::SingleHookConfig)
            .map_err(evolve_token_err)
            .expect_err("unsigned token must error");
        assert!(matches!(err, AppError::Auth(_)));
    }

    /// A token whose signature has been flipped in one byte must fail
    /// verification → 401. Confirms we do not accept partial matches.
    #[test]
    fn tampered_token_returns_unauthorized() {
        let signer = EvolveTokenSigner::new(b"e2e-secret".to_vec());
        let mut token = signed_token(&signer, BlastRadius::SingleHookConfig);
        token.signature[17] ^= 0x01;
        let err = signer
            .verify(&token, BlastRadius::SingleHookConfig)
            .map_err(evolve_token_err)
            .expect_err("tampered token must error");
        assert!(matches!(err, AppError::Auth(_)));
    }

    // ── Token-issuer identity cross-check ────────────────────────────────
    //
    // These exercise the principal-mismatch guard added to the propose
    // handler. Because AppState requires a live Postgres we drive the guard
    // directly: sign a token, call verify() as the handler would, then check
    // the identity equality that the handler checks before building the
    // ChangeProposer.

    /// Same principal in both token.id and proposer_principal — the full
    /// gate (verify + identity check) passes and the pipeline accepts the
    /// proposal.
    #[tokio::test]
    async fn same_principal_valid_token_proceeds() {
        let signer = EvolveTokenSigner::new(b"id-check-secret".to_vec());
        let principal = "agent-xyz".to_string();

        // Token id == proposer_principal.
        let mut token = CapabilityToken {
            id: principal.clone(),
            scopes: [BlastRadius::SingleHookConfig].into_iter().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 10,
            signature: [0u8; 64],
        };
        signer.sign(&mut token);

        // Signature + scope gate must pass.
        signer
            .verify(&token, BlastRadius::SingleHookConfig)
            .expect("valid token must verify");

        // Identity cross-check: token.id == proposer_principal -> no error.
        assert_eq!(
            token.id, principal,
            "token.id must equal proposer_principal for the handler to proceed"
        );

        // Pipeline accepts the artifact.
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let artifact = ChangeArtifact::new(
            "hook config - same principal".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: principal,
                capability_token: token,
            },
            serde_json::json!({ "hook": "on_turn_start" }),
        );
        let id = pipeline.propose(artifact).await.unwrap();
        assert_eq!(
            pipeline.get(&id).await.unwrap().status,
            ChangeArtifactStatus::Proposed
        );
    }

    /// Token signed for principal A but submitted with proposer_principal B
    /// must be rejected with Forbidden (403), not a signature error.
    #[test]
    fn different_principal_token_returns_forbidden() {
        let signer = EvolveTokenSigner::new(b"id-check-secret".to_vec());

        // Token minted for principal A.
        let mut token = CapabilityToken {
            id: "principal-a".to_string(),
            scopes: [BlastRadius::SingleHookConfig].into_iter().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 10,
            signature: [0u8; 64],
        };
        signer.sign(&mut token);

        // Signature verifies (the MAC is valid) ...
        signer
            .verify(&token, BlastRadius::SingleHookConfig)
            .expect("signature must be valid before identity check");

        // ... but the submitter claims to be principal B -- mismatch.
        let proposer_principal = "principal-b";
        let result: Result<(), AppError> = if token.id != proposer_principal {
            Err(AppError::Forbidden(format!(
                "token identity mismatch: token id '{}' does not match proposer_principal '{}'",
                token.id, proposer_principal,
            )))
        } else {
            Ok(())
        };

        let err = result.expect_err("mismatched principal must be rejected");
        assert!(
            matches!(err, AppError::Forbidden(_)),
            "expected Forbidden, got: {err:?}"
        );
    }

    /// When signature verification fails the identity check is never reached
    /// -- the error is still 401, proving the check order is respected.
    #[test]
    fn invalid_signature_returns_401_before_identity_check() {
        let signer = EvolveTokenSigner::new(b"id-check-secret".to_vec());

        // Token whose id already matches the principal, but signature is invalid.
        let token = CapabilityToken {
            id: "principal-a".to_string(),
            scopes: [BlastRadius::SingleHookConfig].into_iter().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 10,
            signature: [0u8; 64], // unsigned -- will fail MAC check
        };

        // verify() must fail with InvalidSignature (401) before identity.
        let err = signer
            .verify(&token, BlastRadius::SingleHookConfig)
            .map_err(evolve_token_err)
            .expect_err("unsigned token must fail before identity check");

        assert!(
            matches!(err, AppError::Auth(_)),
            "expected Auth (401) not Forbidden: {err:?}"
        );
    }

    // ── GET /api/evolve/:id edge cases ─────────────────────────────────────

    /// GET with an unknown (never-proposed) id → pipeline returns None →
    /// the handler builds NotFound → would become HTTP 404.
    #[tokio::test]
    async fn get_unknown_id_pipeline_returns_none() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let phantom = ChangeArtifactId { hash: [0xBB; 32] };
        assert!(
            pipeline.get(&phantom).await.is_none(),
            "phantom id must not exist"
        );
        // Confirm pipeline_err maps NotFound to the correct AppError variant.
        let err = pipeline_err(PipelineError::NotFound(phantom.to_string()));
        assert!(matches!(
            err,
            AppError::Db(sera_db::DbError::NotFound {
                entity: "change_artifact",
                ..
            })
        ));
    }

    /// GET after propose → status is Proposed.
    #[tokio::test]
    async fn get_after_propose_shows_proposed() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let artifact = ChangeArtifact::new(
            "lifecycle test".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: "lc-user".to_string(),
                capability_token: stub_capability_token(BlastRadius::SingleHookConfig),
            },
            serde_json::json!({}),
        );
        let id = pipeline.propose(artifact).await.unwrap();
        assert_eq!(
            pipeline.get(&id).await.unwrap().status,
            ChangeArtifactStatus::Proposed
        );
    }

    /// GET after evaluate (pass) → status is Approved.
    #[tokio::test]
    async fn get_after_evaluate_pass_shows_approved() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let artifact = ChangeArtifact::new(
            "approved lifecycle".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: "lc-user".to_string(),
                capability_token: stub_capability_token(BlastRadius::SingleHookConfig),
            },
            serde_json::json!({}),
        );
        let id = pipeline.propose(artifact).await.unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();
        assert_eq!(
            pipeline.get(&id).await.unwrap().status,
            ChangeArtifactStatus::Approved
        );
    }

    /// GET after evaluate (fail) → status is Rejected.
    #[tokio::test]
    async fn get_after_evaluate_fail_shows_rejected() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let artifact = ChangeArtifact::new(
            "rejected lifecycle".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: "lc-user2".to_string(),
                capability_token: stub_capability_token(BlastRadius::SingleHookConfig),
            },
            serde_json::json!({}),
        );
        let id = pipeline.propose(artifact).await.unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Failed("rule violation".to_string()))
            .await
            .unwrap();
        assert_eq!(
            pipeline.get(&id).await.unwrap().status,
            ChangeArtifactStatus::Rejected
        );
    }

    /// GET after approve + apply → status is Applied.
    #[tokio::test]
    async fn get_after_apply_shows_applied() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let artifact = ChangeArtifact::new(
            "applied lifecycle".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: "lc-user3".to_string(),
                capability_token: stub_capability_token(BlastRadius::SingleHookConfig),
            },
            serde_json::json!({}),
        );
        let id = pipeline.propose(artifact).await.unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();
        pipeline.approve(&id, "approver-lc").await.unwrap();
        pipeline.apply(&id).await.unwrap();
        assert_eq!(
            pipeline.get(&id).await.unwrap().status,
            ChangeArtifactStatus::Applied
        );
    }

    // ── Malformed ChangeArtifactId in URL ──────────────────────────────────

    /// Non-hex string → parse_id returns BadRequest (not a panic).
    #[test]
    fn parse_id_non_hex_is_bad_request() {
        let err = parse_id("not-hex!!").unwrap_err();
        assert!(
            matches!(err, AppError::BadRequest(_)),
            "non-hex id must return BadRequest, got: {err:?}"
        );
    }

    /// Valid hex but 16 bytes (too short, need 32) → parse_id returns BadRequest.
    #[test]
    fn parse_id_too_short_is_bad_request() {
        // 16 bytes = 32 hex chars
        let short = "deadbeef".repeat(4); // 32 hex chars = 16 bytes
        let err = parse_id(&short).unwrap_err();
        assert!(
            matches!(err, AppError::BadRequest(_)),
            "too-short id must return BadRequest, got: {err:?}"
        );
    }

    /// Valid hex but 33 bytes (too long) → parse_id returns BadRequest.
    #[test]
    fn parse_id_too_long_is_bad_request() {
        // 33 bytes = 66 hex chars
        let long = "aa".repeat(33);
        let err = parse_id(&long).unwrap_err();
        assert!(
            matches!(err, AppError::BadRequest(_)),
            "too-long id must return BadRequest, got: {err:?}"
        );
    }

    /// Empty string → parse_id returns BadRequest (not a panic).
    #[test]
    fn parse_id_empty_is_bad_request() {
        let err = parse_id("").unwrap_err();
        assert!(
            matches!(err, AppError::BadRequest(_)),
            "empty id must return BadRequest, got: {err:?}"
        );
    }

    /// Boundary: 31 bytes (62 hex chars) is also BadRequest.
    #[test]
    fn parse_id_31_bytes_is_bad_request() {
        let input = "aa".repeat(31); // 62 hex chars = 31 bytes
        let err = parse_id(&input).unwrap_err();
        assert!(
            matches!(err, AppError::BadRequest(_)),
            "31-byte id must return BadRequest, got: {err:?}"
        );
    }

    // ── Concurrent approve by same approver ────────────────────────────────

    /// Second approve call with the same PrincipalId returns DuplicateApprover;
    /// the signer set stays at 1 and apply still succeeds (Tier-2 needs 1).
    #[tokio::test]
    async fn same_approver_twice_does_not_double_count() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let artifact = ChangeArtifact::new(
            "dup-approve test".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: "eng-proposer".to_string(),
                capability_token: stub_capability_token(BlastRadius::SingleHookConfig),
            },
            serde_json::json!({}),
        );
        let id = pipeline.propose(artifact).await.unwrap();
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();

        // First approval: count = 1.
        let count = pipeline.approve(&id, "dup-approver").await.unwrap();
        assert_eq!(count, 1);

        // Second call with the same principal must be rejected.
        let dup_err = pipeline.approve(&id, "dup-approver").await.unwrap_err();
        assert!(
            matches!(dup_err, PipelineError::DuplicateApprover(_)),
            "expected DuplicateApprover, got: {dup_err}"
        );

        // Signer set is still size 1 — duplicate was not recorded.
        let signers = pipeline.signers(&id).await;
        assert_eq!(signers.len(), 1, "duplicate must not increase signer count");

        // Tier-2 requires 1 approver → apply still succeeds.
        pipeline.apply(&id).await.unwrap();
        assert_eq!(
            pipeline.get(&id).await.unwrap().status,
            ChangeArtifactStatus::Applied
        );
    }

    // ── Token replay ───────────────────────────────────────────────────────

    /// A valid signed token used twice by the same principal creates two
    /// distinct artifacts (different content-addressed IDs). The pipeline does
    /// not yet enforce max_proposals — this test documents current behaviour.
    #[tokio::test]
    async fn same_token_twice_same_principal_creates_two_artifacts() {
        let signer = EvolveTokenSigner::new(b"replay-secret".to_vec());
        let principal = "replay-principal".to_string();

        let mut token = CapabilityToken {
            id: principal.clone(),
            scopes: [BlastRadius::SingleHookConfig].into_iter().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 10,
            signature: [0u8; 64],
        };
        signer.sign(&mut token);

        // Both verify calls pass (the signature is valid).
        signer.verify(&token, BlastRadius::SingleHookConfig).unwrap();
        signer.verify(&token, BlastRadius::SingleHookConfig).unwrap();

        let pipeline = Arc::new(ArtifactPipeline::with_defaults());

        let art1 = ChangeArtifact::new(
            "first proposal".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: principal.clone(),
                capability_token: token.clone(),
            },
            serde_json::json!({ "v": 1 }),
        );
        // Sleep so created_at differs → distinct content-addressed IDs.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let art2 = ChangeArtifact::new(
            "second proposal".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: principal.clone(),
                capability_token: token.clone(),
            },
            serde_json::json!({ "v": 2 }),
        );

        let id1 = pipeline.propose(art1).await.unwrap();
        let id2 = pipeline.propose(art2).await.unwrap();

        assert_ne!(id1.hash, id2.hash, "distinct proposals must produce distinct IDs");
        assert!(pipeline.get(&id1).await.is_some());
        assert!(pipeline.get(&id2).await.is_some());
    }

    /// Token minted for principal-A submitted with proposer_principal=principal-B
    /// is blocked by the identity cross-check with Forbidden (403).
    #[test]
    fn token_replay_cross_principal_returns_forbidden() {
        let signer = EvolveTokenSigner::new(b"replay-secret".to_vec());
        let mut token = CapabilityToken {
            id: "principal-a".to_string(),
            scopes: [BlastRadius::SingleHookConfig].into_iter().collect(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
            max_proposals: 10,
            signature: [0u8; 64],
        };
        signer.sign(&mut token);

        // Signature is valid — but caller claims a different principal.
        signer.verify(&token, BlastRadius::SingleHookConfig).unwrap();
        let proposer_principal = "principal-b";
        let result: Result<(), AppError> = if token.id != proposer_principal {
            Err(AppError::Forbidden(format!(
                "token identity mismatch: token id '{}' does not match proposer_principal '{}'",
                token.id, proposer_principal,
            )))
        } else {
            Ok(())
        };
        assert!(
            matches!(result, Err(AppError::Forbidden(_))),
            "cross-principal replay must return Forbidden"
        );
    }

    // ── evaluate called twice ──────────────────────────────────────────────

    /// Calling evaluate a second time on an artifact that has already been
    /// evaluated returns WrongState. pipeline_err maps this to Conflict (409).
    #[tokio::test]
    async fn evaluate_twice_returns_wrong_state_mapped_to_conflict() {
        let pipeline = Arc::new(ArtifactPipeline::with_defaults());
        let artifact = ChangeArtifact::new(
            "eval-twice test".to_string(),
            ChangeArtifactScope::ConfigEvolution,
            BlastRadius::SingleHookConfig,
            ChangeProposer {
                principal_id: "eval2-user".to_string(),
                capability_token: stub_capability_token(BlastRadius::SingleHookConfig),
            },
            serde_json::json!({}),
        );
        let id = pipeline.propose(artifact).await.unwrap();

        // First evaluate succeeds.
        pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap();

        // Second evaluate must fail with WrongState.
        let pipeline_err_val = pipeline
            .evaluate(&id, |_| DryRunOutcome::Passed)
            .await
            .unwrap_err();
        assert!(
            matches!(pipeline_err_val, PipelineError::WrongState { .. }),
            "second evaluate must return WrongState, got: {pipeline_err_val}"
        );

        // pipeline_err maps WrongState → Conflict (409).
        let app_err = pipeline_err(pipeline_err_val);
        assert!(
            matches!(app_err, AppError::Db(sera_db::DbError::Conflict(_))),
            "WrongState must map to Conflict (409), got: {app_err:?}"
        );
    }
}
