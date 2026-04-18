//! Intercom types — Centrifugo pub/sub messaging.

use serde::{Deserialize, Serialize};

/// A message published via Centrifugo.
/// Maps from TS: IntercomMessage in intercom/types.ts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntercomMessage {
    pub channel: String,
    pub data: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<MessageSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_id: Option<String>,
}
