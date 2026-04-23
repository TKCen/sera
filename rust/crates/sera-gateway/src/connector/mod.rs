//! Connector registry — external channel adapters (Discord, Slack, etc.).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

pub use sera_types::connector::ConnectorError;

/// Connector trait — delivers and receives messages from external channels.
#[async_trait]
pub trait Connector: Send + Sync {
    /// Deliver a message to the external channel.
    async fn deliver(&self, channel_id: &str, message: &str) -> Result<(), ConnectorError>;

    /// Human-readable name of this connector.
    fn name(&self) -> &str;
}

/// Registry of active connectors.
pub type ConnectorRegistry = Arc<RwLock<HashMap<String, Box<dyn Connector>>>>;

/// Create a new empty connector registry.
pub fn new_connector_registry() -> ConnectorRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}
