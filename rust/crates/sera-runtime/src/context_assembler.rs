//! Context assembler — builds the LLM message list in KV-cache-optimized order.
//!
//! The ordering places stable content first so the KV cache can be reused
//! across turns:
//!
//! 1. System prompt (persona) — stable, placed first
//! 2. Tool schemas — stable (injected via the API `tools` parameter, not here)
//! 3. Memory excerpts — semi-stable
//! 4. Conversation history — volatile
//! 5. Current user message — volatile

use serde_json::Value;

/// Assembles the LLM context in KV-cache-optimized order.
pub struct ContextAssembler;

impl ContextAssembler {
    /// Build the full message list for an LLM call.
    ///
    /// `tool_definitions` are included as a system-level description block so
    /// the LLM sees them in a stable position. The actual OpenAI-format `tools`
    /// array is passed separately via the API request body — this method only
    /// arranges the *messages* list.
    ///
    /// # Arguments
    /// - `persona` — the agent's system prompt / persona description
    /// - `tool_definitions` — tool schema JSON objects (rendered as a tool-list
    ///   reminder in a system message)
    /// - `memory_context` — optional memory excerpts to inject
    /// - `history` — prior conversation messages (already serialized as JSON values)
    /// - `current_message` — the latest user message text
    pub fn assemble(
        persona: &str,
        tool_definitions: &[Value],
        memory_context: Option<&str>,
        history: &[Value],
        current_message: &str,
    ) -> Vec<Value> {
        let mut messages: Vec<Value> = Vec::new();

        // 1. System prompt (persona) — most stable, placed first for KV cache.
        messages.push(serde_json::json!({
            "role": "system",
            "content": persona,
        }));

        // 2. Tool schema reminder — stable across turns.
        //    The actual `tools` parameter is set on the request body, but we
        //    include a concise reminder in the system messages so the model
        //    has the tool list in its context window in the optimal position.
        if !tool_definitions.is_empty() {
            let tool_names: Vec<&str> = tool_definitions
                .iter()
                .filter_map(|td| {
                    td.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                })
                .collect();

            if !tool_names.is_empty() {
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": format!(
                        "Available tools: {}. Use them to complete the task. \
                         When done, provide your final answer without tool calls.",
                        tool_names.join(", ")
                    ),
                }));
            }
        }

        // 3. Memory excerpts — semi-stable (changes less often than conversation).
        if let Some(memory) = memory_context {
            if !memory.is_empty() {
                messages.push(serde_json::json!({
                    "role": "system",
                    "content": format!("Relevant memory:\n{memory}"),
                }));
            }
        }

        // 4. Conversation history — volatile, appended in order.
        for msg in history {
            messages.push(msg.clone());
        }

        // 5. Current user message — most volatile, placed last.
        messages.push(serde_json::json!({
            "role": "user",
            "content": current_message,
        }));

        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn basic_assembly_ordering() {
        let persona = "You are a helpful agent.";
        let tools = vec![json!({
            "type": "function",
            "function": {
                "name": "file_read",
                "description": "Read a file",
                "parameters": {}
            }
        })];
        let memory = Some("User prefers concise answers.");
        let history = vec![
            json!({"role": "user", "content": "previous question"}),
            json!({"role": "assistant", "content": "previous answer"}),
        ];
        let current = "What is 2+2?";

        let result = ContextAssembler::assemble(persona, &tools, memory, &history, current);

        // Should have: system(persona) + system(tools) + system(memory) + 2 history + user
        assert_eq!(result.len(), 6);

        // 1. Persona
        assert_eq!(result[0]["role"], "system");
        assert_eq!(result[0]["content"], persona);

        // 2. Tool reminder
        assert_eq!(result[1]["role"], "system");
        let tool_content = result[1]["content"].as_str().unwrap();
        assert!(tool_content.contains("file_read"));
        assert!(tool_content.contains("Available tools"));

        // 3. Memory
        assert_eq!(result[2]["role"], "system");
        let mem_content = result[2]["content"].as_str().unwrap();
        assert!(mem_content.contains("concise answers"));

        // 4. History
        assert_eq!(result[3]["role"], "user");
        assert_eq!(result[3]["content"], "previous question");
        assert_eq!(result[4]["role"], "assistant");
        assert_eq!(result[4]["content"], "previous answer");

        // 5. Current message
        assert_eq!(result[5]["role"], "user");
        assert_eq!(result[5]["content"], current);
    }

    #[test]
    fn assembly_without_memory() {
        let result =
            ContextAssembler::assemble("persona", &[], None, &[], "hello");

        // system(persona) + user(current)
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["role"], "system");
        assert_eq!(result[1]["role"], "user");
        assert_eq!(result[1]["content"], "hello");
    }

    #[test]
    fn assembly_with_empty_memory_string() {
        let result =
            ContextAssembler::assemble("persona", &[], Some(""), &[], "hello");

        // Empty memory string should be skipped.
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn assembly_without_tools() {
        let result = ContextAssembler::assemble(
            "persona",
            &[],
            Some("some memory"),
            &[],
            "hello",
        );

        // system(persona) + system(memory) + user(current) — no tool reminder
        assert_eq!(result.len(), 3);
        assert_eq!(result[0]["content"], "persona");
        assert!(result[1]["content"].as_str().unwrap().contains("some memory"));
        assert_eq!(result[2]["content"], "hello");
    }

    #[test]
    fn assembly_with_multiple_tools() {
        let tools = vec![
            json!({"type": "function", "function": {"name": "shell", "description": "Run cmd", "parameters": {}}}),
            json!({"type": "function", "function": {"name": "file_read", "description": "Read", "parameters": {}}}),
            json!({"type": "function", "function": {"name": "memory_write", "description": "Write mem", "parameters": {}}}),
        ];

        let result =
            ContextAssembler::assemble("agent", &tools, None, &[], "do something");

        // system(persona) + system(tools) + user(current)
        assert_eq!(result.len(), 3);
        let tool_content = result[1]["content"].as_str().unwrap();
        assert!(tool_content.contains("shell"));
        assert!(tool_content.contains("file_read"));
        assert!(tool_content.contains("memory_write"));
    }

    #[test]
    fn assembly_preserves_history_order() {
        let history = vec![
            json!({"role": "user", "content": "msg1"}),
            json!({"role": "assistant", "content": "resp1"}),
            json!({"role": "user", "content": "msg2"}),
            json!({"role": "assistant", "content": "resp2", "tool_calls": []}),
            json!({"role": "tool", "content": "result", "tool_call_id": "tc1"}),
        ];

        let result = ContextAssembler::assemble(
            "sys",
            &[],
            None,
            &history,
            "msg3",
        );

        // system + 5 history + user
        assert_eq!(result.len(), 7);
        // History should be in exact order.
        assert_eq!(result[1]["content"], "msg1");
        assert_eq!(result[2]["content"], "resp1");
        assert_eq!(result[3]["content"], "msg2");
        assert_eq!(result[4]["content"], "resp2");
        assert_eq!(result[5]["content"], "result");
        assert_eq!(result[6]["content"], "msg3");
    }

    #[test]
    fn assembly_with_all_segments() {
        let tools = vec![
            json!({"type": "function", "function": {"name": "shell", "description": "x", "parameters": {}}}),
        ];
        let memory = Some("Agent memory: project is SERA");
        let history = vec![
            json!({"role": "user", "content": "first turn"}),
            json!({"role": "assistant", "content": "first response"}),
        ];

        let result = ContextAssembler::assemble(
            "You are SERA agent Alpha.",
            &tools,
            memory,
            &history,
            "What's next?",
        );

        assert_eq!(result.len(), 6);

        // Verify ordering: persona, tools, memory, history..., current
        assert_eq!(result[0]["content"], "You are SERA agent Alpha.");
        assert!(result[1]["content"].as_str().unwrap().contains("shell"));
        assert!(result[2]["content"].as_str().unwrap().contains("SERA"));
        assert_eq!(result[3]["content"], "first turn");
        assert_eq!(result[4]["content"], "first response");
        assert_eq!(result[5]["content"], "What's next?");
    }
}
