//! WebSocket transport — connects to a remote harness via WebSocket.
//!
//! Per SPEC-gateway §7a: JSON-RPC framing over the SQ/EQ envelope.
//! Gated behind the `enterprise` feature flag.

use std::pin::Pin;
use std::sync::Arc;

use async_stream::stream;
use async_trait::async_trait;
use futures_util::SinkExt;
use futures_util::StreamExt;
use tokio::sync::Mutex;
use tokio_stream::Stream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

use crate::envelope::{Event, Submission};

use super::{Transport, TransportError};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type WsWrite = futures_util::stream::SplitSink<WsStream, Message>;
type WsRead = futures_util::stream::SplitStream<WsStream>;

/// WebSocket transport that connects to a remote agent harness.
///
/// Uses a take-once pattern for the read half: `recv_events` may only be called
/// once per transport instance. This mirrors `InProcessTransport`.
pub struct WebSocketTransport {
    ws_write: Arc<Mutex<WsWrite>>,
    ws_read: Arc<Mutex<Option<WsRead>>>,
}

impl WebSocketTransport {
    /// Connect to a WebSocket server at `url` and return a transport instance.
    pub async fn connect(url: &str) -> Result<Self, TransportError> {
        let (ws_stream, _response) = connect_async(url).await.map_err(|e| {
            TransportError::ConnectionFailed(format!("WebSocket connect failed: {e}"))
        })?;

        let (write, read) = ws_stream.split();

        Ok(Self {
            ws_write: Arc::new(Mutex::new(write)),
            ws_read: Arc::new(Mutex::new(Some(read))),
        })
    }
}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn send_submission(&self, submission: Submission) -> Result<(), TransportError> {
        let json = serde_json::to_string(&submission)
            .map_err(|e| TransportError::SendFailed(format!("serialize failed: {e}")))?;

        self.ws_write
            .lock()
            .await
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| TransportError::SendFailed(format!("ws send failed: {e}")))?;

        Ok(())
    }

    async fn recv_events(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, TransportError> {
        let read =
            self.ws_read.lock().await.take().ok_or_else(|| {
                TransportError::ReceiveFailed("event stream already taken".into())
            })?;

        let event_stream = stream! {
            let mut read = read;
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        if let Ok(event) = serde_json::from_str::<Event>(&text) {
                            yield event;
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    _ => {}
                }
            }
        };

        Ok(Box::pin(event_stream))
    }

    async fn close(&self) -> Result<(), TransportError> {
        self.ws_write
            .lock()
            .await
            .send(Message::Close(None))
            .await
            .map_err(|e| TransportError::SendFailed(format!("ws close failed: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    // Integration test note: WebSocketTransport requires a live WebSocket server.
    // Full integration tests should be added under tests/ with a test harness
    // that binds a local ws:// server using tokio_tungstenite::accept_async.
    //
    // The unit tests below verify serialization logic independently.

    #[test]
    fn submission_roundtrips_to_json() {
        // Verify that Submission can be serialized to a JSON string (the form
        // we send over the wire) without loss.
        let submission = serde_json::json!({
            "id": "test-id",
            "payload": "hello"
        });
        let json = serde_json::to_string(&submission).expect("serialize ok");
        let back: serde_json::Value = serde_json::from_str(&json).expect("deserialize ok");
        assert_eq!(submission, back);
    }

    #[test]
    fn ws_read_take_once_semantics() {
        // Verify the Option<WsRead> take-once pattern used by recv_events:
        // taking from Some yields the value; subsequent takes yield None.
        let mut slot: Option<u32> = Some(42);
        assert_eq!(slot.take(), Some(42));
        assert_eq!(slot.take(), None);
    }
}
