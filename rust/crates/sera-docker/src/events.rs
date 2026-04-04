//! Docker event listener that subscribes to container lifecycle events and publishes to Centrifugo.

use std::sync::Arc;

use bollard::models::EventMessage;
use bollard::system::EventsOptions;
use bollard::Docker;
use futures_util::StreamExt;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::error::DockerError;

/// Event types we care about
const MONITORED_ACTIONS: &[&str] = &["start", "stop", "die", "oom", "health_status"];

/// A processed Docker event ready for publishing
#[derive(Debug, Clone, serde::Serialize)]
pub struct DockerEvent {
    pub container_id: String,
    pub container_name: Option<String>,
    pub action: String,
    pub timestamp: i64,
    pub agent_name: Option<String>,
    pub instance_id: Option<String>,
}

/// Listens for Docker container events and publishes them to Centrifugo.
pub struct DockerEventListener {
    docker: Docker,
    centrifugo: Arc<sera_events::CentrifugoClient>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

impl DockerEventListener {
    /// Create a new Docker event listener.
    ///
    /// # Errors
    /// Returns `DockerError::Connection` if connection to local Docker daemon fails.
    pub fn new(centrifugo: Arc<sera_events::CentrifugoClient>) -> Result<Self, DockerError> {
        let docker = Docker::connect_with_local_defaults()
            .map_err(|e| DockerError::Connection(e.to_string()))?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Ok(Self {
            docker,
            centrifugo,
            shutdown_tx,
            shutdown_rx,
        })
    }

    /// Start listening for Docker events in a background task.
    ///
    /// Returns a JoinHandle for the spawned task. To stop listening, call `stop()`.
    pub fn start(&self) -> JoinHandle<()> {
        let docker = self.docker.clone();
        let centrifugo = self.centrifugo.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            let mut backoff_ms = 1000u64;

            loop {
                // Check shutdown
                if *shutdown_rx.borrow() {
                    tracing::info!("Docker event listener shutting down");
                    break;
                }

                let filters = std::collections::HashMap::from([(
                    "type".to_string(),
                    vec!["container".to_string()],
                )]);
                let options = EventsOptions::<String> {
                    filters,
                    ..Default::default()
                };

                let mut stream = docker.events(Some(options));

                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            tracing::info!("Docker event listener shutting down");
                            return;
                        }
                        event = stream.next() => {
                            match event {
                                Some(Ok(msg)) => {
                                    backoff_ms = 1000; // Reset backoff on success
                                    if let Some(docker_event) = Self::process_event(&msg) {
                                        let data = serde_json::to_value(&docker_event)
                                            .unwrap_or_default();
                                        let centrifugo = centrifugo.clone();
                                        tokio::spawn(async move {
                                            if let Err(e) = centrifugo.publish("docker:events", data).await {
                                                tracing::warn!("Failed to publish docker event: {e}");
                                            }
                                        });
                                    }
                                }
                                Some(Err(e)) => {
                                    tracing::error!("Docker event stream error: {e}");
                                    break; // Break inner loop to reconnect
                                }
                                None => {
                                    tracing::warn!("Docker event stream ended");
                                    break;
                                }
                            }
                        }
                    }
                }

                // Exponential backoff on reconnect
                tracing::info!(backoff_ms, "Reconnecting to Docker events...");
                tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(30_000);
            }
        })
    }

    /// Signal the listener to stop.
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Process a raw Docker event into our event type, filtering for monitored actions.
    fn process_event(msg: &EventMessage) -> Option<DockerEvent> {
        let action = msg.action.as_deref()?;

        // Check if this is a monitored action
        if !MONITORED_ACTIONS.iter().any(|a| action.starts_with(a)) {
            return None;
        }

        let actor = msg.actor.as_ref()?;
        let container_id = actor.id.clone()?;
        let attributes = actor.attributes.as_ref();

        Some(DockerEvent {
            container_id,
            container_name: attributes.and_then(|a| a.get("name").cloned()),
            action: action.to_string(),
            timestamp: msg.time.unwrap_or(0),
            agent_name: attributes.and_then(|a| a.get("sera.agent").cloned()),
            instance_id: attributes.and_then(|a| a.get("sera.instance").cloned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn mock_event(
        action: &str,
        container_id: &str,
        attributes: Option<HashMap<String, String>>,
    ) -> EventMessage {
        EventMessage {
            action: Some(action.to_string()),
            actor: Some(bollard::models::EventActor {
                id: Some(container_id.to_string()),
                attributes,
            }),
            time: Some(1234567890),
            ..Default::default()
        }
    }

    #[test]
    fn process_event_monitored_action() {
        let event = mock_event("start", "abc123", None);
        let result = DockerEventListener::process_event(&event);
        assert!(result.is_some());
        let docker_event = result.unwrap();
        assert_eq!(docker_event.container_id, "abc123");
        assert_eq!(docker_event.action, "start");
        assert_eq!(docker_event.timestamp, 1234567890);
    }

    #[test]
    fn process_event_unmonitored_action() {
        let event = mock_event("pull", "abc123", None);
        let result = DockerEventListener::process_event(&event);
        assert!(result.is_none());
    }

    #[test]
    fn process_event_with_attributes() {
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), "my-container".to_string());
        attrs.insert("sera.agent".to_string(), "my-agent".to_string());
        attrs.insert("sera.instance".to_string(), "inst-123".to_string());

        let event = mock_event("stop", "abc123", Some(attrs));
        let result = DockerEventListener::process_event(&event);
        assert!(result.is_some());
        let docker_event = result.unwrap();
        assert_eq!(docker_event.container_name, Some("my-container".to_string()));
        assert_eq!(docker_event.agent_name, Some("my-agent".to_string()));
        assert_eq!(docker_event.instance_id, Some("inst-123".to_string()));
    }

    #[test]
    fn process_event_missing_fields() {
        let event = EventMessage {
            action: Some("start".to_string()),
            actor: None,
            ..Default::default()
        };
        let result = DockerEventListener::process_event(&event);
        assert!(result.is_none());
    }

    #[test]
    fn process_event_no_action() {
        let event = EventMessage {
            actor: Some(bollard::models::EventActor {
                id: Some("abc123".to_string()),
                attributes: None,
            }),
            ..Default::default()
        };
        let result = DockerEventListener::process_event(&event);
        assert!(result.is_none());
    }

    #[test]
    fn process_event_health_status_variant() {
        let event = mock_event("health_status: healthy", "abc123", None);
        let result = DockerEventListener::process_event(&event);
        assert!(result.is_some());
    }

    #[test]
    fn process_event_no_container_id() {
        let event = EventMessage {
            action: Some("start".to_string()),
            actor: Some(bollard::models::EventActor {
                id: None,
                attributes: None,
            }),
            ..Default::default()
        };
        let result = DockerEventListener::process_event(&event);
        assert!(result.is_none());
    }
}
