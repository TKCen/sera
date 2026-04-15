//! Shared application state passed to all handlers via axum State extractor.

use std::sync::Arc;
use tokio::sync::RwLock;

use sera_auth::JwtService;
use sera_config::core_config::CoreConfig;
use sera_config::providers::ProvidersConfig;
use sera_db::DbPool;
use sera_events::CentrifugoClient;
use sera_tools::sandbox::SandboxProvider;

use sera_gateway::envelope::GenerationMarker;
use sera_gateway::harness_dispatch::HarnessRegistry;
use sera_gateway::kill_switch::KillSwitch;
use sera_gateway::transcript_persist::TranscriptPersistence;
use crate::services::schedule_service::ScheduleService;

/// Shared application state.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub db: DbPool,
    pub config: Arc<CoreConfig>,
    pub jwt: Arc<JwtService>,
    pub providers: Arc<RwLock<ProvidersConfig>>,
    pub sandbox: Arc<dyn SandboxProvider>,
    pub providers_path: Option<String>,
    pub centrifugo: Option<Arc<CentrifugoClient>>,
    pub mcp_registry: Arc<RwLock<crate::routes::mcp::McpRegistry>>,
    pub schedule_svc: Arc<ScheduleService>,
    pub harness_registry: HarnessRegistry,
    pub plugin_registry: sera_gateway::harness_dispatch::PluginRegistry,
    pub queue_backend: Arc<dyn sera_queue::QueueBackend>,
    pub generation_marker: GenerationMarker,
    pub kill_switch: Arc<KillSwitch>,
    pub transcript_persistence: Arc<TranscriptPersistence>,
}
