//! Model response types.

use serde::{Deserialize, Serialize};

/// A response from a model provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    /// The generated message content.
    pub content: String,
    /// The reason the model stopped generating.
    pub finish_reason: FinishReason,
    /// Usage statistics for this completion.
    pub usage: Usage,
    /// Any tool calls the model made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Reason the model stopped generating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
}

/// A tool call made by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// The tool call ID.
    pub id: String,
    /// The name of the tool.
    pub name: String,
    /// The arguments to the tool (JSON).
    pub arguments: serde_json::Value,
}

/// Usage statistics for a completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── FinishReason ──────────────────────────────────────────────────────────

    #[test]
    fn finish_reason_serde_all_variants() {
        let cases = [
            (FinishReason::Stop, "stop"),
            (FinishReason::Length, "length"),
            (FinishReason::ContentFilter, "content_filter"),
            (FinishReason::ToolCalls, "tool_calls"),
        ];
        for (reason, expected_str) in cases {
            let json = serde_json::to_string(&reason).expect("serialize FinishReason");
            assert_eq!(
                json,
                format!("\"{expected_str}\""),
                "wrong JSON for {reason:?}"
            );
            let parsed: FinishReason =
                serde_json::from_str(&json).expect("deserialize FinishReason");
            assert_eq!(parsed, reason, "round-trip failed for {reason:?}");
        }
    }

    #[test]
    fn finish_reason_rejects_unknown_string() {
        let result: Result<FinishReason, _> = serde_json::from_str("\"unknown_reason\"");
        assert!(result.is_err(), "expected error for unknown finish reason");
    }

    // ── Usage ─────────────────────────────────────────────────────────────────

    #[test]
    fn usage_serde_roundtrip_camel_case() {
        let usage = Usage {
            prompt_tokens: 120,
            completion_tokens: 40,
            total_tokens: 160,
        };
        let json = serde_json::to_string(&usage).expect("serialize Usage");
        // camelCase keys
        assert!(
            json.contains("promptTokens"),
            "expected camelCase key, got: {json}"
        );
        assert!(
            json.contains("completionTokens"),
            "expected camelCase key, got: {json}"
        );
        assert!(
            json.contains("totalTokens"),
            "expected camelCase key, got: {json}"
        );

        let parsed: Usage = serde_json::from_str(&json).expect("deserialize Usage");
        assert_eq!(parsed.prompt_tokens, 120);
        assert_eq!(parsed.completion_tokens, 40);
        assert_eq!(parsed.total_tokens, 160);
    }

    #[test]
    fn usage_missing_field_is_error() {
        // total_tokens is not optional — omitting it should fail
        let bad = json!({"promptTokens": 10, "completionTokens": 5});
        let result: Result<Usage, _> = serde_json::from_value(bad);
        assert!(result.is_err(), "expected error for missing totalTokens");
    }

    // ── ToolCall ──────────────────────────────────────────────────────────────

    #[test]
    fn tool_call_serde_roundtrip() {
        let tc = ToolCall {
            id: "call-001".into(),
            name: "bash".into(),
            arguments: json!({"cmd": "ls -la"}),
        };
        let json = serde_json::to_string(&tc).expect("serialize ToolCall");
        let parsed: ToolCall = serde_json::from_str(&json).expect("deserialize ToolCall");
        assert_eq!(parsed.id, "call-001");
        assert_eq!(parsed.name, "bash");
        assert_eq!(parsed.arguments["cmd"], "ls -la");
    }

    #[test]
    fn tool_call_arguments_preserves_nested_structure() {
        let args = json!({
            "path": "/tmp/notes.md",
            "options": {"encoding": "utf-8", "create": true}
        });
        let tc = ToolCall {
            id: "call-xyz".into(),
            name: "file_write".into(),
            arguments: args.clone(),
        };
        let json = serde_json::to_string(&tc).expect("serialize");
        let parsed: ToolCall = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.arguments, args);
    }

    // ── ModelResponse ─────────────────────────────────────────────────────────

    #[test]
    fn model_response_serde_roundtrip_no_tools() {
        let resp = ModelResponse {
            content: "Hello, world!".into(),
            finish_reason: FinishReason::Stop,
            usage: Usage {
                prompt_tokens: 10,
                completion_tokens: 4,
                total_tokens: 14,
            },
            tool_calls: None,
        };
        let json = serde_json::to_string(&resp).expect("serialize ModelResponse");
        // tool_calls should be omitted when None
        assert!(
            !json.contains("tool_calls"),
            "tool_calls should be absent when None, got: {json}"
        );
        let parsed: ModelResponse = serde_json::from_str(&json).expect("deserialize ModelResponse");
        assert_eq!(parsed.content, "Hello, world!");
        assert_eq!(parsed.finish_reason, FinishReason::Stop);
        assert_eq!(parsed.usage.total_tokens, 14);
        assert!(parsed.tool_calls.is_none());
    }

    #[test]
    fn model_response_serde_roundtrip_with_tool_calls() {
        let resp = ModelResponse {
            content: String::new(),
            finish_reason: FinishReason::ToolCalls,
            usage: Usage {
                prompt_tokens: 50,
                completion_tokens: 20,
                total_tokens: 70,
            },
            tool_calls: Some(vec![
                ToolCall {
                    id: "c1".into(),
                    name: "search".into(),
                    arguments: json!({"query": "Rust async"}),
                },
                ToolCall {
                    id: "c2".into(),
                    name: "read_file".into(),
                    arguments: json!({"path": "src/main.rs"}),
                },
            ]),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let parsed: ModelResponse = serde_json::from_str(&json).expect("deserialize");
        let calls = parsed.tool_calls.expect("tool_calls should be Some");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "search");
        assert_eq!(calls[1].name, "read_file");
        assert_eq!(parsed.finish_reason, FinishReason::ToolCalls);
    }

    #[test]
    fn model_response_content_filter_finish_reason() {
        let resp = ModelResponse {
            content: String::new(),
            finish_reason: FinishReason::ContentFilter,
            usage: Usage {
                prompt_tokens: 30,
                completion_tokens: 0,
                total_tokens: 30,
            },
            tool_calls: None,
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(
            json.contains("content_filter"),
            "expected content_filter in JSON, got: {json}"
        );
        let parsed: ModelResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.finish_reason, FinishReason::ContentFilter);
    }

    #[test]
    fn model_response_length_finish_reason() {
        let resp = ModelResponse {
            content: "truncated...".into(),
            finish_reason: FinishReason::Length,
            usage: Usage {
                prompt_tokens: 100,
                completion_tokens: 512,
                total_tokens: 612,
            },
            tool_calls: None,
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let parsed: ModelResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.finish_reason, FinishReason::Length);
        assert_eq!(parsed.usage.completion_tokens, 512);
    }
}
