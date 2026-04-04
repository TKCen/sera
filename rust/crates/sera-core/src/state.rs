//! Shared application state passed to all handlers via axum State extractor.

use std::sync::Arc;
use tokio::sync::RwLock;

use sera_auth::JwtService;
use sera_config::core_config::CoreConfig;
use sera_config::providers::ProvidersConfig;
use sera_db::DbPool;
use sera_docker::ContainerManager;
use sera_events::CentrifugoClient;

/// Shared application state.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub db: DbPool,
    pub config: Arc<CoreConfig>,
    pub jwt: Arc<JwtService>,
    pub providers: Arc<RwLock<ProvidersConfig>>,
    pub docker: Arc<ContainerManager>,
    pub providers_path: Option<String>,
    pub centrifugo: Option<Arc<CentrifugoClient>>,
    pub mcp_registry: Arc<RwLock<crate::routes::mcp::McpRegistry>>,
}
