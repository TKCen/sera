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

impl AppServerTransport {
    /// Connect or spawn the underlying transport.
    ///
    /// `InProcess` cannot be reached this way — callers needing it must
    /// construct an [`in_process::InProcessTransport`] directly with paired
    /// channels, since the runtime end of the channel must be wired up by
    /// the same process.
    ///
    /// `Stdio` requires the `stdio` Cargo feature.
    pub async fn connect(&self) -> Result<Box<dyn Transport>, TransportError> {
        match self {
            #[cfg(feature = "stdio")]
            Self::Stdio { command, args, env } => {
                let t = stdio::StdioTransport::spawn(command, args, env).await?;
                Ok(Box::new(t))
            }
            #[cfg(not(feature = "stdio"))]
            Self::Stdio { .. } => Err(TransportError::ConnectionFailed(
                "stdio transport not enabled (build with --features stdio)".into(),
            )),
            Self::InProcess => Err(TransportError::ConnectionFailed(
                "in_process transport must be constructed via InProcessTransport::new() with paired channels".into(),
            )),
            Self::WebSocket { .. } | Self::Grpc { .. } | Self::WebhookBack { .. } => {
                Err(TransportError::ConnectionFailed(
                    "transport variant not yet wired".into(),
                ))
            }
            Self::Off => Err(TransportError::Closed),
        }
    }
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
