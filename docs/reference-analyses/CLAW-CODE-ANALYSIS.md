# Claw-Code Reference Analysis

> **Source:** `D:\projects\homelab\references\claw-code`
> **Date:** 2026-04-01
> **Purpose:** Extract architecture patterns for SERA agent-runtime improvements

## Overview

Claw-code is a ~20K-line Rust reimplementation of the Claude Code agent harness (with a parallel Python analysis workspace). It captures the production architecture of a Claude Code-like interactive AI agent — the REPL loop, tool system, permission model, session management, and system prompt assembly.

## Architecture Summary

### Crate Structure (Rust)

| Crate | Lines | Purpose |
|-------|-------|---------|
| `rusty-claude-cli` | ~3,600 | CLI binary — REPL, streaming display, arg parsing |
| `runtime` | ~5,300 | Core agent loop, config, sessions, permissions, hooks, compaction, MCP |
| `api` | ~1,500 | Anthropic HTTP client, SSE streaming, OAuth |
| `tools` | ~3,500 | Tool registry & execution (15+ built-in tools) |
| `commands` | ~470 | Slash command metadata |
| `compat-harness` | small | Parity tracking against upstream TS |

### Agent Loop — `ConversationRuntime<C, T>`

**File:** `rust/crates/runtime/src/conversation.rs`

The loop is generic over two traits:

```rust
pub trait ApiClient {
    fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError>;
}

pub trait ToolExecutor {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError>;
}
```

**Flow:**
1. Push user message to session
2. Loop (up to `max_iterations`):
   a. Build `ApiRequest` (system prompt + messages)
   b. Stream → parse events into `AssistantEvent` enum (`TextDelta`, `ToolUse`, `Usage`, `MessageStop`)
   c. Build assistant message from events, record usage
   d. If no tool calls → break
   e. For each tool call:
      - Check permission (`PermissionPolicy.authorize()`)
      - If allowed: run PreToolUse hook → execute tool → run PostToolUse hook
      - If denied: return error tool result with reason
      - Merge hook feedback into tool result
   f. Push tool results, continue loop
3. After loop: check auto-compaction threshold → compact if needed
4. Return `TurnSummary` (messages, results, iterations, usage, compaction event)

**Key design decisions:**
- Tool denial doesn't abort the loop — it sends an error result back to the LLM
- Hook feedback is visible to the LLM (merged into tool output)
- Auto-compaction is token-threshold-based (200K default), not turn-count-based
- `max_iterations` defaults to `usize::MAX` (effectively unlimited)
- Usage tracker is reconstructed from session on initialization (supports resume)

### Permission System

**File:** `rust/crates/runtime/src/permissions.rs`

Five modes ordered by privilege:
1. `ReadOnly` — file reads, search, web fetch
2. `WorkspaceWrite` — + file writes/edits
3. `DangerFullAccess` — + shell execution
4. `Prompt` — always asks user
5. `Allow` — everything auto-allowed

Each tool declares its required mode. Authorization logic:
- If `active >= required` → Allow
- If `active == Allow` → Allow (short circuit)
- If escalation possible (e.g., WorkspaceWrite → DangerFullAccess) → invoke `PermissionPrompter`
- Otherwise → Deny with explanation

### Hook System

**File:** `rust/crates/runtime/src/hooks.rs`

Pre/post tool execution hooks configured in `.claude.json`:
- Commands run as shell processes with env vars: `HOOK_EVENT`, `HOOK_TOOL_NAME`, `HOOK_TOOL_INPUT`, `HOOK_TOOL_OUTPUT`, `HOOK_TOOL_IS_ERROR`
- Full JSON payload piped to stdin
- Exit code semantics: 0 = allow, 2 = deny, other = warn (allow with message)
- Multiple hooks run in sequence; first deny stops the chain
- Stdout captured as feedback message

### Session Compaction

**File:** `rust/crates/runtime/src/compact.rs`

Generates structured summaries of compacted messages:
- **Scope**: message count by role (user/assistant/tool)
- **Tools mentioned**: deduplicated tool names from ToolUse and ToolResult blocks
- **Recent user requests**: last 3 user text messages (truncated to 160 chars)
- **Pending work**: messages containing "todo", "next", "pending", "follow up", "remaining"
- **Key files**: file path candidates extracted from all message content (up to 8)
- **Current work**: most recent non-empty text block
- **Key timeline**: role + summarized content for each message (160 char max per block)

Compacted session structure:
1. System message with continuation text + formatted summary
2. Preserved recent messages (default: 4)

Continuation message includes:
- "This session is being continued from a previous conversation..."
- Summary content
- "Recent messages are preserved verbatim" (if applicable)
- "Continue without asking further questions" directive

Token estimation: `content.len() / 4 + 1` per block (simple char-based approximation).

### System Prompt Assembly

**File:** `rust/crates/runtime/src/prompt.rs`

`SystemPromptBuilder` constructs a `Vec<String>` of segments:
1. **Intro**: "You are an interactive agent that helps users with software engineering tasks."
2. **Output style** (optional): custom style name + prompt
3. **System rules**: tool execution, permissions, hooks, compression, prompt injection awareness
4. **Doing tasks**: scoped changes, no speculative abstractions, security awareness, faithful reporting
5. **Actions with care**: reversibility, blast radius, shared systems
6. **Dynamic boundary marker**: `__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__` (separates static from dynamic)
7. **Environment context**: model family, working directory, date, platform
8. **Project context**: date, cwd, instruction file count, git status, git diff
9. **Instruction files**: discovered CLAUDE.md chain with scope labels
10. **Runtime config**: loaded config entries with source labels

**Instruction file discovery:**
- Walks from cwd to root, checking each directory for: `CLAUDE.md`, `CLAUDE.local.md`, `.claude/CLAUDE.md`, `.claude/instructions.md`
- Deduplicates by content hash (normalized: trimmed, collapsed blank lines)
- Per-file budget: 4,000 chars
- Total budget: 12,000 chars
- Truncation notice appended when budget exceeded

### Session Persistence

**File:** `rust/crates/runtime/src/session.rs`

- `Session { version: u32, messages: Vec<ConversationMessage> }`
- `ConversationMessage { role, blocks: Vec<ContentBlock>, usage: Option<TokenUsage> }`
- `ContentBlock` enum: `Text`, `ToolUse { id, name, input }`, `ToolResult { tool_use_id, tool_name, output, is_error }`
- JSON serialization with custom `to_json()` / `from_json()`
- `save_to_path()` / `load_from_path()`
- Usage stored per-message for tracker reconstruction

### Configuration

**File:** `rust/crates/runtime/src/config.rs`

Hierarchy (lowest → highest priority):
1. Built-in defaults
2. Environment variables (`ANTHROPIC_API_KEY`, `CLAUDE_*`)
3. Global `~/.claude.json`
4. Workspace `.claude.json`
5. Local `.claude.local.json`

Parsed into `RuntimeConfig` with `RuntimeFeatureConfig` containing:
- `hooks: RuntimeHookConfig`
- `mcp: McpConfigCollection`
- `oauth: Option<OAuthConfig>`
- `model: Option<String>`
- `permission_mode: Option<ResolvedPermissionMode>`
- `sandbox: SandboxConfig`

### MCP Integration

**File:** `rust/crates/runtime/src/mcp.rs`, `mcp_client.rs`, `mcp_stdio.rs`

- Tool naming: `mcp__<server>__<tool>` (normalized)
- Transport types: Stdio, SSE, HTTP, WebSocket, SDK, ClaudeAiProxy
- Server config in `.claude.json` under `mcp.servers`
- OAuth support per server
- Stdio: process spawning with JSON-RPC

## Gap Analysis vs SERA

| Pattern | SERA Status | Claw-Code | Impact |
|---------|-------------|-----------|--------|
| Trait-based loop | Concrete classes | Generic traits | High — testability |
| Permission escalation | allowed/denied lists | 5-mode + interactive | High — safety |
| Pre/post hooks | None | Full hook system | Medium — extensibility |
| Compaction summaries | Drop oldest (no summary) | Structured summaries | **Critical** — context quality |
| Prompt assembly | Minimal | Multi-segment builder | **Critical** — agent grounding |
| Instruction discovery | Manifest only | Ancestry chain | High — project awareness |
| Session persistence | Ephemeral | JSON save/load | Medium — reliability |
| Usage tracking | Basic totals | Per-turn, cache-aware | Low — observability |
| Error recovery (overflow/timeout) | Yes | No | SERA ahead |
| Inter-agent messaging | Yes (intercom) | No | SERA ahead |
| Thought streaming | Yes (Centrifugo) | No | SERA ahead |

## Test Patterns Worth Adopting

1. **ScriptedApiClient**: returns different responses per call count — enables end-to-end loop testing
2. **StaticToolExecutor**: register closures per tool name — no mocking framework needed
3. **PermissionPrompter mocks**: `PromptAllowOnce`, `RejectPrompter`, `RecordingPrompter`
4. **Temp directory tests**: create temp dirs, write files, test discovery, clean up
5. **Shell snippet helpers**: platform-aware (`#[cfg(windows)]`) test command generation
