# sera-runtime — Agent Worker Crate

## Purpose

Standalone agent worker process for SERA. Owns the LLM client, tool registry, tool dispatch, context engine, and turn loop. Can run as a standalone binary or be embedded as a library.

## Running the Binary

```bash
# Interactive REPL (TTY input)
cargo run -p sera-runtime -- \
  --llm-url http://localhost:1234/v1 \
  --model mistral-7b \
  --api-key abc123

# NDJSON gateway mode (piped input)
cargo run -p sera-runtime -- --ndjson < submissions.ndjson
```

### Key Environment Variables

| Var | Default | Purpose |
|-----|---------|---------|
| `LLM_BASE_URL` | (required) | OpenAI-compatible API endpoint |
| `LLM_MODEL` | (required) | Model name (e.g., `mistral-7b`, `gpt-4`) |
| `LLM_API_KEY` | (required) | API key for LLM provider |
| `MAX_TOKENS` | 2048 | Max tokens per LLM response |
| `AGENT_ID` | `sera-local` | Agent identifier |
| `AGENT_CHAT_PORT` | 0 | Health check HTTP port (0 = disabled) |
| `RUST_LOG` | `info` | Tracing filter (NDJSON mode) |

## Module Map

| Module | Purpose |
|--------|---------|
| `main.rs` | CLI entry point, interactive REPL, NDJSON transport loop |
| `lib.rs` | Public API surface |
| `turn.rs` | Four-method lifecycle (observe, think, act, react); traits `LlmProvider`, `ToolDispatcher` |
| `default_runtime.rs` | `AgentRuntime` trait implementation; turn execution orchestration |
| `context_engine/` | Context assembly and compaction; KV cache for session state |
| `compaction/` | Token budget management; condenser pipeline for context window reduction |
| `tools/` | Tool registry (15+ built-in tools), dispatcher, per-tool executors |
| `context.rs` | Session context and state tracking |
| `session_manager.rs` | Session lifecycle, key rotation, metrics |
| `llm_client.rs` | OpenAI-compatible LLM calls, token counting |
| `config.rs` | Runtime configuration from env + CLI |
| `types.rs` | Tool definitions, context types |
| `error.rs` | Error types |
| `manifest.rs` | Agent manifest parsing |
| `health.rs` | HTTP health check server |
| `harness.rs` | Testing harness |
| `handoff.rs` | Agent delegation and handoff |
| `subagent.rs` | Sub-agent spawning and lifecycle |
| `delegation.rs` | Tool-level delegation strategy |

## Core Types

### Turn Loop

- **`LlmProvider`** trait: Chat interface (messages + tools → response with tool calls)
- **`ToolDispatcher`** trait: Execute tool calls → results
- **`TurnOutcome`**: Union of 8 terminal states (FinalOutput, Handoff, Compact, Interruption, RunAgain, Stop, WaitingForApproval, etc.)

### Context Engine

- **`ContextEngine`** trait: Assemble context window from history given a token budget
- **`ContextPipeline`**: Default implementation with KV cache + compaction
- **`CompactionCheckpoint`**: Tracks context compression events (timestamp, tokens before/after, summary)

### NDJSON Protocol

Gateway ↔ runtime communication over JSON lines.

**Submission** (gateway → runtime):
```json
{"id": "uuid", "op": {"type": "user_turn", "items": [...], "session_key": "..."}}
```

**Event** (runtime → gateway):
```json
{"id": "uuid", "submission_id": "uuid", "msg": {"type": "handshake", ...}, "timestamp": "...", "parent_session_key": "..."}
```

Message types: `Handshake`, `TurnStarted`, `StreamingDelta`, `ToolCallBegin`, `ToolCallEnd`, `TurnCompleted`, `Error`.

## Test Layout

- **Unit tests**: Inline in each module (e.g., `turn.rs#[cfg(test)]`)
- **Integration tests**: Under `tests/`
  - `tests/runtime_acceptance.rs`: Full turn cycles
  - `tests/integration.rs`: Gateway + runtime interaction
  - `tests/mock_lm_studio_test.rs`: LLM provider mocking
  - `tests/file_lock_tests.rs`: File access contention

## Key Decisions

- **All JSON**: Messages, tool args, results, context use `serde_json::Value` for decoupling
- **Pluggable context**: Trait-based engine allows custom context assembly strategies
- **Four-method lifecycle**: Observe → think → act → react, with hooks at each point
- **Standalone**: Runtime owns LLM client and tool registry; no dependency on gateway for execution

## See Also

- `docs/ARCHITECTURE.md` — overall SERA design
- `docs/plan/specs/` — runtime protocol specs (if present)
- `rust/CLAUDE.md` — workspace toolchain and dev workflow
