//! Reasoning loop — async state machine that drives agent task execution.
//!
//! Enhanced MVS turn loop:
//! receive message → assemble context → call LLM → execute tools → persist → repeat

use std::path::Path;

use crate::config::RuntimeConfig;
use crate::context::ContextManager;
use crate::context_assembler::ContextAssembler;
use crate::llm_client::LlmClient;
use crate::session_manager::SessionManager;
use crate::tools::mvs_tools::MvsToolRegistry;
use crate::tools::ToolRegistry;
use crate::types::{ChatMessage, TaskInput, TaskOutput, ToolCallRecord, UsageStats};

/// Exit reason for the reasoning loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitReason {
    /// The agent produced a final text response.
    Completed,
    /// The maximum number of iterations was reached.
    MaxIterations,
    /// The LLM returned an unrecoverable error.
    LlmError(String),
    /// A session reset was requested by the agent.
    SessionReset,
}

impl std::fmt::Display for ExitReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExitReason::Completed => write!(f, "completed"),
            ExitReason::MaxIterations => write!(f, "max_iterations"),
            ExitReason::LlmError(e) => write!(f, "llm_error: {e}"),
            ExitReason::SessionReset => write!(f, "session_reset"),
        }
    }
}

/// States of the reasoning loop.
enum State {
    Init,
    Think,
    Act,
    Observe,
    Done(ExitReason),
}

/// Configuration for the enhanced reasoning loop.
pub struct LoopConfig<'a> {
    pub runtime_config: &'a RuntimeConfig,
    pub workspace_path: Option<&'a Path>,
    pub persona: Option<&'a str>,
    pub memory_context: Option<&'a str>,
    pub session_manager: Option<&'a SessionManager>,
}

const MAX_CONTEXT_OVERFLOW_RETRIES: u32 = 3;
const MAX_TIMEOUT_RETRIES: u32 = 2;
const SESSION_RESET_SIGNAL: &str = "SESSION_RESET_REQUESTED";

/// Run the enhanced reasoning loop for a given task.
///
/// This is the primary entry point that integrates SessionManager,
/// ContextAssembler, and MvsToolRegistry.
pub async fn run_enhanced(loop_config: LoopConfig<'_>, input: TaskInput) -> anyhow::Result<TaskOutput> {
    let llm_client = LlmClient::new(loop_config.runtime_config);
    let context_manager = ContextManager::new(
        loop_config.runtime_config.context_window,
    );

    // Set up MVS tool registry if workspace is provided, otherwise use the
    // generic ToolRegistry.
    let mvs_registry = loop_config
        .workspace_path
        .map(MvsToolRegistry::new);
    let generic_registry = ToolRegistry::new();

    let max_iterations = input.max_iterations.unwrap_or(10);
    let mut messages: Vec<ChatMessage> = Vec::new();
    let mut tool_records: Vec<ToolCallRecord> = Vec::new();
    let mut usage = UsageStats::default();

    // Resolve session for transcript persistence.
    let agent_id = input
        .agent_id
        .as_deref()
        .unwrap_or(&loop_config.runtime_config.agent_id);
    let session_id = if let Some(sm) = loop_config.session_manager {
        match input.session_id.as_deref() {
            Some(sid) => sid.to_string(),
            None => sm.get_or_create_session(agent_id)?,
        }
    } else {
        String::new()
    };

    // Load existing transcript from the session if available.
    let history_messages = if !session_id.is_empty() {
        if let Some(sm) = loop_config.session_manager {
            sm.load_transcript(&session_id).unwrap_or_default()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    // Determine persona.
    let persona = loop_config.persona.unwrap_or(
        "You are a SERA agent. Complete the given task using the available tools. \
         When done, provide your final answer in a message without tool calls.",
    );

    // Build tool definitions — prefer MVS tools if available.
    let tool_defs_json: Vec<serde_json::Value> = if let Some(ref mvs) = mvs_registry {
        mvs.definitions()
    } else {
        generic_registry
            .definitions()
            .iter()
            .map(|td| serde_json::to_value(td).unwrap_or_default())
            .collect()
    };

    // Assemble initial context using the ContextAssembler.
    let history_json: Vec<serde_json::Value> = history_messages
        .iter()
        .map(|m| serde_json::to_value(m).unwrap_or_default())
        .collect();

    // Add any input context messages to the history.
    let mut full_history = history_json;
    for ctx_msg in &input.context {
        full_history.push(serde_json::to_value(ctx_msg).unwrap_or_default());
    }

    let assembled = ContextAssembler::assemble(
        persona,
        &tool_defs_json,
        loop_config.memory_context,
        &full_history,
        &input.prompt,
    );

    // Convert assembled JSON values back to ChatMessages for the loop.
    for val in &assembled {
        if let Ok(msg) = serde_json::from_value::<ChatMessage>(val.clone()) {
            messages.push(msg);
        }
    }

    // Persist the user prompt message to transcript.
    if !session_id.is_empty()
        && let Some(sm) = loop_config.session_manager
    {
        let user_msg = ChatMessage {
            role: "user".to_string(),
            content: Some(input.prompt.clone()),
            ..Default::default()
        };
        let _ = sm.append_message(&session_id, &user_msg);
    }

    let mut state = State::Init;
    let mut context_overflow_retries: u32 = 0;

    loop {
        state = match state {
            State::Init => State::Think,

            State::Think => {
                usage.iterations += 1;
                if usage.iterations > max_iterations {
                    tracing::warn!("Max iterations ({max_iterations}) reached, stopping");
                    State::Done(ExitReason::MaxIterations)
                } else {
                    // Compact context if approaching the context window limit.
                    if context_manager.is_near_limit(&messages) {
                        let result = context_manager.compact(&mut messages);
                        if result.dropped_count > 0 {
                            tracing::info!(
                                dropped = result.dropped_count,
                                tokens_before = result.tokens_before,
                                tokens_after = result.tokens_after,
                                "Context compacted"
                            );
                        }
                    }
                    let ctx_messages = context_manager.prepare(&messages);

                    // Call LLM with timeout retry.
                    let mut timeout_retries = 0u32;
                    let result = loop {
                        let call_result = llm_client
                            .chat(&ctx_messages, &generic_registry.definitions())
                            .await;

                        match &call_result {
                            Err(e) if is_timeout_error(e) && timeout_retries < MAX_TIMEOUT_RETRIES => {
                                timeout_retries += 1;
                                tracing::warn!(
                                    attempt = timeout_retries,
                                    "LLM call timed out, retrying ({timeout_retries}/{MAX_TIMEOUT_RETRIES})"
                                );
                                continue;
                            }
                            Err(e) if is_context_overflow_error(e)
                                && context_overflow_retries < MAX_CONTEXT_OVERFLOW_RETRIES =>
                            {
                                context_overflow_retries += 1;
                                tracing::warn!(
                                    attempt = context_overflow_retries,
                                    "Context overflow, compacting and retrying ({context_overflow_retries}/{MAX_CONTEXT_OVERFLOW_RETRIES})"
                                );
                                // Force aggressive compaction: keep system + last quarter.
                                let keep = messages.len() / 4;
                                let keep = keep.max(2); // At minimum keep system + 1 msg
                                if messages.len() > keep {
                                    let system_msg = messages[0].clone();
                                    let tail: Vec<ChatMessage> =
                                        messages[messages.len() - keep + 1..].to_vec();
                                    messages.clear();
                                    messages.push(system_msg);
                                    messages.push(ChatMessage {
                                        role: "system".to_string(),
                                        content: Some(format!(
                                            "[Context compacted: earlier messages removed to fit within context window. Retry {}/{}]",
                                            context_overflow_retries, MAX_CONTEXT_OVERFLOW_RETRIES
                                        )),
                                        ..Default::default()
                                    });
                                    messages.extend(tail);
                                }
                                break call_result;
                            }
                            _ => break call_result,
                        }
                    };

                    // If the last result was a context overflow that we just compacted for,
                    // loop back to Think to re-call with the compacted context.
                    match result {
                        Ok(resp) => {
                            usage.prompt_tokens += resp.prompt_tokens;
                            usage.completion_tokens += resp.completion_tokens;
                            usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;

                            let assistant_msg = resp.message.clone();

                            // Persist assistant message to transcript.
                            if !session_id.is_empty()
                                && let Some(sm) = loop_config.session_manager
                            {
                                let _ = sm.append_message(&session_id, &assistant_msg);
                            }

                            messages.push(assistant_msg);

                            if resp.message.tool_calls.is_some() {
                                State::Act
                            } else {
                                State::Done(ExitReason::Completed)
                            }
                        }
                        Err(e) if is_context_overflow_error(&e)
                            && context_overflow_retries <= MAX_CONTEXT_OVERFLOW_RETRIES =>
                        {
                            // We already compacted above; retry the Think state.
                            // Decrement iterations since this was a retry, not a real turn.
                            usage.iterations = usage.iterations.saturating_sub(1);
                            State::Think
                        }
                        Err(e) => {
                            tracing::error!("LLM call failed: {e}");
                            State::Done(ExitReason::LlmError(format!("{e}")))
                        }
                    }
                }
            }

            State::Act => {
                let tool_calls = messages
                    .last()
                    .and_then(|m| m.tool_calls.clone())
                    .unwrap_or_default();

                let mut session_reset_requested = false;

                for tc in &tool_calls {
                    let start = std::time::Instant::now();
                    let args: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or_default();

                    tracing::info!(tool = %tc.function.name, "Executing tool");

                    let result = if let Some(ref mvs) = mvs_registry {
                        match mvs.execute(&tc.function.name, &args).await {
                            Ok(r) => r,
                            Err(e) => format!("Tool error: {e}"),
                        }
                    } else {
                        match generic_registry.execute(&tc.function.name, &args).await {
                            Ok(r) => r,
                            Err(e) => format!("Tool error: {e}"),
                        }
                    };

                    // Check for session reset signal.
                    if result.contains(SESSION_RESET_SIGNAL) {
                        session_reset_requested = true;
                    }

                    tool_records.push(ToolCallRecord {
                        tool_name: tc.function.name.clone(),
                        arguments: args,
                        result: result.clone(),
                        duration_ms: start.elapsed().as_millis() as u64,
                    });

                    // Truncate large tool outputs to stay within context budget.
                    let truncated_result = context_manager.truncate_tool_output(&result);

                    let tool_msg = ChatMessage {
                        role: "tool".to_string(),
                        content: Some(truncated_result),
                        tool_call_id: Some(tc.id.clone()),
                        name: Some(tc.function.name.clone()),
                        ..Default::default()
                    };

                    // Persist tool result to transcript.
                    if !session_id.is_empty()
                        && let Some(sm) = loop_config.session_manager
                    {
                        let _ = sm.append_message(&session_id, &tool_msg);
                    }

                    messages.push(tool_msg);
                }

                if session_reset_requested {
                    // Handle session reset: archive and create new session.
                    if let Some(sm) = loop_config.session_manager {
                        match sm.reset_session(agent_id) {
                            Ok(new_sid) => {
                                tracing::info!(
                                    old_session = %session_id,
                                    new_session = %new_sid,
                                    "Session reset completed"
                                );
                            }
                            Err(e) => {
                                tracing::error!("Session reset failed: {e}");
                            }
                        }
                    }
                    State::Done(ExitReason::SessionReset)
                } else {
                    State::Observe
                }
            }

            State::Observe => State::Think,

            State::Done(reason) => {
                // Extract final answer.
                let result = messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "assistant" && m.content.is_some())
                    .and_then(|m| m.content.clone());

                let status = match &reason {
                    ExitReason::Completed => "completed",
                    ExitReason::MaxIterations => "max_iterations",
                    ExitReason::LlmError(_) => "failed",
                    ExitReason::SessionReset => "session_reset",
                };

                let error = match &reason {
                    ExitReason::LlmError(e) => Some(e.clone()),
                    _ => None,
                };

                return Ok(TaskOutput {
                    task_id: input.task_id,
                    status: status.to_string(),
                    result,
                    error,
                    messages,
                    tool_calls: tool_records,
                    usage,
                });
            }
        };
    }
}

/// Run the reasoning loop for a given task (original interface, kept for
/// backward compatibility).
pub async fn run(config: &RuntimeConfig, input: TaskInput) -> anyhow::Result<TaskOutput> {
    let loop_config = LoopConfig {
        runtime_config: config,
        workspace_path: None,
        persona: None,
        memory_context: None,
        session_manager: None,
    };
    run_enhanced(loop_config, input).await
}

/// Check if an error looks like a timeout.
fn is_timeout_error(e: &crate::llm_client::LlmError) -> bool {
    matches!(e, crate::llm_client::LlmError::Timeout(_))
}

/// Check if an error looks like a context overflow / token limit exceeded.
fn is_context_overflow_error(e: &crate::llm_client::LlmError) -> bool {
    matches!(e, crate::llm_client::LlmError::ContextOverflow(_))
}
