//! Delegation orchestrator — ties handoff definitions to subagent spawning.
//!
//! [`DelegationOrchestrator`] implements [`DelegationProtocol`] and is the
//! primary entry-point for agent-to-agent delegation at runtime.  It:
//!
//! - Validates delegation depth to prevent infinite loops.
//! - Checks the request target against the configured allowed-targets list.
//! - Converts a [`Handoff`] tool call into a [`DelegationRequest`].
//! - Drives the [`SubagentManager`] to spawn and track the delegated agent.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use tracing::{debug, warn};

use crate::handoff::{
    DelegationConfig, DelegationError, DelegationProtocol, DelegationRequest, DelegationResponse,
    Handoff,
};
use crate::subagent::{SubagentManager, SubagentStatus};

/// Orchestrator that implements [`DelegationProtocol`] by coordinating
/// [`Handoff`] definitions and a [`SubagentManager`].
pub struct DelegationOrchestrator {
    /// Runtime configuration (depth limit, timeouts, allowed targets).
    config: DelegationConfig,
    /// Subagent manager used to spawn and track delegated agents.
    manager: Arc<dyn SubagentManager>,
}

impl DelegationOrchestrator {
    /// Create a new orchestrator backed by `manager` with the given `config`.
    pub fn new(config: DelegationConfig, manager: Arc<dyn SubagentManager>) -> Self {
        Self { config, manager }
    }

    /// Create a new orchestrator with [`DelegationConfig::default`].
    pub fn with_defaults(manager: Arc<dyn SubagentManager>) -> Self {
        Self::new(DelegationConfig::default(), manager)
    }

    /// Convert a [`Handoff`] tool call into a [`DelegationRequest`].
    ///
    /// `tool_input` is the JSON value received in the tool-call arguments.
    /// `current_depth` is the caller's current delegation depth.
    pub fn handoff_to_request(
        &self,
        handoff: &Handoff,
        tool_input: serde_json::Value,
        current_depth: u32,
    ) -> DelegationRequest {
        DelegationRequest {
            target_agent_id: handoff.tool_name.clone(),
            task_description: handoff.tool_description.clone(),
            context: tool_input,
            input_filter: handoff.input_filter.clone(),
            timeout: self.config.default_timeout,
            depth: current_depth + 1,
        }
    }
}

#[async_trait]
impl DelegationProtocol for DelegationOrchestrator {
    async fn delegate(
        &self,
        request: DelegationRequest,
    ) -> Result<DelegationResponse, DelegationError> {
        // Depth guard.
        if request.depth > self.config.max_depth {
            warn!(
                depth = request.depth,
                max_depth = self.config.max_depth,
                target = %request.target_agent_id,
                "delegation rejected: max depth exceeded"
            );
            return Err(DelegationError::MaxDepthExceeded {
                depth: request.depth,
                max_depth: self.config.max_depth,
            });
        }

        // Allowed-targets guard.
        if !self.can_delegate_to(&request.target_agent_id).await {
            warn!(target = %request.target_agent_id, "delegation rejected: target not allowed");
            return Err(DelegationError::TargetNotAllowed {
                target: request.target_agent_id.clone(),
            });
        }

        debug!(
            target = %request.target_agent_id,
            depth = request.depth,
            "delegating task"
        );

        // Generate a unique session key for this delegation invocation.
        let session_key = format!(
            "delegation-{}-{}",
            request.target_agent_id,
            uuid::Uuid::new_v4()
        );

        // Spawn the subagent.
        let handle = self
            .manager
            .spawn(&request.target_agent_id, &session_key, request.context.clone())
            .await
            .map_err(|e| {
                DelegationError::Transport(anyhow::anyhow!(
                    "spawn failed for '{}': {}",
                    request.target_agent_id,
                    e
                ))
            })?;

        // Determine effective timeout.
        let timeout = request
            .timeout
            .or(self.config.default_timeout)
            .unwrap_or(std::time::Duration::from_secs(300));

        let start = Instant::now();
        let mut status_rx = handle.status_rx;

        // Poll status until terminal or timeout.
        loop {
            if start.elapsed() >= timeout {
                let elapsed_secs = start.elapsed().as_secs_f64();
                warn!(
                    target = %request.target_agent_id,
                    elapsed_secs,
                    "delegation timed out"
                );
                // Best-effort cancel.
                let _ = self.manager.cancel(&session_key).await;
                return Ok(DelegationResponse::Timeout { elapsed_secs });
            }

            // Wait for a status change with a short poll interval.
            let changed = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                status_rx.changed(),
            )
            .await;

            let status = status_rx.borrow().clone();

            match status {
                SubagentStatus::Completed => {
                    debug!(target = %request.target_agent_id, "delegation completed");
                    return Ok(DelegationResponse::Success {
                        output: serde_json::Value::Null,
                        session_id: session_key,
                    });
                }
                SubagentStatus::Failed => {
                    debug!(target = %request.target_agent_id, "delegation failed");
                    return Ok(DelegationResponse::Error {
                        message: format!(
                            "subagent '{}' failed",
                            request.target_agent_id
                        ),
                        detail: None,
                    });
                }
                SubagentStatus::Cancelled => {
                    debug!(target = %request.target_agent_id, "delegation cancelled");
                    return Ok(DelegationResponse::Error {
                        message: format!(
                            "subagent '{}' was cancelled",
                            request.target_agent_id
                        ),
                        detail: None,
                    });
                }
                SubagentStatus::Spawning | SubagentStatus::Running => {
                    // Still in progress; if the watch channel closed without a
                    // terminal status, treat it as a failure.
                    if let Ok(Err(_)) = changed {
                        return Ok(DelegationResponse::Error {
                            message: format!(
                                "subagent '{}' status channel closed unexpectedly",
                                request.target_agent_id
                            ),
                            detail: None,
                        });
                    }
                    // Otherwise continue polling.
                }
            }
        }
    }

    async fn can_delegate_to(&self, target_agent_id: &str) -> bool {
        match &self.config.allowed_targets {
            // No restriction — all targets are allowed.
            None => true,
            Some(allowed) => allowed.contains(target_agent_id),
        }
    }

    async fn list_available_agents(&self) -> Vec<String> {
        match &self.config.allowed_targets {
            Some(allowed) => {
                let mut ids: Vec<String> = allowed.iter().cloned().collect();
                ids.sort();
                ids
            }
            // Without an explicit allow-list we cannot enumerate all agents;
            // return the currently active ones as a best-effort list.
            None => self.manager.list_active().await,
        }
    }
}
