//! Shared application state passed to all handlers via axum State extractor.

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use sera_auth::JwtService;
use sera_config::core_config::CoreConfig;
use sera_config::providers::ProvidersConfig;
use sera_db::lane_queue::LaneQueue;
use sera_hooks::{ChainExecutor, HookRegistry};
use sera_meta::artifact_pipeline::ArtifactPipeline;
use sera_meta::constitutional::ConstitutionalRegistry;
use sera_telemetry::CentrifugoClient;
use sera_tools::sandbox::SandboxProvider;

use crate::db_backend::DbBackend;
use crate::envelope::GenerationMarker;
use crate::evolve_token::{EvolveTokenSigner, ProposalUsageStore};
use crate::harness_dispatch::HarnessRegistry;
use crate::kill_switch::KillSwitch;
use crate::services::schedule_service::ScheduleService;
use crate::session_store::SessionStore;
use crate::transcript_persist::TranscriptPersistence;

/// Shared application state.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    /// Pluggable database backend. Today's Postgres-only routes reach the
    /// underlying `sqlx::PgPool` via `db.pg_pool().expect(...)`; SQLite-backed
    /// deployments carry the same shape but surface the `SqliteDb` handle
    /// instead. See [`crate::db_backend`].
    pub db: Arc<dyn DbBackend>,
    pub config: Arc<CoreConfig>,
    pub jwt: Arc<JwtService>,
    pub providers: Arc<RwLock<ProvidersConfig>>,
    pub sandbox: Arc<dyn SandboxProvider>,
    pub providers_path: Option<String>,
    pub centrifugo: Option<Arc<CentrifugoClient>>,
    pub mcp_registry: Arc<RwLock<crate::routes::mcp::McpRegistry>>,
    pub schedule_svc: Arc<ScheduleService>,
    pub harness_registry: HarnessRegistry,
    pub plugin_registry: crate::harness_dispatch::PluginRegistry,
    pub queue_backend: Arc<dyn sera_queue::QueueBackend>,
    pub generation_marker: GenerationMarker,
    pub kill_switch: Arc<KillSwitch>,
    pub transcript_persistence: Arc<TranscriptPersistence>,
    /// Per-session lane queue for serialising inbound messages across channels
    /// (Discord, HTTP chat, API). Wraps [`LaneQueue`] in a tokio [`Mutex`] so
    /// async handlers can mutate the shared state.
    pub lane_queue: Arc<Mutex<LaneQueue>>,
    /// Self-evolution pipeline — backs the `/api/evolve/*` route handlers so
    /// propose → evaluate → approve → apply transitions are executed end to
    /// end against [`sera_meta::artifact_pipeline::ArtifactPipeline`].
    pub evolution_pipeline: Arc<ArtifactPipeline>,
    /// Constitutional-rule registry — consulted at
    /// [`sera_types::evolution::ConstitutionalEnforcementPoint::PreApproval`]
    /// inside `/api/evolve/evaluate/:id` to gate the dry-run on rule
    /// violations. Until sera-runtime exposes a ShadowSessionExecutor this
    /// provides the real "shadow replay" gate; production wiring will layer
    /// LLM-turn replay on top of the same registry.
    pub constitutional_registry: Arc<ConstitutionalRegistry>,
    /// Registry of in-process hook modules shared with the chain executor.
    pub hook_registry: Arc<HookRegistry>,
    /// Chain executor used by evolve route handlers to fire
    /// [`sera_types::hook::HookPoint::OnChangeArtifactProposed`] hook chains
    /// with `HookContext.change_artifact` populated end-to-end.
    pub chain_executor: Arc<ChainExecutor>,
    /// HMAC-SHA-512 signer used by `/api/evolve/propose` to verify the
    /// capability token submitted with each change-artifact proposal. See
    /// [`sera_gateway::evolve_token`] for the canonical-byte layout and the
    /// rationale behind keeping verification gateway-local rather than in
    /// `sera-auth`.
    pub evolve_token_signer: Arc<EvolveTokenSigner>,
    /// Proposal-usage store tracking how many proposals each capability-token
    /// id has consumed. Enforces
    /// [`sera_auth::CapabilityToken::max_proposals`] at the gateway
    /// layer. Backed by Postgres in production (restart-safe) and by the
    /// in-memory store in tests.
    pub proposal_usage: Arc<dyn ProposalUsageStore>,
    /// Submission envelope store — every agent-facing route appends a
    /// [`sera_gateway::envelope::Submission`] here before calling the
    /// underlying service. Backed by [`sera_gateway::session_store::InMemorySessionStore`]
    /// in the default boot path; swapped for the PartTable+git implementation
    /// when `sera-r9ed` lands.
    pub session_store: Arc<dyn SessionStore>,
}
