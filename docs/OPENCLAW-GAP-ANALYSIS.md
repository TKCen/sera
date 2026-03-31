# OpenClaw vs SERA — Gap Analysis

> Generated 2026-03-31. Based on OpenClaw source (163K GitHub stars, MIT, single-agent personal AI assistant) vs SERA (multi-agent Docker-native orchestration platform).

## Executive Summary

OpenClaw excels at **single-agent reliability** — its agentic loop has battle-tested retry logic, automatic context compaction, model failover, and 40+ built-in tools. SERA excels at **multi-agent orchestration** — sandboxed Docker containers, hierarchical/parallel processes, circle-based shared memory, capability policies, and egress proxy.

**SERA's strategic gap**: the individual agent execution quality (the "inner loop") is significantly less mature than OpenClaw's. SERA has the better architecture for multi-agent home deployments, but each agent's moment-to-moment reasoning, error recovery, and tool handling needs hardening.

---

## Gap Categories

### Critical Gaps (directly impact agent quality)

| # | Area | OpenClaw | SERA Today | Priority |
|---|------|----------|------------|----------|
| 1 | **Agentic loop resilience** | Retry with overflow compaction (3 attempts), timeout compaction (2 attempts), tool result truncation fallback, model failover across auth profiles | Single `while` loop, MAX_ITERATIONS=10, no retry on overflow/timeout, no model failover | **P0** |
| 2 | **Tool loop detection** | Dedicated `tool-loop-detection-config.ts` — detects repetitive tool patterns and breaks out | Duplicate tool call detection (same name+args) only — misses semantic loops (same tool, slightly different args) | **P0** |
| 3 | **System prompt richness** | 15+ composable sections: identity, skills, memory, user identity, time, reply tags, messaging, voice/TTS, documentation, workspace notes, heartbeat, context files, tool summaries, model aliases, reasoning hints | Basic template: name + role + description + optional principles + generic tool guidelines | **P0** |
| 4 | **Context compaction maturity** | Auto-compaction on by default, pre-compaction memory save reminder, `/compact` manual trigger, community DAG-based plugin, explicit compaction events | Two strategies (sliding-window default, summarize), but summarize **not implemented** in agent-runtime — silently falls back. No pre-compaction memory save. No manual trigger. | **P0** |
| 5 | **Built-in tool count** | ~40 tools: messaging, sessions, web, media (image/PDF/TTS), canvas, cron, subagents, nodes REPL, code execution, config management | 10 tools: web-search, web-fetch, file-read, file-write, file-list, shell-exec, schedule-task, delegate-task, knowledge-store, knowledge-query | **P1** |
| 6 | **Streaming tool results** | Stream tool output as it executes, separate `onToolResult` + `onToolOutput` callbacks, block reply chunking | Tool results returned only after all tools complete. No streaming of intermediate results. | **P1** |
| 7 | **Parallel tool execution** | Tools executed via SessionManager with configurable concurrency | All tools executed sequentially in a `for` loop | **P1** |

### Important Gaps (improve agent UX and reliability)

| # | Area | OpenClaw | SERA Today | Priority |
|---|------|----------|------------|----------|
| 8 | **Thinking/reasoning levels** | 5 levels: off/low/medium/high/xhigh, configurable per-session, thinking blocks streamed and optionally filtered | Reasoning captured from models that emit it (Qwen3, DeepSeek), but no configurable thinking levels, no explicit reasoning mode | **P1** |
| 9 | **Tool argument repair** | `attempt.tool-call-argument-repair.ts` — fixes malformed JSON from LLMs, sanitizes provider-specific quirks, trims invalid tool names | No argument repair — JSON parse failure returns error to LLM | **P1** |
| 10 | **Stream wrapping chain** | 6-layer wrapping: Ollama compat → argument repair → name trimming → sanitize malformed calls → sensitive stop reason handling → idle timeout | Raw streaming with no transformation layers | **P2** |
| 11 | **Context file injection** | Bootstrap files + workspace notes embedded in system prompt | No equivalent — agents don't get reference documents in their prompt | **P1** |
| 12 | **Skill discovery & auto-trigger** | SKILL.md files with YAML frontmatter, auto-detected by keyword matching, 5700+ community skills on ClawHub, dependency resolution | Skills are code-defined in `builtins/`, no file-based skill format, no marketplace, no auto-trigger by keyword | **P2** |
| 13 | **Model failover** | Rotates through auth profiles and models on failure, exponential backoff on overload, `FailoverError` class | Provider health service tracks failures, but agent runtime has no failover — single model per run | **P1** |
| 14 | **Pre-compaction memory save** | Before compacting, agent is reminded to save important context to memory files | No pre-compaction hook — important context may be lost during sliding-window compaction | **P1** |
| 15 | **Tool result context guard** | `installToolResultContextGuard()` prevents oversized results from blowing context before they're added | Truncation at fixed byte limit (50KB), but no context-aware guard checking remaining budget | **P2** |

### Nice-to-Have Gaps (differentiation opportunities)

| # | Area | OpenClaw | SERA Today | Priority |
|---|------|----------|------------|----------|
| 16 | **Image/PDF/TTS tools** | Built-in image analysis (Vision API), PDF extraction, text-to-speech (ElevenLabs, Deepgram) | No media tools — agents can't view images, process PDFs, or speak | **P2** |
| 17 | **Canvas/live documents** | Canvas tool for creating/updating live rendered documents | No equivalent | **P3** |
| 18 | **Memory citations** | Citations mode (full/brief/off) — agent can reference which memory blocks informed its response | Memory blocks injected as XML but no citation tracking | **P3** |
| 19 | **Session transcript indexing** | Historical sessions indexed for RAG — agent can recall past conversation context | Session JSONL stored but not indexed for retrieval | **P2** |
| 20 | **Hybrid memory search** | Vector (0.7 weight) + full-text (0.3 weight) + MMR diversity + temporal decay (30-day half-life) | Vector search via Qdrant. No full-text fallback, no MMR, no temporal decay. | **P2** |

---

## SERA's Advantages (areas OpenClaw lacks entirely)

These are SERA differentiators that should be preserved and highlighted:

| Area | SERA | OpenClaw |
|------|------|----------|
| **Docker sandboxing** | Every agent runs in an isolated container with capability policies and egress proxy | Full system access, no sandboxing |
| **Multi-agent orchestration** | Hierarchical, sequential, parallel processes with delegation and task queues | Single-agent only, no orchestration |
| **Circle shared memory** | Agents share knowledge within circles with constitution-governed access | No shared memory concept |
| **Capability policies** | Tiered sandbox boundaries (tier-1/2/3), network ACLs, command allowlists | No capability restrictions |
| **Egress proxy** | Squid-based HTTP proxy with per-agent ACLs | No network filtering |
| **Audit trail** | Structured audit logging of all agent actions | No audit system |
| **Provider health routing** | Circuit breaker, health scoring, automatic provider rotation | Single model, no health tracking |
| **Budget enforcement** | Per-agent token budgets enforced at LLM proxy level | No budget system |
| **Agent lifecycle** | Start/stop/health monitoring of multiple agent instances | Single always-on agent |

---

## Recommended Implementation Order

### Phase 1: Harden the Inner Loop (P0)

1. **Resilient agentic loop** — Add retry logic for context overflow (compact + retry up to 3x), timeout recovery (compact if >65% context used), and graceful degradation. Model the retry state machine on OpenClaw's `overflowCompactionAttempts` / `timeoutCompactionAttempts` pattern.

2. **Implement summarize compaction in agent-runtime** — The strategy exists in core's `ContextCompactionService` but the agent-runtime silently falls back to sliding-window. Wire up the summarize strategy using the agent's own model (or a cheaper compaction model).

3. **Enrich system prompt assembly** — Build a composable prompt builder with sections:
   - Identity (name, role, description, principles)
   - Available tools (with brief descriptions)
   - Memory instructions (how to use knowledge-store/query)
   - Time and timezone
   - Workspace context (injected reference files)
   - Agent-specific notes from manifest
   - Circle constitution (already done via SkillInjector)
   - Reasoning hints for thinking models

4. **Tool loop detection** — Beyond exact duplicate detection, add pattern detection for semantic loops (same tool called 3+ times in a row, oscillating between two tools, etc.).

### Phase 2: Improve Tool Quality (P1)

5. **Parallel tool execution** — When LLM returns multiple tool calls, execute them concurrently (with configurable max concurrency). Many tools (web-fetch, file-read) are I/O-bound and benefit from parallelism.

6. **Tool argument repair** — Before failing on malformed JSON from the LLM, attempt repair: strip trailing commas, fix unquoted keys, handle single quotes, trim invalid characters from tool names.

7. **Model failover in agent-runtime** — When the primary model fails (auth error, overload, timeout), try the next configured model. The core already has `ProviderHealthService` — expose ranked alternatives to the runtime.

8. **Pre-compaction memory save** — Before compacting context, inject a system message asking the agent to save any important unsaved context to knowledge-store. This prevents information loss during compaction.

9. **Context-aware tool result guard** — Before adding tool results to context, check remaining token budget. If a result would push past the high-water mark, truncate to fit rather than triggering compaction on the next iteration.

10. **Add missing tools**:
    - `image-view` — Pass images to vision-capable models for analysis
    - `pdf-read` — Extract text/tables from PDFs
    - `code-eval` — Run JavaScript/Python snippets in sandbox (beyond shell-exec)
    - `http-request` — Full HTTP client (POST, PUT, headers, auth) beyond web-fetch

### Phase 3: Advanced Features (P2)

11. **Configurable thinking levels** — Allow manifest to specify thinking depth (off/low/medium/high). Map to model-specific parameters (Claude extended thinking, DeepSeek thinking tokens).

12. **Session transcript indexing** — Index completed session transcripts into vector store for cross-session RAG. Agents can recall "what we discussed last time."

13. **Hybrid memory search** — Add full-text search alongside vector search with configurable weights. Add MMR for diversity. Add temporal decay so recent memories rank higher.

14. **Streaming tool results** — Stream tool output to the client as it executes rather than waiting for completion. Especially important for long-running shell commands.

15. **File-based skill format** — Support SKILL.md files in agent workspaces alongside code-defined builtins. Lower barrier to skill creation, enable user-authored skills without code changes.

---

## Architecture Notes

### What NOT to copy from OpenClaw

1. **No sandboxing** — OpenClaw's biggest security weakness. SERA's Docker isolation is a core differentiator. Don't weaken it.
2. **Flat config file** — OpenClaw uses a single massive `config.json`. SERA's manifest-per-agent YAML approach is cleaner for multi-agent.
3. **SQLite for everything** — Works for single-user but doesn't scale. SERA's PostgreSQL + Qdrant is the right choice for multi-agent.
4. **Community skill registry without signing** — OpenClaw's ClawHub had 341 malicious skills. If SERA adds a skill marketplace, require signing/verification.
5. **Auth disabled by default** — OpenClaw has been exploited via exposed instances. SERA correctly requires auth on all API endpoints.

### What to learn from OpenClaw

1. **Retry state machine** — The most important pattern. OpenClaw's loop is resilient because it has explicit retry budgets for each failure mode (overflow, timeout, tool truncation) and never retries the same way twice.
2. **Composable system prompts** — The prompt is the agent's "soul." A rich, context-aware system prompt dramatically improves agent quality. OpenClaw builds 15+ sections; SERA builds 3-4.
3. **Tool argument repair** — LLMs frequently produce slightly malformed JSON. Repairing it is better than failing.
4. **Pre-compaction memory save** — Simple but impactful. Prevents the most common complaint about context management: losing important context.
5. **Stream wrapping chain** — Layered transformations on the stream catch and fix issues at each level, rather than trying to handle everything in one place.
