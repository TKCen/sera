//! Reasoning loop — async state machine that drives agent task execution.

use crate::config::RuntimeConfig;
use crate::context::ContextManager;
use crate::llm_client::LlmClient;
use crate::tools::ToolRegistry;
use crate::types::{ChatMessage, TaskInput, TaskOutput, ToolCallRecord, UsageStats};

/// States of the reasoning loop.
enum State {
    Init,
    Think,
    Act,
    Observe,
    Done,
}

/// Run the reasoning loop for a given task.
pub async fn run(config: &RuntimeConfig, input: TaskInput) -> anyhow::Result<TaskOutput> {
    let llm_client = LlmClient::new(config);
    let tool_registry = ToolRegistry::new();
    let mut context_manager =
        ContextManager::new(config.context_window, config.compaction_strategy.clone());

    let max_iterations = input.max_iterations.unwrap_or(25);
    let mut messages: Vec<ChatMessage> = Vec::new();
    let mut tool_records: Vec<ToolCallRecord> = Vec::new();
    let mut usage = UsageStats::default();

    // Initialize conversation
    messages.push(ChatMessage {
        role: "system".to_string(),
        content: Some(
            "You are a SERA agent. Complete the given task using the available tools. \
             When done, provide your final answer in a message without tool calls."
                .to_string(),
        ),
        ..Default::default()
    });

    // Add any context messages
    for ctx_msg in &input.context {
        messages.push(ctx_msg.clone());
    }

    // Add the task prompt
    messages.push(ChatMessage {
        role: "user".to_string(),
        content: Some(input.prompt.clone()),
        ..Default::default()
    });

    let mut state = State::Init;

    loop {
        state = match state {
            State::Init => State::Think,

            State::Think => {
                usage.iterations += 1;
                if usage.iterations > max_iterations {
                    tracing::warn!("Max iterations ({max_iterations}) reached, stopping");
                    break;
                }

                // Compact context if needed
                let ctx_messages = context_manager.prepare(&messages);

                // Call LLM
                let result = llm_client.chat(&ctx_messages, &tool_registry.definitions()).await;

                match result {
                    Ok(resp) => {
                        usage.prompt_tokens += resp.prompt_tokens;
                        usage.completion_tokens += resp.completion_tokens;
                        usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;

                        messages.push(resp.message.clone());

                        if resp.message.tool_calls.is_some() {
                            State::Act
                        } else {
                            State::Done
                        }
                    }
                    Err(e) => {
                        tracing::error!("LLM call failed: {e}");
                        return Ok(TaskOutput {
                            task_id: input.task_id,
                            status: "failed".to_string(),
                            result: None,
                            error: Some(format!("LLM error: {e}")),
                            messages,
                            tool_calls: tool_records,
                            usage,
                        });
                    }
                }
            }

            State::Act => {
                let tool_calls = messages
                    .last()
                    .and_then(|m| m.tool_calls.clone())
                    .unwrap_or_default();

                for tc in &tool_calls {
                    let start = std::time::Instant::now();
                    let args: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or_default();

                    tracing::info!(tool = %tc.function.name, "Executing tool");

                    let result = match tool_registry.execute(&tc.function.name, &args).await {
                        Ok(r) => r,
                        Err(e) => format!("Tool error: {e}"),
                    };

                    tool_records.push(ToolCallRecord {
                        tool_name: tc.function.name.clone(),
                        arguments: args,
                        result: result.clone(),
                        duration_ms: start.elapsed().as_millis() as u64,
                    });

                    messages.push(ChatMessage {
                        role: "tool".to_string(),
                        content: Some(result),
                        tool_call_id: Some(tc.id.clone()),
                        name: Some(tc.function.name.clone()),
                        ..Default::default()
                    });
                }

                State::Observe
            }

            State::Observe => State::Think,

            State::Done => break,
        };
    }

    // Extract final answer
    let result = messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && m.content.is_some())
        .and_then(|m| m.content.clone());

    Ok(TaskOutput {
        task_id: input.task_id,
        status: if result.is_some() { "completed" } else { "failed" }.to_string(),
        result,
        error: None,
        messages,
        tool_calls: tool_records,
        usage,
    })
}
