//! Stdio transport — spawns child process with NDJSON on stdin/stdout.
//!
//! Feature-gated behind `stdio`.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio_stream::Stream;

use crate::envelope::{Event, Submission};

use super::{Transport, TransportError};

/// Stdio transport that communicates via NDJSON over stdin/stdout of a child process.
pub struct StdioTransport {
    child: Arc<Mutex<Child>>,
}

impl StdioTransport {
    /// Spawn a child process for stdio transport.
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &std::collections::HashMap<String, String>,
    ) -> Result<Self, TransportError> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .envs(env.iter())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = cmd.spawn().map_err(|e| {
            TransportError::ConnectionFailed(format!("failed to spawn: {e}"))
        })?;

        Ok(Self {
            child: Arc::new(Mutex::new(child)),
        })
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send_submission(&self, submission: Submission) -> Result<(), TransportError> {
        let mut child = self.child.lock().await;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or(TransportError::SendFailed("stdin not available".into()))?;

        let mut json = serde_json::to_string(&submission)
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;
        json.push('\n');

        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))?;

        Ok(())
    }

    async fn recv_events(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, TransportError> {
        let mut child = self.child.lock().await;
        let stdout = child
            .stdout
            .take()
            .ok_or(TransportError::ReceiveFailed("stdout not available".into()))?;

        let reader = BufReader::new(stdout);
        let stream = async_stream::stream! {
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(event) = serde_json::from_str::<Event>(&line) {
                    yield event;
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn close(&self) -> Result<(), TransportError> {
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
        Ok(())
    }
}
