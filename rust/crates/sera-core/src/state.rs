//! Shared application state passed to all handlers via axum State extractor.

use std::sync::Arc;

use sera_auth::JwtService;
use sera_config::core_config::CoreConfig;
use sera_db::DbPool;

/// Shared application state.
#[derive(Clone)]
#[allow(dead_code)]
pub struct AppState {
    pub db: DbPool,
    pub config: Arc<CoreConfig>,
    pub jwt: Arc<JwtService>,
}
