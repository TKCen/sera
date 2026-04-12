//! AppServerTransport — transport layer between gateway and agent runtimes.
//!
//! Per SPEC-gateway §7a: every harness MUST provide InProcess (compile-time contract).

pub mod in_process;

#[cfg(feature = "stdio")]
pub mod stdio;

#[cfg(feature = "enterprise")]
pub mod websocket;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use tokio_stream::Stream;

use crate::envelope::{Event, Submission};

/// Transport variants for connecting to agent runtimes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppServerTransport {
    InProcess,
    Stdio {
        command: String,
        args: Vec<String>,
        env: std::collections::HashMap<String, String>,
    },
    WebSocket {
        bind: String,
        tls: bool,
    },
    Grpc {
        endpoint: String,
        tls: bool,
    },
    WebhookBack {
        callback_base_url: String,
    },
    Off,
}

/// Errors from transport operations.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("send failed: {0}")]
    SendFailed(String),
    #[error("receive failed: {0}")]
    ReceiveFailed(String),
    #[error("transport closed")]
    Closed,
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
}

/// Transport trait for sending submissions and receiving events.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a submission to the runtime.
    async fn send_submission(&self, submission: Submission) -> Result<(), TransportError>;

    /// Receive events from the runtime as a stream.
    async fn recv_events(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, TransportError>;

    /// Close the transport.
    async fn close(&self) -> Result<(), TransportError>;
}

/// Configuration for transport, deserializable from agent manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    pub transport: AppServerTransport,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    300
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            transport: AppServerTransport::InProcess,
            timeout_secs: default_timeout(),
        }
    }
}
