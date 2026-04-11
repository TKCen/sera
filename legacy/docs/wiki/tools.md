# SERA Tools

## Tool Registry

SERA has a **ToolRegistry** that manages tool definitions and execution. MVS implements 7 core tools.

### Tool Definition Format

Each tool is defined as an OpenAI-compatible function schema:

```json
{
  "type": "function",
  "function": {
    "name": "memory_read",
    "description": "Read memory by key",
    "parameters": {
      "type": "object",
      "properties": {
        "key": { "type": "string", "description": "Memory key" }
      },
      "required": ["key"]
    }
  }
}
```

## MVS Tools (7)

### 1. memory_read
**Purpose**: Retrieve a memory entry by key.

**Parameters**:
- `key` (string, required) — Memory identifier

**Returns**: Memory content as string, or "Not found" if missing.

**Example**:
```json
{ "name": "memory_read", "arguments": "{\"key\":\"config/deployment\"}" }
```

### 2. memory_write
**Purpose**: Store or update a memory entry.

**Parameters**:
- `key` (string, required) — Memory identifier
- `content` (string, required) — Memory content

**Returns**: "Stored" or error message.

### 3. memory_search
**Purpose**: Find memory entries matching keywords.

**Parameters**:
- `query` (string, required) — Search keywords

**Returns**: List of matching entries with heading context.

**Example result**:
```
Match: patterns/auth
> ## JWT Validation
> Validate exp claim immediately on token parse...

Match: learnings/token
> Tokens expire after 24 hours...
```

### 4. file_read
**Purpose**: Read a file from the workspace.

**Parameters**:
- `path` (string, required) — Relative path from workspace root

**Returns**: File contents or error if:
- Path escapes workspace (security check)
- File doesn't exist
- Permission denied

**Path safety**: Blocks attempts to read outside workspace:
```rust
if !path.canonicalize()?.starts_with(workspace.canonicalize()?) {
    return Err("Path escapes workspace");
}
```

### 5. file_write
**Purpose**: Create or overwrite a file.

**Parameters**:
- `path` (string, required) — Relative path from workspace
- `content` (string, required) — File content

**Returns**: "Written" or error.

**Path safety**: Same escape check as file_read.

### 6. shell
**Purpose**: Execute a shell command (with timeout).

**Parameters**:
- `command` (string, required) — Shell command
- `timeout_secs` (integer, optional, default: 30) — Execution timeout

**Returns**: Command output (stdout) or error (stderr).

**Execution**:
```rust
use tokio::process::Command;

let output = Command::new("bash")
    .arg("-c")
    .arg(&command)
    .timeout(Duration::from_secs(timeout_secs))
    .output()
    .await?;
```

**Restrictions** (post-MVS):
- Deny list for dangerous commands (rm -rf /, etc.)
- Require explicit allow list for file modifications

### 7. session_reset
**Purpose**: Signal session reset (archive current, create new).

**Parameters**: None

**Returns**: Special signal `SESSION_RESET_REQUESTED` which triggers:
1. Current session archived
2. New session created
3. Turn exits with status "session_reset"

## Tool Matching (Allow Lists)

Each agent specifies allowed tools via **glob patterns** in the spec:

```yaml
spec:
  tools:
    allow: ["memory_*", "file_read", "shell"]
```

**Glob expansion**:
- `memory_*` → matches memory_read, memory_write, memory_search
- `file_*` → matches file_read, file_write
- `shell` → matches exactly shell
- Exact strings take precedence over patterns

**Validation at execution**:
```rust
fn matches_allowed(&tool_name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| {
        if p.contains('*') {
            glob::Pattern::new(p).matches(tool_name)
        } else {
            p == tool_name
        }
    })
}
```

**Example agent specs**:

Agent "sera" (general purpose):
```yaml
tools:
  allow: ["memory_*", "file_*", "shell", "session_*"]
```

Agent "researcher" (read-only):
```yaml
tools:
  allow: ["memory_*", "file_read"]  # No write, no shell
```

Agent "admin":
```yaml
tools:
  allow: ["*"]  # All tools
```

## Tool Execution Flow

1. **Parse tool call** from LLM response
   ```rust
   let tool_call = tc;  // { id, function: { name, arguments } }
   let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)?;
   ```

2. **Validate**
   - Check tool name in agent's allow list
   - Return error if not allowed

3. **Execute**
   ```rust
   let result = tool_registry.execute(&tc.function.name, &args).await?;
   ```

4. **Record** to audit trail
   ```rust
   db.append_transcript(
       &session_id,
       "tool",
       Some(&result),
       None,
       Some(&tc.id)  // tool_call_id
   )?;
   ```

5. **Append to messages** for next LLM call
   ```rust
   messages.push(ChatMessage {
       role: "tool",
       content: Some(result),
       tool_call_id: Some(tc.id),
       name: Some(tc.function.name),
       ..Default::default()
   });
   ```

## Error Handling

### Tool Not Found
```
Tool error: Unknown tool: memory_foo
```

### Tool Not Allowed
```
Tool error: Tool 'shell' not in agent allow list
```

### Execution Error
```
Tool error: File not found: /workspace/missing.txt
```

### Path Escape
```
Tool error: Path escapes workspace: ../../sensitive.txt
```

### Timeout
```
Tool error: Command timed out after 30 seconds
```

### Result Size Limit
(Post-MVS) Tool results capped at 4KB to prevent context explosion:
```rust
const MAX_TOOL_RESULT_SIZE: usize = 4096;
if result.len() > MAX_TOOL_RESULT_SIZE {
    return format!("[Truncated: {} bytes]", result.len());
}
```

## Adding a New Tool

1. **Define parameters** in OpenAI format
2. **Implement ToolExecutor trait**
   ```rust
   #[async_trait]
   pub trait ToolExecutor: Send + Sync {
       fn name(&self) -> &str;
       fn description(&self) -> &str;
       fn parameters(&self) -> serde_json::Value;
       async fn execute(&self, args: &serde_json::Value) -> anyhow::Result<String>;
   }
   ```

3. **Register in ToolRegistry::new()**
   ```rust
   let tools: Vec<Box<dyn ToolExecutor>> = vec![
       // ... existing tools ...
       Box::new(my_new_tool::MyNewTool),
   ];
   ```

4. **Add to agent allow lists** in sera.yaml if MVS or config-driven.

## Post-MVS Enhancements

- Full tool schemas with nested parameters
- Tool use precedence (prefer specific over glob)
- Streaming tool results (for long outputs)
- Tool-to-tool chaining (tool can invoke another tool)
- Tool version negotiation (LLM queries supported versions)
- Metrics: tool success rate, latency histograms

---

Last updated: 2026-04-09
