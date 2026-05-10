//! BYOH dispatch interceptor (sera-plcv M1 AC2, production-path slice).
//!
//! Pattern A only — Pattern B sessions do not share a tool catalog with the
//! gateway, so this module's contract does not apply there.
//!
//! ## What this is
//!
//! A gateway-owned transport decorator that applies
//! [`ByohCatalogRegistry::prepare_outbound_call`] to every
//! [`EventMsg::ToolCallBegin`] frame as it crosses the Stdio/NDJSON seam from a
//! BYOH harness. When the registry decides the harness's emitted name must be
//! suppressed and forwarded as a canonical (e.g. Hermes `Read` →
//! `read_file`), the interceptor rewrites the `tool` field on the event
//! *before* it reaches any consumer.
//!
//! This is the production-path counterpart to the in-process composition
//! probe pinned by [`hermes_pattern_a_stdio_dispatch`]: that test consults the
//! registry from inside the test loop, which only proves the contract is
//! consultable. This module proves the contract is *automatically* applied at
//! the transport seam, regardless of what the consumer remembers to do.
//!
//! ## What this is not
//!
//! - Not a registration channel. Registration uses the existing canonical
//!   seam, [`ByohCatalogRegistry::register_harness`]. A wire-level
//!   `Op::Register(RegisterOp::ByohCatalog)` variant is a follow-up that
//!   requires a public protocol change; until then, the gateway pre-registers
//!   harnesses out-of-band.
//! - Not a harness-side enforcer. The harness is still responsible for not
//!   invoking its own internal implementation when the catalog says
//!   suppressed; this interceptor only guarantees the gateway sees the
//!   canonical name.
//! - Not a strict failure mode. If the registry returns an error
//!   (unregistered harness, unknown tool), the interceptor passes the frame
//!   through unchanged. Hard failure on unknown harnesses belongs with the
//!   downstream dispatcher.
//!
//! [`hermes_pattern_a_stdio_dispatch`]: ../../tests/hermes_pattern_a_stdio_dispatch.rs

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_stream::{Stream, StreamExt};

use crate::byoh_registration::{ByohCatalogRegistry, OutboundCallDecision};
use crate::envelope::{Event, EventMsg, Submission};
use crate::transport::{Transport, TransportError};

/// Rewrite a single [`Event`] using the registry's outbound-call decision for
/// the given `harness_id`.
///
/// When the event is a [`EventMsg::ToolCallBegin`] *and* the registry says
/// `SuppressInternalAndForward { canonical }` *and* the canonical name
/// differs from the emitted name, returns a new [`Event`] with the
/// `tool` field replaced. Otherwise returns the input unchanged.
///
/// All other [`OutboundCallDecision`] outcomes and all
/// [`crate::byoh_registration::DispatchError`] cases pass through. See module
/// docs for the rationale.
pub fn intercept_outbound_event(
    registry: &ByohCatalogRegistry,
    harness_id: &str,
    mut event: Event,
) -> Event {
    let canonical_rewrite: Option<String> = match &event.msg {
        EventMsg::ToolCallBegin { tool, .. } => match registry
            .prepare_outbound_call(harness_id, tool)
        {
            Ok(OutboundCallDecision::SuppressInternalAndForward { canonical })
                if canonical != *tool =>
            {
                Some(canonical)
            }
            _ => None,
        },
        _ => None,
    };

    if let Some(canonical) = canonical_rewrite
        && let EventMsg::ToolCallBegin { tool, .. } = &mut event.msg
    {
        *tool = canonical;
    }

    event
}

/// Transport decorator that wraps an inner [`Transport`] reading from a
/// Pattern A BYOH harness and applies [`intercept_outbound_event`] to every
/// frame in `recv_events`. `send_submission` and `close` are forwarded
/// untouched.
pub struct ByohInterceptingTransport {
    inner: Box<dyn Transport>,
    registry: Arc<ByohCatalogRegistry>,
    harness_id: String,
}

impl ByohInterceptingTransport {
    /// Wrap `inner` so its outbound `ToolCallBegin` frames are rewritten via
    /// `registry` for the given `harness_id`.
    pub fn new(
        inner: Box<dyn Transport>,
        registry: Arc<ByohCatalogRegistry>,
        harness_id: impl Into<String>,
    ) -> Self {
        Self {
            inner,
            registry,
            harness_id: harness_id.into(),
        }
    }
}

#[async_trait]
impl Transport for ByohInterceptingTransport {
    async fn send_submission(&self, submission: Submission) -> Result<(), TransportError> {
        self.inner.send_submission(submission).await
    }

    async fn recv_events(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = Event> + Send>>, TransportError> {
        let inner_stream = self.inner.recv_events().await?;
        let registry = Arc::clone(&self.registry);
        let harness_id = self.harness_id.clone();
        let mapped = inner_stream
            .map(move |evt| intercept_outbound_event(&registry, &harness_id, evt));
        Ok(Box::pin(mapped))
    }

    async fn close(&self) -> Result<(), TransportError> {
        self.inner.close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::byoh_catalog::{GatewayToolDecl, HarnessCatalog, HarnessToolDecl};
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    fn h(name: &str) -> HarnessToolDecl {
        HarnessToolDecl {
            name: name.to_string(),
            description: format!("harness {name}"),
            schema: json!({"type": "object"}),
        }
    }

    fn g(name: &str, aliases: &[&str]) -> GatewayToolDecl {
        GatewayToolDecl {
            name: name.to_string(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            description: format!("gateway {name}"),
            schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }
    }

    fn tool_call_begin(tool: &str) -> Event {
        Event {
            id: Uuid::nil(),
            submission_id: Uuid::nil(),
            msg: EventMsg::ToolCallBegin {
                turn_id: Uuid::nil(),
                call_id: "call-1".into(),
                tool: tool.to_string(),
                arguments: json!({"path": "/etc/passwd"}),
            },
            trace: Default::default(),
            timestamp: Utc::now(),
            parent_session_key: None,
            parent_task_id: None,
        }
    }

    fn registered_registry() -> ByohCatalogRegistry {
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
        registry
            .register_harness(HarnessCatalog {
                harness_id: "hermes-acp".into(),
                tools: vec![h("Read"), h("Bash")],
            })
            .expect("registration ok");
        registry
    }

    #[test]
    fn aliased_tool_call_begin_is_rewritten_to_canonical() {
        let registry = registered_registry();
        let evt = intercept_outbound_event(&registry, "hermes-acp", tool_call_begin("Read"));
        match evt.msg {
            EventMsg::ToolCallBegin { tool, arguments, .. } => {
                assert_eq!(tool, "read_file", "alias must be rewritten");
                assert_eq!(arguments["path"], "/etc/passwd", "args untouched");
            }
            other => panic!("expected ToolCallBegin, got {other:?}"),
        }
    }

    #[test]
    fn passthrough_tool_call_begin_is_unchanged() {
        let registry = registered_registry();
        let evt = intercept_outbound_event(&registry, "hermes-acp", tool_call_begin("Bash"));
        match evt.msg {
            EventMsg::ToolCallBegin { tool, .. } => assert_eq!(tool, "Bash"),
            other => panic!("expected ToolCallBegin, got {other:?}"),
        }
    }

    #[test]
    fn canonical_name_is_left_unchanged() {
        // The harness emits the canonical itself — no rewrite needed.
        let registry = registered_registry();
        let evt = intercept_outbound_event(&registry, "hermes-acp", tool_call_begin("read_file"));
        match evt.msg {
            EventMsg::ToolCallBegin { tool, .. } => assert_eq!(tool, "read_file"),
            other => panic!("expected ToolCallBegin, got {other:?}"),
        }
    }

    #[test]
    fn unregistered_harness_passes_through_unchanged() {
        // Documented graceful path: an unregistered harness is the
        // dispatcher's problem, not the interceptor's.
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
        let evt = intercept_outbound_event(&registry, "ghost", tool_call_begin("Read"));
        match evt.msg {
            EventMsg::ToolCallBegin { tool, .. } => assert_eq!(tool, "Read"),
            other => panic!("expected ToolCallBegin, got {other:?}"),
        }
    }

    #[test]
    fn unknown_tool_passes_through_unchanged() {
        let registry = registered_registry();
        let evt = intercept_outbound_event(&registry, "hermes-acp", tool_call_begin("Mystery"));
        match evt.msg {
            EventMsg::ToolCallBegin { tool, .. } => assert_eq!(tool, "Mystery"),
            other => panic!("expected ToolCallBegin, got {other:?}"),
        }
    }

    #[test]
    fn non_tool_call_event_passes_through_unchanged() {
        let registry = registered_registry();
        let evt = Event {
            id: Uuid::nil(),
            submission_id: Uuid::nil(),
            msg: EventMsg::StreamingDelta {
                delta: "hello".into(),
            },
            trace: Default::default(),
            timestamp: Utc::now(),
            parent_session_key: None,
            parent_task_id: None,
        };
        let after = intercept_outbound_event(&registry, "hermes-acp", evt);
        match after.msg {
            EventMsg::StreamingDelta { delta } => assert_eq!(delta, "hello"),
            other => panic!("expected StreamingDelta, got {other:?}"),
        }
    }
}
