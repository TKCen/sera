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
#[derive(Debug, Clone)]
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
    if pattern.ends_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
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
}
