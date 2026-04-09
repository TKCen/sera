//! Tool definition types — OpenAI function-calling schemas for the LLM.
//!
//! MVS scope: 7 built-in tools per mvs-review-plan §9.
//! No progressive disclosure, no sandbox, no profiles.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A tool definition in OpenAI function-calling format.
/// This is the schema sent to the LLM so it knows what tools are available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

/// The function portion of a tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: FunctionParameters,
}

/// JSON Schema for function parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionParameters {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub properties: HashMap<String, ParameterSchema>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
}

/// Schema for a single parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

/// The result of executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: message.into(),
            is_error: true,
        }
    }
}

/// The 7 MVS built-in tool names.
pub const MVS_TOOLS: &[&str] = &[
    "memory_read",
    "memory_write",
    "memory_search",
    "file_read",
    "file_write",
    "shell",
    "session_reset",
];

/// Check if a tool name matches an allow pattern (supports glob-style wildcards).
/// Used by AgentToolsSpec.allow patterns like "memory_*", "file_*".
pub fn tool_matches_pattern(tool_name: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return tool_name.starts_with(prefix);
    }
    tool_name == pattern
}

/// Check if a tool is allowed by a set of allow patterns.
pub fn tool_is_allowed(tool_name: &str, allow_patterns: &[String]) -> bool {
    if allow_patterns.is_empty() {
        return true; // Empty allow list = allow all
    }
    allow_patterns.iter().any(|p| tool_matches_pattern(tool_name, p))
}

// ── Spec-aligned Tool architecture (SPEC-tools) ─────────────────────────────

/// Risk level of a tool — determines authorization requirements.
/// SPEC-tools: capability ≠ execution; risk level gates approval routing.
/// Ordered from least to most dangerous for comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Read-only observation (e.g., file_read, memory_search).
    Read,
    /// Modifies state (e.g., file_write, memory_write).
    Write,
    /// Runs arbitrary code (e.g., shell_exec, code_eval).
    Execute,
    /// System-level operations (e.g., agent management, config changes).
    Admin,
}

/// Where a tool executes — determines isolation boundaries.
/// SPEC-tools §6a: pluggable sandbox providers (Docker, WASM, MicroVM, External).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTarget {
    /// Runs in the agent runtime process.
    InProcess,
    /// Runs in a sandboxed environment (Docker, WASM, MicroVM).
    Sandbox(String),
    /// Runs on the local host (file operations).
    Local,
    /// Runs on a remote service.
    Remote(String),
    /// External tool via MCP or other protocol.
    External,
}

/// Metadata describing a tool's identity and capabilities.
/// SPEC-tools: returned by Tool::metadata(), used for progressive disclosure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub risk_level: RiskLevel,
    pub execution_target: ExecutionTarget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Tool profile — preset allow/deny configurations.
/// SPEC-tools §5: profiles simplify tool policy management.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfile {
    /// Minimal tools — read-only, no execution.
    Minimal,
    /// Basic tools — read/write, no code execution.
    Basic,
    /// Coding tools — includes shell, code eval.
    Coding,
    /// Full access to all tools.
    Full,
    /// Custom profile defined by allow/deny patterns.
    Custom,
}

/// Tool policy — controls which tools an agent can use.
/// SPEC-tools: profile + allow/deny patterns. Deny takes precedence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<ToolProfile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny_patterns: Vec<String>,
}

impl ToolPolicy {
    /// Check if a tool is allowed by this policy.
    /// Deny patterns take precedence over allow patterns.
    pub fn allows(&self, tool_name: &str) -> bool {
        if self.deny_patterns.iter().any(|p| tool_matches_pattern(tool_name, p)) {
            return false;
        }
        tool_is_allowed(tool_name, &self.allow_patterns)
    }

    /// Create a policy for a given profile with sensible defaults.
    pub fn from_profile(profile: ToolProfile) -> Self {
        match profile {
            ToolProfile::Minimal => Self {
                profile: Some(ToolProfile::Minimal),
                allow_patterns: vec![
                    "memory_read".to_string(),
                    "memory_search".to_string(),
                    "file_read".to_string(),
                ],
                deny_patterns: vec![],
            },
            ToolProfile::Basic => Self {
                profile: Some(ToolProfile::Basic),
                allow_patterns: vec![
                    "memory_*".to_string(),
                    "file_*".to_string(),
                    "session_*".to_string(),
                ],
                deny_patterns: vec!["shell".to_string()],
            },
            ToolProfile::Coding => Self {
                profile: Some(ToolProfile::Coding),
                allow_patterns: vec!["*".to_string()],
                deny_patterns: vec![],
            },
            ToolProfile::Full => Self {
                profile: Some(ToolProfile::Full),
                allow_patterns: vec!["*".to_string()],
                deny_patterns: vec![],
            },
            ToolProfile::Custom => Self {
                profile: Some(ToolProfile::Custom),
                allow_patterns: vec![],
                deny_patterns: vec![],
            },
        }
    }
}

// ── Tool trait + execution context types (SPEC-tools §3) ─────────────────────

use async_trait::async_trait;

/// Wraps FunctionParameters as a spec-aligned schema type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub parameters: FunctionParameters,
}

/// Input to a tool execution call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInput {
    /// The name of the tool being called.
    pub name: String,
    /// The arguments passed by the model, as a JSON object.
    pub arguments: serde_json::Value,
    /// The call ID from the LLM (for correlating results).
    pub call_id: String,
}

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// The content to return to the model.
    pub content: String,
    /// Whether the execution resulted in an error.
    pub is_error: bool,
    /// Optional metadata attached to the result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl ToolOutput {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            metadata: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: message.into(),
            is_error: true,
            metadata: None,
        }
    }
}

/// Error variants for tool execution.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ToolError {
    #[error("tool not found: {0}")]
    NotFound(String),
    #[error("unauthorized to execute tool: {0}")]
    Unauthorized(String),
    #[error("tool execution failed: {0}")]
    ExecutionFailed(String),
    #[error("tool execution timed out")]
    Timeout,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("policy denied tool execution: {0}")]
    PolicyDenied(String),
}

/// Injected credentials available to a tool at execution time.
/// Populated by the Secret Manager and pre-tool hooks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CredentialBag(pub HashMap<String, String>);

impl CredentialBag {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.0.get(key)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Distributed tracing handle for a tool execution span.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditHandle {
    pub trace_id: String,
    pub span_id: String,
}

/// Lightweight reference to a session — used in tool context.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionRef(pub String);

impl SessionRef {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for SessionRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lightweight reference to an agent — used in tool registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentRef(pub String);

impl AgentRef {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for AgentRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Context provided to a tool at execution time.
/// SPEC-tools §3: session, principal, credentials, policy, and audit handle.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// The session this execution is taking place in.
    pub session: SessionRef,
    /// The principal (agent or human) invoking the tool.
    pub principal: crate::principal::PrincipalRef,
    /// Credentials injected by the Secret Manager and pre-tool hooks.
    pub credentials: CredentialBag,
    /// Policy governing which tools this principal may use.
    pub policy: ToolPolicy,
    /// Distributed tracing handle for audit and observability.
    pub audit_handle: AuditHandle,
}

/// The core tool abstraction. All built-in, plugin, and MCP-bridged tools implement this.
/// SPEC-tools §3: capability exposure is separate from execution authority.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Return static metadata about this tool.
    fn metadata(&self) -> ToolMetadata;

    /// Return the JSON Schema for this tool's parameters.
    fn schema(&self) -> ToolSchema;

    /// Execute the tool with the given input and context.
    async fn execute(&self, input: ToolInput, ctx: ToolContext) -> Result<ToolOutput, ToolError>;

    /// Return the risk level of this tool (convenience — same as metadata().risk_level).
    fn risk_level(&self) -> RiskLevel {
        self.metadata().risk_level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definition_roundtrip() {
        let def = ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "memory_read".to_string(),
                description: "Read a memory file".to_string(),
                parameters: FunctionParameters {
                    schema_type: "object".to_string(),
                    properties: {
                        let mut m = HashMap::new();
                        m.insert(
                            "path".to_string(),
                            ParameterSchema {
                                schema_type: "string".to_string(),
                                description: Some("Path to the memory file".to_string()),
                                enum_values: None,
                                default: None,
                            },
                        );
                        m
                    },
                    required: vec!["path".to_string()],
                },
            },
        };
        let json = serde_json::to_string(&def).unwrap();
        let parsed: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.function.name, "memory_read");
        assert_eq!(parsed.function.parameters.required.len(), 1);
    }

    #[test]
    fn tool_matches_exact() {
        assert!(tool_matches_pattern("shell", "shell"));
        assert!(!tool_matches_pattern("shell_exec", "shell"));
    }

    #[test]
    fn tool_matches_wildcard() {
        assert!(tool_matches_pattern("memory_read", "memory_*"));
        assert!(tool_matches_pattern("memory_write", "memory_*"));
        assert!(tool_matches_pattern("memory_search", "memory_*"));
        assert!(!tool_matches_pattern("file_read", "memory_*"));
    }

    #[test]
    fn tool_matches_star_all() {
        assert!(tool_matches_pattern("anything", "*"));
    }

    #[test]
    fn tool_is_allowed_empty_allows_all() {
        assert!(tool_is_allowed("any_tool", &[]));
    }

    #[test]
    fn tool_is_allowed_with_patterns() {
        let patterns = vec![
            "memory_*".to_string(),
            "file_*".to_string(),
            "shell".to_string(),
            "session_*".to_string(),
        ];
        assert!(tool_is_allowed("memory_read", &patterns));
        assert!(tool_is_allowed("file_write", &patterns));
        assert!(tool_is_allowed("shell", &patterns));
        assert!(tool_is_allowed("session_reset", &patterns));
        assert!(!tool_is_allowed("web_fetch", &patterns));
    }

    #[test]
    fn mvs_tools_count() {
        assert_eq!(MVS_TOOLS.len(), 7);
    }

    #[test]
    fn tool_result_success() {
        let r = ToolResult::success("ok");
        assert!(!r.is_error);
        assert_eq!(r.content, "ok");
    }

    #[test]
    fn tool_result_error() {
        let r = ToolResult::error("failed");
        assert!(r.is_error);
        assert_eq!(r.content, "failed");
    }

    // ── SPEC-tools aligned type tests ────────────────────────────────────

    #[test]
    fn risk_level_ordering() {
        assert!(RiskLevel::Read < RiskLevel::Write);
        assert!(RiskLevel::Write < RiskLevel::Execute);
        assert!(RiskLevel::Execute < RiskLevel::Admin);
    }

    #[test]
    fn risk_level_serde() {
        let variants = vec![
            (RiskLevel::Read, "read"),
            (RiskLevel::Write, "write"),
            (RiskLevel::Execute, "execute"),
            (RiskLevel::Admin, "admin"),
        ];
        for (level, expected) in variants {
            let json = serde_json::to_string(&level).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
            let parsed: RiskLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, level);
        }
    }

    #[test]
    fn execution_target_serde() {
        let cases = vec![
            (ExecutionTarget::InProcess, r#""in_process""#),
            (ExecutionTarget::Local, r#""local""#),
            (ExecutionTarget::External, r#""external""#),
        ];
        for (target, expected) in cases {
            let json = serde_json::to_string(&target).unwrap();
            assert_eq!(json, expected);
            let parsed: ExecutionTarget = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, target);
        }
    }

    #[test]
    fn execution_target_sandbox_serde() {
        let target = ExecutionTarget::Sandbox("docker".to_string());
        let json = serde_json::to_string(&target).unwrap();
        let parsed: ExecutionTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ExecutionTarget::Sandbox("docker".to_string()));
    }

    #[test]
    fn execution_target_remote_serde() {
        let target = ExecutionTarget::Remote("https://tools.example.com".to_string());
        let json = serde_json::to_string(&target).unwrap();
        let parsed: ExecutionTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed,
            ExecutionTarget::Remote("https://tools.example.com".to_string())
        );
    }

    #[test]
    fn tool_metadata_roundtrip() {
        let meta = ToolMetadata {
            name: "shell".to_string(),
            description: "Execute shell commands".to_string(),
            version: "1.0.0".to_string(),
            author: Some("sera".to_string()),
            risk_level: RiskLevel::Execute,
            execution_target: ExecutionTarget::Sandbox("docker".to_string()),
            tags: vec!["compute".to_string()],
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: ToolMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "shell");
        assert_eq!(parsed.risk_level, RiskLevel::Execute);
    }

    #[test]
    fn tool_profile_serde() {
        let variants = vec![
            (ToolProfile::Minimal, "minimal"),
            (ToolProfile::Basic, "basic"),
            (ToolProfile::Coding, "coding"),
            (ToolProfile::Full, "full"),
            (ToolProfile::Custom, "custom"),
        ];
        for (profile, expected) in variants {
            let json = serde_json::to_string(&profile).unwrap();
            assert_eq!(json, format!("\"{expected}\""));
        }
    }

    #[test]
    fn tool_policy_deny_overrides_allow() {
        let policy = ToolPolicy {
            profile: None,
            allow_patterns: vec!["*".to_string()],
            deny_patterns: vec!["shell".to_string()],
        };
        assert!(policy.allows("memory_read"));
        assert!(policy.allows("file_write"));
        assert!(!policy.allows("shell"));
    }

    #[test]
    fn tool_policy_empty_allows_all() {
        let policy = ToolPolicy {
            profile: None,
            allow_patterns: vec![],
            deny_patterns: vec![],
        };
        assert!(policy.allows("anything"));
    }

    #[test]
    fn tool_policy_from_minimal_profile() {
        let policy = ToolPolicy::from_profile(ToolProfile::Minimal);
        assert!(policy.allows("memory_read"));
        assert!(policy.allows("memory_search"));
        assert!(policy.allows("file_read"));
        assert!(!policy.allows("shell"));
        assert!(!policy.allows("file_write"));
    }

    #[test]
    fn tool_policy_from_basic_profile() {
        let policy = ToolPolicy::from_profile(ToolProfile::Basic);
        assert!(policy.allows("memory_read"));
        assert!(policy.allows("file_write"));
        assert!(!policy.allows("shell")); // explicitly denied
    }

    #[test]
    fn tool_policy_from_coding_profile() {
        let policy = ToolPolicy::from_profile(ToolProfile::Coding);
        assert!(policy.allows("shell"));
        assert!(policy.allows("memory_read"));
        assert!(policy.allows("anything"));
    }

    #[test]
    fn tool_policy_deny_wildcard() {
        let policy = ToolPolicy {
            profile: None,
            allow_patterns: vec!["*".to_string()],
            deny_patterns: vec!["memory_*".to_string()],
        };
        assert!(!policy.allows("memory_read"));
        assert!(!policy.allows("memory_write"));
        assert!(policy.allows("shell"));
        assert!(policy.allows("file_read"));
    }

    // ── New SPEC-tools types ──────────────────────────────────────────────────

    #[test]
    fn tool_schema_roundtrip() {
        let schema = ToolSchema {
            parameters: FunctionParameters {
                schema_type: "object".to_string(),
                properties: HashMap::new(),
                required: vec![],
            },
        };
        let json = serde_json::to_string(&schema).unwrap();
        let parsed: ToolSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.parameters.schema_type, "object");
    }

    #[test]
    fn tool_input_roundtrip() {
        let input = ToolInput {
            name: "file_read".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
            call_id: "call_abc123".to_string(),
        };
        let json = serde_json::to_string(&input).unwrap();
        let parsed: ToolInput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "file_read");
        assert_eq!(parsed.call_id, "call_abc123");
        assert_eq!(parsed.arguments["path"], "/tmp/test.txt");
    }

    #[test]
    fn tool_output_success() {
        let out = ToolOutput::success("done");
        assert!(!out.is_error);
        assert_eq!(out.content, "done");
        assert!(out.metadata.is_none());
    }

    #[test]
    fn tool_output_error() {
        let out = ToolOutput::error("failed");
        assert!(out.is_error);
        assert_eq!(out.content, "failed");
    }

    #[test]
    fn tool_output_roundtrip() {
        let out = ToolOutput {
            content: "result".to_string(),
            is_error: false,
            metadata: Some({
                let mut m = HashMap::new();
                m.insert("lines".to_string(), serde_json::json!(42));
                m
            }),
        };
        let json = serde_json::to_string(&out).unwrap();
        let parsed: ToolOutput = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content, "result");
        assert_eq!(parsed.metadata.as_ref().unwrap()["lines"], 42);
    }

    #[test]
    fn credential_bag_operations() {
        let mut bag = CredentialBag::new();
        assert!(bag.is_empty());
        bag.insert("api_key", "secret123");
        assert!(!bag.is_empty());
        assert_eq!(bag.get("api_key").unwrap(), "secret123");
        assert!(bag.get("missing").is_none());
    }

    #[test]
    fn credential_bag_roundtrip() {
        let mut bag = CredentialBag::new();
        bag.insert("token", "abc");
        let json = serde_json::to_string(&bag).unwrap();
        let parsed: CredentialBag = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.get("token").unwrap(), "abc");
    }

    #[test]
    fn session_ref_display() {
        let s = SessionRef::new("sess-xyz");
        assert_eq!(s.to_string(), "sess-xyz");
    }

    #[test]
    fn agent_ref_display() {
        let a = AgentRef::new("agent-001");
        assert_eq!(a.to_string(), "agent-001");
    }

    #[test]
    fn session_ref_roundtrip() {
        let s = SessionRef::new("sess-abc");
        let json = serde_json::to_string(&s).unwrap();
        let parsed: SessionRef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.0, "sess-abc");
    }

    #[test]
    fn agent_ref_roundtrip() {
        let a = AgentRef::new("agent-42");
        let json = serde_json::to_string(&a).unwrap();
        let parsed: AgentRef = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.0, "agent-42");
    }

    #[test]
    fn tool_error_display() {
        assert_eq!(
            ToolError::NotFound("shell".to_string()).to_string(),
            "tool not found: shell"
        );
        assert_eq!(
            ToolError::Unauthorized("shell".to_string()).to_string(),
            "unauthorized to execute tool: shell"
        );
        assert_eq!(ToolError::Timeout.to_string(), "tool execution timed out");
        assert_eq!(
            ToolError::PolicyDenied("admin_*".to_string()).to_string(),
            "policy denied tool execution: admin_*"
        );
    }

    #[test]
    fn tool_policy_integration_with_context() {
        // Verify ToolPolicy used inside ToolContext works as expected
        let policy = ToolPolicy::from_profile(ToolProfile::Basic);
        assert!(policy.allows("memory_read"));
        assert!(!policy.allows("shell"));
    }
}
