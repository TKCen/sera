//! Secrets types — encrypted key-value store.

use serde::{Deserialize, Serialize};

/// A secret entry (value is always encrypted at rest in the DB).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretEntry {
    pub key: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}
