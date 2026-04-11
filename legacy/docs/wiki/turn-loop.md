# SERA Turn Loop

## State Machine Overview

The turn loop is an async state machine that drives agent task execution from input to completion.

```
Init → Think → Act → Observe → Done
       ↓   ↑ ↓   ↑
       └───┴─┴───┘ (retry loop on error)
```

**Exit Reasons**: Completed, MaxIterations, LlmError, SessionReset

## States

### Init
Initial state. Transitions to Think.

### Think
Invoke the LLM with current context.

**Context assembled** in order (KV-cache optimized):
1. System prompt (persona)
2. Tool definitions (JSON schema)
3. Memory context (optional)
4. Conversation history (all prior messages)
5. Current user prompt

**LLM Response** parsed for:
- `finish_reason` — "stop", "tool_calls", "max_tokens"
- `tool_calls` array (if present)
- `content` — final text response

**Transitions**:
- If `tool_calls` present → Act
- If `finish_reason` == "stop" and no tool_calls → Done(Completed)
- If error → Done(LlmError)
- If max_iterations reached → Done(MaxIterations)

**Error Handling**:

1. **Timeout Error** (2 retries):
   - Sleep and retry LLM call
   - Decrement iteration count (don't count as real turn)

2. **Context Overflow** (3 retries):
   - Aggressively compact messages
   - Keep system + last 1/4 of history
   - Re-enter Think state
   - Retry without incrementing iteration

3. **Provider Unavailable** (1 retry):
   - Simple backoff + one retry
   - Fail on second attempt

### Act
Execute tool calls returned from Think.

**Per Tool Call**:
1. Parse tool name and arguments (JSON)
2. Validate name against agent's allow list (glob patterns)
3. Execute with path safety checks (reject escaping workspace)
4. Record tool call with:
   - Function name
   - Arguments (JSON)
   - Result (string)
   - Duration (ms)

**Special Signal**: If tool result contains `SESSION_RESET_REQUESTED`:
- Archive current session
- Create new session
- Transition to Done(SessionReset)

**Otherwise**: Append tool result to messages, transition to Observe.

### Observe
Processing point between tool execution and next Think call.

**Transition**: Always → Think

### Done
Final state. Return TaskOutput with:
- Status (completed, max_iterations, failed, session_reset)
- Final reply text (last assistant message content)
- All messages (system, user, assistant, tool)
- All tool call records
- Usage stats (prompt/completion tokens)

## Context Assembly

```rust
pub fn assemble(
    persona: &str,
    tools: &[serde_json::Value],
    memory_context: Option<&str>,
    history: &[serde_json::Value],  // Prior messages
    prompt: &str,                    // Current user input
) -> Vec<serde_json::Value> {
    let mut messages = vec![];
    
    // 1. System prompt
    messages.push(json!({ "role": "system", "content": persona }));
    
    // 2. Tools
    if !tools.is_empty() {
        messages.push(json!({
            "role": "system",
            "content": format!("Available tools: {:?}", tools)
        }));
    }
    
    // 3. Memory
    if let Some(mem) = memory_context {
        messages.push(json!({
            "role": "system",
            "content": format!("Knowledge: {}", mem)
        }));
    }
    
    // 4. History
    messages.extend(history.iter().cloned());
    
    // 5. Current
    messages.push(json!({ "role": "user", "content": prompt }));
    
    messages
}
```

## Iteration Limits

**Max iterations per turn**: 10 (configurable)

Each Think state increments iteration counter. Retries (timeout, overflow) do NOT increment. This allows resilience while bounding run time.

If iteration == max_iterations in Think state → Done(MaxIterations).

## Message Persistence

**Every message persisted to SQLite**:
- After LLM Think call: assistant message
- After tool execution: tool result message
- On user input: user message

**Session key format**: `agent:{agent_id}:main`

**Transcript structure**:
```sql
id, session_id, role, content, tool_calls, tool_call_id, created_at
```

- `role` ∈ { "user", "assistant", "tool", "system" }
- `tool_calls` — JSON array of tool calls (only for assistant messages that initiated tools)
- `tool_call_id` — ID matching a prior tool call (only for tool result messages)

## Tool Execution

### Validation
```rust
// 1. Parse arguments
let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)?;

// 2. Validate against allow list (glob patterns)
if !matches_any_pattern(&tc.function.name, &agent_spec.tools.allow) {
    return Err("Tool not allowed");
}

// 3. Execute
let result = tool_registry.execute(&tc.function.name, &args).await?;
```

### Path Safety
All tools (file_ops, shell_exec, etc.) validate that paths don't escape workspace:

```rust
fn validate_path(path: &Path, workspace: &Path) -> Result<()> {
    let canonical = path.canonicalize()?;
    let workspace_canonical = workspace.canonicalize()?;
    
    if !canonical.starts_with(&workspace_canonical) {
        return Err("Path escapes workspace");
    }
    Ok(())
}
```

## Usage Tracking

Accumulated across all LLM calls in a turn:

```rust
pub struct UsageStats {
    pub iterations: u32,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}
```

Updated after each LLM response from the provider's usage field.

## Example Execution Flow

```
Input: TaskInput { task_id: "t1", prompt: "What's 2+2?" }

[Init]
  → State = Think

[Think] iteration=1
  context = [system, tools, history, "What's 2+2?"]
  LLM call → "The answer is 4"
  finish_reason = "stop", no tool_calls
  → State = Done(Completed)

[Done]
  return TaskOutput {
    task_id: "t1",
    status: "completed",
    result: "The answer is 4",
    usage: UsageStats { iterations: 1, ... }
  }
```

## Example with Tools

```
Input: "Create a file /workspace/notes.txt with content 'Hello'"

[Init] → Think

[Think] iteration=1
  LLM sees file_write tool
  Decides to use it
  finish_reason = "tool_calls"
  tool_calls: [{ id: "call_1", function: { name: "file_write", arguments: "{...}" } }]
  → State = Act

[Act]
  Execute file_write with arguments
  Result: "File created"
  Append to messages: { role: "tool", tool_call_id: "call_1", content: "File created" }
  → State = Observe

[Observe]
  → State = Think

[Think] iteration=2
  context = [system, tools, history, "What's 2+2?", <asst message with tool_calls>, <tool result>]
  LLM sees tool result, decides next step
  finish_reason = "stop", no tool_calls
  LLM says "File created successfully at /workspace/notes.txt"
  → State = Done(Completed)

[Done]
  return TaskOutput {
    status: "completed",
    result: "File created successfully...",
    tool_calls: [
      { tool_name: "file_write", arguments: {...}, result: "File created", duration_ms: 42 }
    ]
  }
```

## Error Recovery Examples

### Timeout Retry
```
[Think] iteration=1
  LLM call → Timeout error
  timeout_retries = 1 (< MAX_TIMEOUT_RETRIES)
  Sleep 100ms
  Retry LLM call
  → Success
  iterations stays at 1 (retry doesn't count)
```

### Context Overflow
```
[Think] iteration=5, context_overflow_retries=0
  LLM call → ContextOverflow error
  context_overflow_retries = 1
  Compact: keep system + last 25% of history
  iteration stays at 5 (compaction doesn't count)
  Retry Think state
```

---

Last updated: 2026-04-09
