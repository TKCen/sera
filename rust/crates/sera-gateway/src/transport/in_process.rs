//! InProcess transport — mpsc channel pair for in-process agent runtimes.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, mpsc};
use tokio_stream::{Stream, wrappers::ReceiverStream};

use crate::envelope::{Event, Submission};

use super::{Transport, TransportError};

/// In-process transport using tokio mpsc channels.
pub struct InProcessTransport {
    submission_tx: mpsc::Sender<Submission>,
    event_rx: Arc<Mutex<Option<mpsc::Receiver<Event>>>>,
}

impl InProcessTransport {
    /// Create a new in-process transport pair.
    ///
    /// Returns (transport, submission_rx, event_tx) — the runtime end holds
    /// the rx/tx pair and processes submissions / emits events.
    pub fn new(buffer: usize) -> (Self, mpsc::Receiver<Submission>, mpsc::Sender<Event>) {
        let (submission_tx, submission_rx) = mpsc::channel(buffer);
        let (event_tx, event_rx) = mpsc::channel(buffer);

        let transport = Self {
            submission_tx,
            event_rx: Arc::new(Mutex::new(Some(event_rx))),
        };

        (transport, submission_rx, event_tx)
    }
}

#[async_trait]
impl Transport for InProcessTransport {
    async fn send_submission(&self, submission: Submission) -> Result<(), TransportError> {
        self.submission_tx
            .send(submission)
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))
    }

    async fn recv_events(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, TransportError> {
        let rx = self
            .event_rx
            .lock()
            .await
            .take()
            .ok_or(TransportError::ReceiveFailed(
                "event receiver already taken".into(),
            ))?;
        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    async fn close(&self) -> Result<(), TransportError> {
        // Dropping the sender will close the channel
        Ok(())
    }
}
