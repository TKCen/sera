//! Delegation tools — `delegation_spawn`, `delegation_list`, `delegation_yield`, `delegation_send`.
//!
//! These tools expose governed delegation primitives landed in bead
//! sera-a1u. Together they let an agent:
//!
//! 1. `delegation_spawn` — create a named delegation under its own agent id,
//!    returning a `delegation_id` the parent can refer to later.
//! 2. `delegation_list` — inspect known delegations and pending-yield counts.
//! 3. `delegation_yield` — pause the current turn and await the next event
//!    emitted by a specific delegation. Resumes with the event as a tool result.
//!    Blocks up to `timeout_secs` (default 120s).
//! 4. `delegation_send` — send a message into a named delegation
//!    (fire-and-forget unless paired with a yield).
//!
//! All four implement the native `Tool` trait (bead sera-ttrm-5) with
//! appropriate risk levels. Child execution itself flows through the existing
//! `SubagentManager` + `DelegationBus`; this module only provides the tool
//! surface.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use sera_types::tool::{
    ExecutionTarget, FunctionParameters, ParameterSchema, RiskLevel, Tool, ToolContext, ToolError,
    ToolInput, ToolMetadata, ToolOutput, ToolSchema, ToolScope,
};

use crate::delegation_bus::{ChildSessionMeta, DelegationBus, DelegationEvent};

/// Default wall-clock timeout for `delegation_yield` when the caller omits
/// `timeout_secs`. Matches the ~2-minute "long think" budget used by the
/// default LLM timeout.
const DEFAULT_YIELD_TIMEOUT_SECS: u64 = 120;

// ── delegation_spawn ───────────────────────────────────────────────────────────

/// Spawn a named delegation under the current agent.
///
/// Arguments:
/// - `name` (string, required) — caller-supplied delegation name. Used
///   verbatim as the `delegation_id` the caller will pass to
///   `delegation_yield` / `delegation_send`.
/// - `prompt` (string, required) — initial prompt for the child.
/// - `agent_template` (string, optional) — template to instantiate the child
///   with. When omitted, the child inherits the parent's template.
///
/// Returns a JSON object `{ "delegation_id": ..., "spawned": true }`.
///
/// This MVP registers the child with the [`DelegationBus`] but does not
/// actually drive a full subagent process — that is handled by the runtime's
/// existing spawn path (sera-runtime::subagent) and layered on top.
pub struct DelegationSpawnTool {
    bus: DelegationBus,
}

impl DelegationSpawnTool {
    /// Build the tool bound to a shared delegation bus.
    pub fn new(bus: DelegationBus) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl Tool for DelegationSpawnTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "delegation_spawn".to_string(),
            description:
                "Register a named delegation under the current agent. This records the delegation and returns a delegation_id for delegation_yield / delegation_send; it does not by itself execute the child task."
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Execute,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["delegation".to_string(), "subagent".to_string()],
            // sera-u4gj: spawning a child session under another agent is
            // sub-agent dispatch — `Agent` scope.
            scope: ToolScope::Agent,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "name".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Caller-supplied delegation name (used as the delegation_id).".to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "prompt".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Initial prompt for the child session.".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "agent_template".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some(
                    "Optional agent template to instantiate the child with.".to_string(),
                ),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["name".to_string(), "prompt".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let name = input.arguments["name"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'name'".to_string()))?;
        let prompt = input.arguments["prompt"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'prompt'".to_string()))?;
        let agent_template = input.arguments["agent_template"].as_str();

        // The delegation_id is the caller-supplied name — stable, so the
        // parent can later refer to it. If a name collision occurs we still
        // overwrite the old meta: parents re-spawning with the same name
        // effectively re-bind the child.
        let session_id = name.to_string();

        let parent_agent_id = ctx.principal.id.0.clone();
        let child_agent_id = agent_template
            .map(str::to_owned)
            .unwrap_or_else(|| parent_agent_id.clone());

        self.bus.register_session(
            session_id.clone(),
            ChildSessionMeta {
                parent_agent_id,
                child_agent_id: child_agent_id.clone(),
                initial_prompt: prompt.to_string(),
            },
        );

        let result = serde_json::json!({
            "delegation_id": session_id,
            "spawned": true,
            "child_agent_id": child_agent_id,
            "execution_started": false,
            "status": "registered_only",
            "note": "delegation_spawn registered the delegation but did not execute the child task in this MVP; do not call delegation_spawn again for the same request unless the operator asks for another delegation",
        });
        Ok(ToolOutput::success(result.to_string()))
    }
}

// ── delegation_list ────────────────────────────────────────────────────────────

/// List known delegations under this runtime's delegation bus.
pub struct DelegationListTool {
    bus: DelegationBus,
}

impl DelegationListTool {
    /// Build the tool bound to a shared delegation bus.
    pub fn new(bus: DelegationBus) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl Tool for DelegationListTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "delegation_list".to_string(),
            description:
                "List known delegations and pending-yield counts for this runtime."
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["delegation".to_string(), "session".to_string()],
            scope: ToolScope::Read,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "limit".to_string(),
            ParameterSchema {
                schema_type: "integer".to_string(),
                description: Some(
                    "Maximum number of delegations to return (default 100, max 500).".to_string(),
                ),
                enum_values: None,
                default: Some(serde_json::json!(100)),
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec![],
            },
        }
    }

    async fn execute(&self, input: ToolInput, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let limit = input.arguments["limit"].as_u64().unwrap_or(100).min(500) as usize;
        let delegations = self
            .bus
            .list_sessions()
            .into_iter()
            .take(limit)
            .map(|snapshot| {
                serde_json::json!({
                    "delegation_id": snapshot.session_id,
                    "parent_agent_id": snapshot.parent_agent_id,
                    "child_agent_id": snapshot.child_agent_id,
                    "initial_prompt": snapshot.initial_prompt,
                    "pending_subscribers": snapshot.pending_subscribers,
                })
            })
            .collect::<Vec<_>>();
        let count = delegations.len();
        tracing::info!(
            tool = "delegation_list",
            count,
            limit,
            "delegation_list executed"
        );
        let body = serde_json::json!({
            "count": count,
            "delegations": delegations,
        });
        Ok(ToolOutput::success(body.to_string()))
    }
}

// ── delegation_yield ───────────────────────────────────────────────────────────

/// Pause the current turn and await the next event from a named delegation.
/// The event is returned as the tool result.
///
/// Arguments:
/// - `delegation_id` (string, required) — delegation id previously returned
///   by `delegation_spawn`.
/// - `timeout_secs` (number, optional, default 120) — wall-clock timeout.
pub struct DelegationYieldTool {
    bus: DelegationBus,
}

impl DelegationYieldTool {
    /// Build the tool bound to a shared delegation bus.
    pub fn new(bus: DelegationBus) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl Tool for DelegationYieldTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "delegation_yield".to_string(),
            description:
                "Pause the current turn and await the next event emitted by a specific child session. Returns the event as a tool result."
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Read,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["delegation".to_string()],
            // sera-u4gj: yielding only observes the next bus event — the
            // child session must already exist (gated by `delegation_spawn`),
            // so this surface is read-only on the parent side.
            scope: ToolScope::Read,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "delegation_id".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Delegation id (from delegation_spawn).".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "timeout_secs".to_string(),
            ParameterSchema {
                schema_type: "integer".to_string(),
                description: Some(
                    "Wall-clock timeout in seconds (default 120).".to_string(),
                ),
                enum_values: None,
                default: Some(serde_json::json!(DEFAULT_YIELD_TIMEOUT_SECS)),
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["delegation_id".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = input.arguments["delegation_id"]
            .as_str()
            .or_else(|| input.arguments["session_id"].as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing 'delegation_id'".to_string()))?;
        let timeout_secs = input.arguments["timeout_secs"]
            .as_u64()
            .unwrap_or(DEFAULT_YIELD_TIMEOUT_SECS);

        let rx = self
            .bus
            .subscribe_next(session_id)
            .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

        match tokio::time::timeout(Duration::from_secs(timeout_secs), rx).await {
            Ok(Ok(event)) => {
                let body = serde_json::json!({
                    "delegation_id": session_id,
                    "event": event,
                });
                Ok(ToolOutput::success(body.to_string()))
            }
            Ok(Err(_)) => Err(ToolError::ExecutionFailed(format!(
                "delegation bus closed subscriber for session '{session_id}'"
            ))),
            Err(_) => Err(ToolError::Timeout),
        }
    }
}

// ── delegation_send ────────────────────────────────────────────────────────────

/// Send a message into a named delegation. Fire-and-forget — the caller
/// must pair this with a [`DelegationYieldTool`] call to observe the child's
/// response.
///
/// Arguments:
/// - `delegation_id` (string, required).
/// - `message` (string, required) — the message to deliver.
///
/// Returns `{ "delivered": true, "subscribers_notified": N }` on success.
pub struct DelegationSendTool {
    bus: DelegationBus,
}

impl DelegationSendTool {
    /// Build the tool bound to a shared delegation bus.
    pub fn new(bus: DelegationBus) -> Self {
        Self { bus }
    }
}

#[async_trait]
impl Tool for DelegationSendTool {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "delegation_send".to_string(),
            description:
                "Send a message into a named delegation. Fire-and-forget unless paired with delegation_yield."
                    .to_string(),
            version: "1.0.0".to_string(),
            author: None,
            risk_level: RiskLevel::Write,
            execution_target: ExecutionTarget::InProcess,
            tags: vec!["delegation".to_string()],
            // sera-u4gj: delivering a message into another agent's session
            // is cross-agent communication — `Agent` scope.
            scope: ToolScope::Agent,
        }
    }

    fn schema(&self) -> ToolSchema {
        let mut properties: HashMap<String, ParameterSchema> = HashMap::new();
        properties.insert(
            "delegation_id".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Target delegation id.".to_string()),
                enum_values: None,
                default: None,
            },
        );
        properties.insert(
            "message".to_string(),
            ParameterSchema {
                schema_type: "string".to_string(),
                description: Some("Message content to deliver.".to_string()),
                enum_values: None,
                default: None,
            },
        );
        ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties,
                required: vec!["delegation_id".to_string(), "message".to_string()],
            },
        }
    }

    async fn execute(
        &self,
        input: ToolInput,
        _ctx: ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let session_id = input.arguments["delegation_id"]
            .as_str()
            .or_else(|| input.arguments["session_id"].as_str())
            .ok_or_else(|| ToolError::InvalidInput("Missing 'delegation_id'".to_string()))?;
        let message = input.arguments["message"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("Missing 'message'".to_string()))?;

        if !self.bus.is_known(session_id) {
            return Err(ToolError::InvalidInput(format!(
                "unknown session '{session_id}' — did you call delegation_spawn first?"
            )));
        }

        // `delegation_send` models an incoming message *to* the child by
        // publishing a `MessageEmitted` event on the bus. In an MVP without
        // a real child process this is what a yield-side caller will
        // observe; in a richer deployment the child process itself
        // republishes its reply.
        let delivered = self.bus.publish(
            session_id,
            DelegationEvent::MessageEmitted {
                content: message.to_string(),
            },
        );

        let body = serde_json::json!({
            "delivered": true,
            "subscribers_notified": delivered,
        });
        Ok(ToolOutput::success(body.to_string()))
    }
}

// ── Registration helper ─────────────────────────────────────────────────────

/// Build and return the four delegation tools as a tuple, each bound to the
/// same [`DelegationBus`]. Used by `TraitToolRegistry::with_delegation`.
pub fn build_delegation_tools(
    bus: DelegationBus,
) -> (
    DelegationSpawnTool,
    DelegationListTool,
    DelegationYieldTool,
    DelegationSendTool,
) {
    (
        DelegationSpawnTool::new(bus.clone()),
        DelegationListTool::new(bus.clone()),
        DelegationYieldTool::new(bus.clone()),
        DelegationSendTool::new(bus),
    )
}

// Helper so downstream consumers (runtime main.rs, gateway wiring) can pull
// the bus through a single Arc without juggling clones directly.
impl DelegationSpawnTool {
    /// Return a clone of the bus this tool is bound to.
    pub fn bus(&self) -> DelegationBus {
        self.bus.clone()
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use sera_types::principal::{PrincipalId, PrincipalKind, PrincipalRef};
    use sera_types::tool::{
        AuditHandle, CredentialBag, SessionRef, ToolContext, ToolPolicy, ToolProfile,
    };

    fn make_ctx(principal_id: &str) -> ToolContext {
        ToolContext {
            session: SessionRef::new("parent-session"),
            principal: PrincipalRef {
                id: PrincipalId(principal_id.to_string()),
                kind: PrincipalKind::Agent,
            },
            credentials: CredentialBag::new(),
            policy: ToolPolicy::from_profile(ToolProfile::Full),
            audit_handle: AuditHandle {
                trace_id: "trace-1".to_string(),
                span_id: "span-1".to_string(),
            },
            ..ToolContext::default()
        }
    }

    fn mk_input(name: &str, args: serde_json::Value) -> ToolInput {
        ToolInput {
            name: name.to_string(),
            arguments: args,
            call_id: "call-test".to_string(),
        }
    }

    #[tokio::test]
    async fn delegation_spawn_registers_with_bus() {
        let bus = DelegationBus::new();
        let tool = DelegationSpawnTool::new(bus.clone());

        let input = mk_input(
            "delegation_spawn",
            serde_json::json!({
                "name": "child-1",
                "prompt": "research X",
                "agent_template": "researcher",
            }),
        );
        let out = tool.execute(input, make_ctx("parent-007")).await.unwrap();
        assert!(!out.is_error);

        // The response must contain the session_id we passed in.
        let parsed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(parsed["delegation_id"], "child-1");
        assert_eq!(parsed["spawned"], true);
        assert_eq!(parsed["child_agent_id"], "researcher");

        // The bus must know the session with correct parent/child linkage.
        let meta = bus.session_meta("child-1").expect("session registered");
        assert_eq!(meta.parent_agent_id, "parent-007");
        assert_eq!(meta.child_agent_id, "researcher");
        assert_eq!(meta.initial_prompt, "research X");
    }

    #[tokio::test]
    async fn delegation_spawn_inherits_parent_template_when_omitted() {
        let bus = DelegationBus::new();
        let tool = DelegationSpawnTool::new(bus.clone());

        let input = mk_input(
            "delegation_spawn",
            serde_json::json!({ "name": "child-2", "prompt": "hi" }),
        );
        tool.execute(input, make_ctx("parent-xyz")).await.unwrap();
        let meta = bus.session_meta("child-2").unwrap();
        assert_eq!(meta.child_agent_id, "parent-xyz");
    }

    #[tokio::test]
    async fn delegation_spawn_requires_name_and_prompt() {
        let bus = DelegationBus::new();
        let tool = DelegationSpawnTool::new(bus);

        let no_name = mk_input("delegation_spawn", serde_json::json!({ "prompt": "p" }));
        let err = tool.execute(no_name, make_ctx("a")).await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));

        let no_prompt = mk_input("delegation_spawn", serde_json::json!({ "name": "c" }));
        let err = tool
            .execute(no_prompt, make_ctx("a"))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn delegation_list_reports_known_child_sessions() {
        let bus = DelegationBus::new();
        let spawn = DelegationSpawnTool::new(bus.clone());
        let list = DelegationListTool::new(bus.clone());

        spawn
            .execute(
                mk_input(
                    "delegation_spawn",
                    serde_json::json!({
                        "name": "child-list",
                        "prompt": "inspect me",
                        "agent_template": "helper",
                    }),
                ),
                make_ctx("parent-list"),
            )
            .await
            .unwrap();
        let _rx = bus.subscribe_next("child-list").unwrap();

        let out = list
            .execute(mk_input("delegation_list", serde_json::json!({})), make_ctx("parent-list"))
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(parsed["count"], 1);
        assert_eq!(parsed["delegations"][0]["delegation_id"], "child-list");
        assert_eq!(parsed["delegations"][0]["parent_agent_id"], "parent-list");
        assert_eq!(parsed["delegations"][0]["child_agent_id"], "helper");
        assert_eq!(parsed["delegations"][0]["initial_prompt"], "inspect me");
        assert_eq!(parsed["delegations"][0]["pending_subscribers"], 1);
    }

    #[tokio::test]
    async fn delegation_yield_receives_next_event() {
        let bus = DelegationBus::new();
        let spawn = DelegationSpawnTool::new(bus.clone());
        let yield_tool = DelegationYieldTool::new(bus.clone());

        spawn
            .execute(
                mk_input(
                    "delegation_spawn",
                    serde_json::json!({ "name": "child-y", "prompt": "p" }),
                ),
                make_ctx("parent"),
            )
            .await
            .unwrap();

        // Schedule a publish after a short delay so yield has time to subscribe.
        let bus_clone = bus.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            bus_clone.publish(
                "child-y",
                DelegationEvent::TurnCompleted {
                    output: "answer".into(),
                },
            );
        });

        let out = yield_tool
            .execute(
                mk_input(
                    "delegation_yield",
                    serde_json::json!({ "delegation_id": "child-y", "timeout_secs": 5 }),
                ),
                make_ctx("parent"),
            )
            .await
            .unwrap();
        assert!(!out.is_error);

        let parsed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(parsed["delegation_id"], "child-y");
        assert_eq!(parsed["event"]["type"], "turn_completed");
        assert_eq!(parsed["event"]["output"], "answer");
    }

    #[tokio::test]
    async fn delegation_yield_times_out_cleanly() {
        let bus = DelegationBus::new();
        bus.register_session(
            "child-t",
            ChildSessionMeta {
                parent_agent_id: "p".into(),
                child_agent_id: "c".into(),
                initial_prompt: "".into(),
            },
        );
        let tool = DelegationYieldTool::new(bus);
        let err = tool
            .execute(
                mk_input(
                    "delegation_yield",
                    // `timeout_secs` is a u64 in args — but we can't set
                    // sub-second values that way; use a direct bus subscribe
                    // to keep the test fast instead.
                    serde_json::json!({ "delegation_id": "child-t", "timeout_secs": 1 }),
                ),
                make_ctx("p"),
            )
            .await;
        match err {
            Err(ToolError::Timeout) => {}
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn delegation_yield_errors_on_unknown_session() {
        let bus = DelegationBus::new();
        let tool = DelegationYieldTool::new(bus);
        let err = tool
            .execute(
                mk_input(
                    "delegation_yield",
                    serde_json::json!({ "delegation_id": "ghost", "timeout_secs": 5 }),
                ),
                make_ctx("p"),
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidInput(msg) => {
                assert!(msg.contains("ghost"), "message should name the session: {msg}");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn delegation_send_delivers_to_yielding_parent() {
        let bus = DelegationBus::new();
        let spawn = DelegationSpawnTool::new(bus.clone());
        let send = DelegationSendTool::new(bus.clone());
        let yield_tool = DelegationYieldTool::new(bus.clone());

        spawn
            .execute(
                mk_input(
                    "delegation_spawn",
                    serde_json::json!({ "name": "child-s", "prompt": "" }),
                ),
                make_ctx("parent"),
            )
            .await
            .unwrap();

        // Subscribe first (yield), then send.
        let yield_handle = tokio::spawn({
            let yield_tool = yield_tool;
            async move {
                yield_tool
                    .execute(
                        mk_input(
                            "delegation_yield",
                            serde_json::json!({ "delegation_id": "child-s", "timeout_secs": 5 }),
                        ),
                        make_ctx("parent"),
                    )
                    .await
            }
        });

        // Give the yield task a moment to subscribe.
        tokio::time::sleep(Duration::from_millis(30)).await;

        let out = send
            .execute(
                mk_input(
                    "delegation_send",
                    serde_json::json!({
                        "delegation_id": "child-s",
                        "message": "ping",
                    }),
                ),
                make_ctx("parent"),
            )
            .await
            .unwrap();
        assert!(!out.is_error);
        let parsed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(parsed["delivered"], true);
        assert_eq!(parsed["subscribers_notified"], 1);

        let yield_out = yield_handle.await.unwrap().unwrap();
        let ev: serde_json::Value = serde_json::from_str(&yield_out.content).unwrap();
        assert_eq!(ev["event"]["type"], "message_emitted");
        assert_eq!(ev["event"]["content"], "ping");
    }

    #[tokio::test]
    async fn delegation_send_errors_on_unknown_session() {
        let bus = DelegationBus::new();
        let tool = DelegationSendTool::new(bus);
        let err = tool
            .execute(
                mk_input(
                    "delegation_send",
                    serde_json::json!({ "delegation_id": "nope", "message": "x" }),
                ),
                make_ctx("p"),
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidInput(msg) => {
                assert!(msg.contains("nope"), "message should name session: {msg}");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn delegation_send_without_subscribers_returns_zero() {
        let bus = DelegationBus::new();
        bus.register_session(
            "quiet",
            ChildSessionMeta {
                parent_agent_id: "p".into(),
                child_agent_id: "c".into(),
                initial_prompt: "".into(),
            },
        );
        let tool = DelegationSendTool::new(bus);
        let out = tool
            .execute(
                mk_input(
                    "delegation_send",
                    serde_json::json!({ "delegation_id": "quiet", "message": "hi" }),
                ),
                make_ctx("p"),
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(parsed["delivered"], true);
        assert_eq!(parsed["subscribers_notified"], 0);
    }

    #[tokio::test]
    async fn metadata_and_schema_shape() {
        let bus = DelegationBus::new();
        let (spawn, list, yield_tool, send) = build_delegation_tools(bus);

        assert_eq!(spawn.metadata().name, "delegation_spawn");
        assert_eq!(spawn.metadata().risk_level, RiskLevel::Execute);
        assert_eq!(list.metadata().name, "delegation_list");
        assert_eq!(list.metadata().risk_level, RiskLevel::Read);
        assert_eq!(yield_tool.metadata().name, "delegation_yield");
        assert_eq!(yield_tool.metadata().risk_level, RiskLevel::Read);
        assert_eq!(send.metadata().name, "delegation_send");
        assert_eq!(send.metadata().risk_level, RiskLevel::Write);

        // Schemas must advertise required fields.
        let spawn_required = spawn.schema().parameters.required;
        assert!(spawn_required.contains(&"name".to_string()));
        assert!(spawn_required.contains(&"prompt".to_string()));

        let yield_required = yield_tool.schema().parameters.required;
        assert_eq!(yield_required, vec!["delegation_id".to_string()]);

        let list_required = list.schema().parameters.required;
        assert!(list_required.is_empty());

        let send_required = send.schema().parameters.required;
        assert!(send_required.contains(&"delegation_id".to_string()));
        assert!(send_required.contains(&"message".to_string()));
    }

    /// sera-u4gj — explicit `ToolScope` per delegation tool. Spawning and
    /// sending into a named child session are cross-agent operations
    /// (`Agent`); yielding only observes the next bus event (`Read`).
    #[test]
    fn metadata_scopes() {
        let bus = DelegationBus::new();
        let (spawn, list, yield_tool, send) = build_delegation_tools(bus);
        assert_eq!(spawn.metadata().scope, ToolScope::Agent);
        assert_eq!(list.metadata().scope, ToolScope::Read);
        assert_eq!(yield_tool.metadata().scope, ToolScope::Read);
        assert_eq!(send.metadata().scope, ToolScope::Agent);
    }

    #[tokio::test]
    async fn concurrent_yields_all_resume_on_publish() {
        let bus = DelegationBus::new();
        bus.register_session(
            "multi",
            ChildSessionMeta {
                parent_agent_id: "p".into(),
                child_agent_id: "c".into(),
                initial_prompt: "".into(),
            },
        );

        let t1 = Arc::new(DelegationYieldTool::new(bus.clone()));
        let t2 = Arc::clone(&t1);
        let t3 = Arc::clone(&t1);

        let h1 = tokio::spawn({
            let t = t1.clone();
            async move {
                t.execute(
                    mk_input(
                        "delegation_yield",
                        serde_json::json!({ "delegation_id": "multi", "timeout_secs": 5 }),
                    ),
                    make_ctx("p"),
                )
                .await
            }
        });
        let h2 = tokio::spawn({
            let t = t2.clone();
            async move {
                t.execute(
                    mk_input(
                        "delegation_yield",
                        serde_json::json!({ "delegation_id": "multi", "timeout_secs": 5 }),
                    ),
                    make_ctx("p"),
                )
                .await
            }
        });
        let h3 = tokio::spawn({
            let t = t3.clone();
            async move {
                t.execute(
                    mk_input(
                        "delegation_yield",
                        serde_json::json!({ "delegation_id": "multi", "timeout_secs": 5 }),
                    ),
                    make_ctx("p"),
                )
                .await
            }
        });

        // Give all three yields time to subscribe.
        tokio::time::sleep(Duration::from_millis(40)).await;

        let n = bus.publish(
            "multi",
            DelegationEvent::MessageEmitted {
                content: "fanout".into(),
            },
        );
        assert_eq!(n, 3);

        for h in [h1, h2, h3] {
            let out = h.await.unwrap().unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&out.content).unwrap();
            assert_eq!(parsed["event"]["type"], "message_emitted");
            assert_eq!(parsed["event"]["content"], "fanout");
        }
    }

    #[tokio::test]
    async fn build_delegation_tools_share_bus() {
        let bus = DelegationBus::new();
        let (spawn, _list, yield_tool, send) = build_delegation_tools(bus.clone());

        spawn
            .execute(
                mk_input(
                    "delegation_spawn",
                    serde_json::json!({ "name": "shared", "prompt": "" }),
                ),
                make_ctx("p"),
            )
            .await
            .unwrap();

        // yield must subscribe first — otherwise a send fired before the
        // subscription lands will just observe zero subscribers and the
        // yield will later time out.
        let yield_fut = tokio::spawn({
            let yield_tool = yield_tool;
            async move {
                yield_tool
                    .execute(
                        mk_input(
                            "delegation_yield",
                            serde_json::json!({ "delegation_id": "shared", "timeout_secs": 5 }),
                        ),
                        make_ctx("p"),
                    )
                    .await
            }
        });

        // Give yield time to subscribe.
        tokio::time::sleep(Duration::from_millis(30)).await;

        // Now send — should find the session spawn registered and deliver.
        let send_out = send
            .execute(
                mk_input(
                    "delegation_send",
                    serde_json::json!({ "delegation_id": "shared", "message": "x" }),
                ),
                make_ctx("p"),
            )
            .await
            .unwrap();
        assert!(!send_out.is_error);

        let yield_out = yield_fut.await.unwrap().unwrap();
        assert!(!yield_out.is_error);
    }
}
