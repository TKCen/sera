# KV Cache Optimization — Design Document

> **Status:** Draft
> **Author:** Architect agent
> **Date:** 2026-04-07
> **Scope:** agent-runtime context assembly, Core LLM proxy, provider-specific cache hints

---

## 1. Current State

### 1.1 Context Assembly Flow

SERA has a **two-layer context assembly** architecture. Requests flow through both layers sequentially:

**Layer 1 — Agent-Runtime (in-container)**
The `ReasoningLoop` in `core/agent-runtime/src/loop.ts:329–343` builds the initial message array:

```
messages = [
  { role: 'system', content: systemPrompt },        // line 329
  { role: 'system', content: bootContext },           // line 331–337 (if present)
  ...history,                                         // line 339
  { role: 'user', content: task }                     // line 340–343
]
```

The system prompt is built by `SystemPromptBuilder` (`systemPromptBuilder.ts`) with priority-ordered sections:

| Priority | Section ID            | Stability    | Required |
| -------- | --------------------- | ------------ | -------- |
| 0        | identity              | Static       | Yes      |
| 5        | core-memory           | Semi-static  | Yes      |
| 10       | principles            | Static       | No       |
| 20       | communication-style   | Static       | No       |
| 30       | available-tools       | Per-run\*    | Yes      |
| 40       | tool-usage-guidelines | Static       | Yes      |
| 50       | memory-instructions   | Static       | Yes      |
| 55       | memory-management     | Static       | Yes      |
| 60       | **time-context**      | **Per-call** | Yes      |
| 70       | circle-context        | Semi-static  | No       |
| 80       | delegation-context    | Semi-static  | No       |
| 90       | agent-notes           | Static       | No       |
| 100      | workspace-context     | Semi-static  | No       |
| 110      | reasoning-hints       | Static       | No       |
| 120      | constraints           | Static       | Yes      |
| 130      | output-format         | Static       | No       |

\*Tool definitions change per-run due to keyword-based gating (`selectActiveGroups` at loop.ts:102–136) but are stable within a run.

**Layer 2 — Core LLM Proxy (server-side)**
When the agent-runtime calls `/v1/llm/chat/completions`, the proxy (`core/src/routes/llmProxy.ts:144–188`) runs `ContextAssembler.assemble()` which:

1. Appends circle constitution to the system prompt (`ContextAssembler.ts:186–189`)
2. Injects skills via `SkillInjector` (`ContextAssembler.ts:195–201`)
3. Performs RAG memory retrieval and appends `<injected_memory>` blocks (`ContextAssembler.ts:233–244`)

Then optionally runs `ContextCompactionService.compact()` (`llmProxy.ts:195–209`).

**Layer 3 — LlmRouter → Provider**
`LlmRouter.chatCompletionStream()` dispatches to pi-mono provider functions. No provider-specific message transformation occurs — the OpenAI-compatible format is forwarded as-is.

### 1.2 Tool Definitions

Tools are sent as the `tools` parameter on every LLM call (`loop.ts:524–525`). The tool set is:

- Filtered per-run by keyword gating (`selectActiveGroups`)
- Capped at `CORE_TOOL_LIMIT = 12` tools per call
- Stable within a single run (same task = same tools)

Tool schemas are defined as static `BUILTIN_TOOLS` in `tools/definitions.ts`. The order depends on array filtering, which uses `Array.filter()` — **deterministic given the same input**, but the gating itself can produce different tool sets across runs.

### 1.3 History Management

The `ContextManager` (`contextManager.ts`) implements two compaction strategies:

- **sliding-window** (default): Drops oldest non-system messages from the front (`performCompaction`, line 552–559)
- **summarise**: Uses an LLM call to summarize dropped messages (`performSummarizeCompaction`, line 678–765)

Both strategies preserve system messages and the N most recent messages (default: 4).

Additional mechanisms:

- `clearOldToolResults()` (line 207–232): Replaces old tool results with `[cleared — re-read if needed]` placeholder
- `truncateToolOutput()` (line 168–189): Pre-truncates individual tool outputs to `TOOL_OUTPUT_MAX_TOKENS`
- Pre-compaction memory flush (loop.ts:419–501): Gives agent one turn to save context before compaction

### 1.4 Serialization

- Agent-runtime uses `safeStringify()` (`json.ts:107–120`) which uses `JSON.stringify` with a circular-reference replacer — **no sorted keys, no stable whitespace**
- Tool definitions use standard `JSON.stringify` via axios serialization
- The Core proxy re-serializes the entire request body before forwarding

---

## 2. Gap Analysis — Anti-Patterns for KV Cache

### 2.1 [CRITICAL] Time Context Embedded in System Prompt (Priority 60)

**File:** `systemPromptBuilder.ts:254–268`

The `addTimeContext()` method embeds `new Date().toISOString()` directly into the system prompt. This changes **every single call**, invalidating the entire KV cache for everything after this point in the system message.

Since the time context is at priority 60, it sits in the **middle** of the system prompt — between tool usage guidelines (priority 40–55) and circle/delegation context (priority 70–80). This means roughly 40–60% of the system prompt that follows the timestamp is also invalidated on every call.

**Impact:** Catastrophic. Every LLM call is a complete cache miss for the system prompt suffix. For a typical 3000-token system prompt, ~1500 tokens of cacheable content after the timestamp are wasted on every turn.

### 2.2 [CRITICAL] Sliding-Window Compaction Invalidates Prefix

**File:** `contextManager.ts:552–559`

The sliding-window strategy uses `nonSystemMessages.shift()` to drop messages from the front of history. This changes the content at the first non-system message position, which invalidates the KV cache for the **entire conversation history** that follows.

For Anthropic's explicit caching (which allows up to 4 breakpoints), this means a cache miss on every compaction event. For OpenAI's automatic caching, the 1024-token prefix match is preserved only if the system prompt is long enough to absorb the change.

**Impact:** High. Every compaction event (triggered at 95% utilization) causes a full cache rebuild for conversation history.

### 2.3 [HIGH] Dynamic RAG Content Injected Into System Prompt

**File:** `ContextAssembler.ts:244`

The assembled system prompt has RAG memory blocks (`<injected_memory>`) appended at the end:

```typescript
const newSystemContent = memoryContext ? `${skillsPrompt}\n\n${memoryContext}` : skillsPrompt;
```

Since RAG results change per query (different embeddings, different top-K results, different scores), the system message content changes every turn. Because this is a single system message, any change invalidates the cache for everything after the first differing token.

**Impact:** High. The system prompt effectively becomes fully volatile because RAG content is appended to it rather than placed in a separate message.

### 2.4 [MEDIUM] Non-Deterministic Serialization

**File:** `json.ts:107–120`, `llmClient.ts:172–181`

`safeStringify()` and the default `JSON.stringify` used throughout do not sort object keys. While JavaScript V8/Bun preserve insertion order (so the same code path produces the same key order), any refactoring, dynamic object construction, or spread operator usage can silently change key order, causing cache invalidation.

Tool definition objects in `definitions.ts` are static literals, so their serialization is currently stable. But the message construction in `llmClient.ts:172–181` uses conditional spreads:

```typescript
...(m.tool_calls ? { tool_calls: m.tool_calls } : {}),
...(m.tool_call_id ? { tool_call_id: m.tool_call_id } : {}),
```

This produces different key orders depending on whether `tool_calls` or `tool_call_id` are present.

**Impact:** Medium. Currently stable in practice due to V8 insertion-order guarantees, but fragile and a latent risk.

### 2.5 [MEDIUM] No Provider-Specific Cache Hints

**File:** `llmClient.ts` (agent-runtime), `LlmRouter.ts` (core)

- **Anthropic:** No `cache_control` breakpoints are set anywhere in the codebase. The `cache_creation_input_tokens` and `cache_read_input_tokens` fields are parsed from responses (llmClient.ts:327–328) but nothing is done to _optimize_ for them.
- **OpenAI:** No awareness of the 1024-token minimum prefix or 128-token alignment.
- **Gemini:** No use of the Context Caching API for the large system prompts that would benefit from it.

**Impact:** Medium. Anthropic explicitly requires `cache_control` breakpoints to enable prompt caching — without them, **no caching occurs at all** on Anthropic models. This is a missed 90% cost reduction on cached tokens.

### 2.6 [LOW] Tool Set Changes Between Runs

**File:** `loop.ts:246–280`

The `selectActiveGroups()` function selects tool groups based on keyword matching against the task string. Different tasks activate different tool groups, changing the `tools` parameter. Since tools are typically serialized before conversation history in the API request, this invalidates the cache for all subsequent content.

Within a single run (same task), the tool set is stable. But across runs for the same agent, it varies.

**Impact:** Low for single-run optimization (tools are stable within a run), but relevant for persistent agents handling sequential tasks.

### 2.7 [LOW] Boot Context as Separate System Message

**File:** `loop.ts:331–337`

Boot context is injected as a second system message. Some providers (OpenAI) may handle multiple system messages differently than a single concatenated one, affecting cache prefix matching.

**Impact:** Low. Most providers treat consecutive system messages equivalently.

---

## 3. Proposed Changes

### P0: Static-to-Dynamic Reordering of Context Payload

**Goal:** Ensure the most stable content occupies the leftmost positions in the message array, maximizing prefix cache hits.

#### P0.1 — Move Time Context to End of System Prompt

**File:** `core/agent-runtime/src/systemPromptBuilder.ts`
**Change:** Move `time-context` from priority 60 to priority 135 (after output-format at 130).

```typescript
// Change in addTimeContext():
addTimeContext(timezone: string = 'UTC'): this {
  // ...existing code...
  return this.addSection({
    id: 'time-context',
    priority: 135,  // was: 60 — moved to end to preserve cache prefix
    content: lines.join('\n'),
    required: true,
  });
}
```

Better yet, reduce the timestamp granularity to hourly (truncate minutes/seconds) to extend cache lifetime:

```typescript
const now = new Date();
// Truncate to hour for cache stability
const hourTruncated = new Date(now);
hourTruncated.setMinutes(0, 0, 0);
```

**Effort:** Trivial (2 lines changed)
**Impact:** Immediately recovers ~40–60% of system prompt cacheability.

#### P0.2 — Separate RAG Content From System Prompt

**File:** `core/src/llm/ContextAssembler.ts`
**Change:** Instead of appending `<injected_memory>` to the system message content, inject it as a separate user message immediately before the latest user message.

```typescript
// Before (line 289):
return messages.map((m) => (m.role === 'system' ? { ...m, content: newSystemContent } : m));

// After:
const enrichedMessages = messages.map((m) =>
  m.role === 'system' ? { ...m, content: skillsPrompt } : m
);
if (memoryContext) {
  // Insert RAG context as a separate message before the last user message
  const lastUserIdx = enrichedMessages.findLastIndex((m) => m.role === 'user');
  if (lastUserIdx !== -1) {
    enrichedMessages.splice(lastUserIdx, 0, {
      role: 'user',
      content: `[Retrieved context]\n${memoryContext}`,
    });
  }
}
return enrichedMessages;
```

**Effort:** Small (10 lines)
**Impact:** System prompt becomes fully stable across turns (assuming P0.1 is also applied). Only the RAG message and latest user message change per turn.

#### P0.3 — Reorder Message Array for Static-to-Dynamic

**File:** `core/agent-runtime/src/loop.ts`
**Change:** Ensure the message array follows this structure:

```
[0] system prompt          — stable across entire run
[1] system: boot context   — stable across entire run
[2..N] conversation history — append-only within run
[N+1] user: latest input   — changes each turn
```

The current code already follows this pattern (loop.ts:329–343). No change needed here, but document it as an invariant with a code comment.

**Effort:** None (documentation only)

### P1: Append-Only History with Stepped Truncation

**Goal:** When compaction is needed, truncate from the beginning of history but at stable "step" boundaries to minimize cache invalidation.

#### P1.1 — Stepped Sliding-Window Compaction

**File:** `core/agent-runtime/src/contextManager.ts`
**Change:** Instead of dropping messages one at a time until under budget, drop in fixed-size steps (e.g., 4 messages at a time, aligned to tool-call/result pairs). This ensures that the remaining prefix after compaction has a higher chance of matching a previous cache entry.

```typescript
// In performCompaction(), replace the while loop (lines 557–560):
const COMPACTION_STEP = 4; // Drop in groups of 4 (roughly 2 tool-call/result pairs)
while (
  nonSystemMessages.length > keepLimit &&
  this.countMessageTokens([...systemMessages, ...nonSystemMessages]) >= targetTokens
) {
  const toDrop = Math.min(COMPACTION_STEP, nonSystemMessages.length - keepLimit);
  for (let i = 0; i < toDrop; i++) {
    droppedMessages.push(nonSystemMessages.shift()!);
  }
}
```

**Effort:** Small
**Impact:** Moderate. Reduces the frequency of prefix changes during compaction.

#### P1.2 — Preserve System Message Prefix on Compaction

**File:** `core/agent-runtime/src/contextManager.ts`
**Change:** When injecting a compaction summary, place it as the **last** system message rather than between the system prompt and conversation history. This preserves the system prompt prefix for caching.

Currently (line 634):

```typescript
messages.splice(0, messages.length, ...systemMessages, continuationMsg, ...nonSystemMessages);
```

The `continuationMsg` is a system message inserted between the original system messages and the retained conversation. This is fine for cache purposes as long as the original system messages are unchanged — which they are. The continuation message does change between compactions, but it comes after the stable system prefix. **No change needed** — the current ordering is already correct for this concern.

**Effort:** None
**Impact:** Confirms current behavior is cache-friendly in this regard.

### P2: Deterministic Serialization

**Goal:** Ensure identical logical payloads produce identical byte sequences.

#### P2.1 — Sorted-Key JSON Serialization for LLM Requests

**File:** `core/agent-runtime/src/json.ts`
**Change:** Add a `stableStringify` function that sorts keys recursively:

```typescript
export function stableStringify(value: unknown, indent?: number): string {
  return JSON.stringify(sortKeys(value), null, indent);
}

function sortKeys(val: unknown): unknown {
  if (val === null || typeof val !== 'object') return val;
  if (Array.isArray(val)) return val.map(sortKeys);
  return Object.fromEntries(
    Object.entries(val as Record<string, unknown>)
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([k, v]) => [k, sortKeys(v)])
  );
}
```

**File:** `core/agent-runtime/src/llmClient.ts`
**Change:** Use `stableStringify` instead of `safeStringify` for the request body (line 207):

```typescript
const safeBody = stableStringify(body);
```

**Effort:** Small (new function + 1 line change)
**Impact:** Low-to-medium. Eliminates a class of subtle cache-busting bugs from key ordering changes. More important as a defensive measure than an immediate win.

#### P2.2 — Stable Tool Definition Ordering

**File:** `core/agent-runtime/src/loop.ts`
**Change:** After tool gating and filtering, sort the tool definitions alphabetically by name before sending:

```typescript
this.toolDefs = gatedToolDefs.sort((a, b) => a.function.name.localeCompare(b.function.name));
```

**Effort:** Trivial (1 line)
**Impact:** Low. Ensures that even if internal filtering order changes, the serialized output is stable.

### P3: Provider-Specific Cache Hints

**Goal:** Exploit each provider's caching mechanism for maximum hit rates.

#### P3.1 — Anthropic `cache_control` Breakpoints

**File:** New file `core/src/llm/providers/AnthropicCacheAdapter.ts` (or within `LlmRouter.ts`)
**Change:** Before dispatching to Anthropic, inject `cache_control: { type: 'ephemeral' }` breakpoints at up to 4 strategic positions:

1. **End of system prompt** — the entire system prompt is cached
2. **End of tool definitions** — tool schemas are cached
3. **End of static context** (boot context / RAG) — cached across turns
4. **End of conversation history minus last 2 messages** — grows with conversation

Implementation requires detecting the provider from model name and transforming the messages array:

```typescript
function injectAnthropicCacheBreakpoints(messages: ChatMessage[], tools?: ToolDefinition[]): void {
  // Breakpoint 1: system message
  const systemMsg = messages.find((m) => m.role === 'system');
  if (systemMsg) {
    (systemMsg as any).cache_control = { type: 'ephemeral' };
  }

  // Breakpoint 2: last tool definition (if tools provided)
  if (tools?.length) {
    (tools[tools.length - 1] as any).cache_control = { type: 'ephemeral' };
  }

  // Breakpoint 3: conversation history (mark the message 2 before last)
  const nonSystem = messages.filter((m) => m.role !== 'system');
  if (nonSystem.length > 2) {
    const target = nonSystem[nonSystem.length - 3];
    if (target) (target as any).cache_control = { type: 'ephemeral' };
  }
}
```

**Effort:** Medium (new adapter + provider detection logic)
**Impact:** Very high for Anthropic. Prompt caching reduces input token cost by 90% and latency by ~50% for cached prefixes. For a typical 8000-token system prompt, this saves ~7200 tokens of processing per turn.

#### P3.2 — OpenAI Prefix Padding for 128-Token Alignment

**File:** `core/agent-runtime/src/systemPromptBuilder.ts` or new adapter
**Change:** Pad the system prompt to the nearest 128-token boundary to maximize cache utilization. OpenAI caches in 128-token increments starting at 1024 tokens.

```typescript
function padTo128Boundary(prompt: string, tokenCounter: (s: string) => number): string {
  const tokens = tokenCounter(prompt);
  if (tokens < 1024) return prompt; // Below minimum, no caching anyway
  const remainder = tokens % 128;
  if (remainder === 0) return prompt;
  const paddingTokens = 128 - remainder;
  // Add whitespace padding (approximately 4 chars per token)
  return prompt + '\n' + ' '.repeat(paddingTokens * 4);
}
```

**Effort:** Small
**Impact:** Low. OpenAI's automatic caching already works well; this is a marginal improvement (~64 tokens average waste reduction per call).

#### P3.3 — Gemini Context Caching for Large System Prompts

**File:** New adapter or `LlmRouter.ts`
**Change:** For Gemini models with system prompts exceeding 32K tokens, use the explicit Context Caching API to create a cached context object. This is most relevant for agents with large boot contexts or workspace context files.

This requires:

1. Computing a content hash of the system prompt + tools
2. Checking if a cached context exists for this hash
3. If not, creating one via the Gemini caching API with a TTL
4. Referencing the cached context in subsequent requests

**Effort:** High (new API integration, cache lifecycle management)
**Impact:** High for Gemini with large contexts. Reduces per-request cost by up to 75% for contexts over 32K tokens. Most SERA agents have smaller contexts, so this is primarily valuable for research/analysis agents with large boot context directories.

---

## 4. Implementation Plan

### Phase 1 — Quick Wins (P0) — Est. 1–2 days

| #   | Change                               | File(s)                          | Complexity |
| --- | ------------------------------------ | -------------------------------- | ---------- |
| 1   | Move time-context to priority 135    | `systemPromptBuilder.ts:264`     | Trivial    |
| 2   | Truncate timestamp to hourly         | `systemPromptBuilder.ts:255–258` | Trivial    |
| 3   | Separate RAG from system prompt      | `ContextAssembler.ts:244,289`    | Small      |
| 4   | Add cache-ordering invariant comment | `loop.ts:329`                    | Trivial    |

**Tests:** Update `systemPromptBuilder.test.ts` priority assertions. Add test verifying RAG content is in a separate message. Verify existing loop tests still pass.

### Phase 2 — History Stability (P1) — Est. 1 day

| #   | Change                               | File(s)                     | Complexity |
| --- | ------------------------------------ | --------------------------- | ---------- |
| 5   | Stepped compaction (groups of 4)     | `contextManager.ts:557–560` | Small      |
| 6   | Sort tool definitions alphabetically | `loop.ts:253–274`           | Trivial    |

**Tests:** Update compaction tests to verify step-based dropping. Add test for tool ordering stability.

### Phase 3 — Serialization (P2) — Est. 0.5 days

| #   | Change                           | File(s)            | Complexity |
| --- | -------------------------------- | ------------------ | ---------- |
| 7   | Add `stableStringify` to json.ts | `json.ts`          | Small      |
| 8   | Use stableStringify in llmClient | `llmClient.ts:207` | Trivial    |

**Tests:** Unit test for `stableStringify` with nested objects, arrays, various key orders.

### Phase 4 — Provider Hints (P3) — Est. 3–5 days

| #   | Change                              | File(s)                             | Complexity |
| --- | ----------------------------------- | ----------------------------------- | ---------- |
| 9   | Anthropic cache_control breakpoints | New adapter + `LlmRouter.ts`        | Medium     |
| 10  | OpenAI 128-token padding            | `systemPromptBuilder.ts` or adapter | Small      |
| 11  | Gemini Context Caching API          | New adapter + cache lifecycle       | High       |

**Tests:** Provider-specific unit tests with mocked API calls. Integration test verifying cache_control fields are present in Anthropic requests.

### Dependency Order

```
Phase 1 (P0) ← no dependencies, start immediately
Phase 2 (P1) ← independent of Phase 1
Phase 3 (P2) ← independent of Phase 1/2
Phase 4 (P3) ← benefits from Phase 1 (stable prefix makes breakpoints more effective)
```

All phases can be developed in parallel, but Phase 1 should be deployed first for maximum impact.

---

## 5. Expected Impact

### Cost Reduction Estimates

| Provider      | Current Cache Hit Rate                     | Projected Cache Hit Rate    | Token Cost Reduction   |
| ------------- | ------------------------------------------ | --------------------------- | ---------------------- |
| **Anthropic** | ~0% (no cache_control)                     | 60–80% (with breakpoints)   | 50–70% on input tokens |
| **OpenAI**    | 10–30% (auto, but timestamp breaks prefix) | 50–70% (stable prefix)      | 25–40% on input tokens |
| **Gemini**    | ~0% (under 32K threshold)                  | 20–40% (for large contexts) | 15–30% on input tokens |

### Latency Reduction Estimates

- **Anthropic:** Cached prefixes reduce TTFT (time to first token) by ~50%. For a 5-turn conversation with 8K system prompt, this saves ~1–2 seconds per turn.
- **OpenAI:** Automatic caching provides ~30% latency reduction on cached prefixes. Fixing the timestamp issue extends this to more of the prompt.
- **Gemini:** Context caching reduces latency proportional to cached context size.

### Per-Phase Impact Breakdown

| Phase               | Effort   | Cost Impact                           | Latency Impact |
| ------------------- | -------- | ------------------------------------- | -------------- |
| P0 (reorder)        | 1–2 days | High (enables all other caching)      | Medium         |
| P1 (history)        | 1 day    | Medium (reduces compaction churn)     | Low            |
| P2 (serialization)  | 0.5 days | Low (defensive, prevents regressions) | Negligible     |
| P3 (provider hints) | 3–5 days | Very High (especially Anthropic)      | High           |

### ROI Prioritization

**P0 + P3.1 (Anthropic breakpoints)** together deliver ~80% of the total possible improvement. A SERA agent making 50 LLM calls per task with an 8K-token system prompt currently processes ~400K input tokens. With P0+P3.1, roughly 240K of those tokens would be cache hits at 10% of the cost, saving ~216K token-equivalents of cost per task.

---

## 6. Observability

### Metrics to Track

The agent-runtime already parses `cache_creation_input_tokens` and `cache_read_input_tokens` from responses (`llmClient.ts:327–328`) and accumulates them in usage tracking (`loop.ts:296–299`). To validate this optimization:

1. **Cache hit ratio per turn:** `cacheReadTokens / (promptTokens + cacheReadTokens)` — available from existing `TaskOutput.usage`
2. **Cache hit ratio per agent:** Aggregate across runs in metering service
3. **System prompt token stability:** Log a hash of the system prompt per turn; consecutive identical hashes indicate cache-friendly behavior
4. **Compaction frequency:** Already tracked via thought stream events

### Dashboard Addition

Add a "Cache Efficiency" panel to the agent dashboard showing:

- Cache read tokens vs. total prompt tokens (per agent, per model)
- Cache creation events (indicates cache misses)
- Trend over time (should increase after optimization)

---

## 7. Risks and Trade-offs

| Risk                                                                                   | Mitigation                                                                                                                             |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| Moving time-context to end changes agent behavior if agents rely on timestamp position | Timestamp content is unchanged; only its position in the prompt changes. No agent logic references position.                           |
| Separating RAG from system prompt may reduce RAG influence on reasoning                | RAG content in a separate user message is still strongly attended to. Empirical testing recommended.                                   |
| Anthropic cache_control adds API-specific fields to generic message format             | Use a provider adapter layer that transforms just before dispatch. Keep the internal format provider-agnostic.                         |
| Hourly timestamp truncation reduces temporal precision                                 | Agents that need sub-hour precision can use `shell-exec date` or the user message timestamp. System prompt precision is rarely needed. |
| stableStringify has ~10–15% overhead vs JSON.stringify                                 | Only applied to the LLM request body (once per turn). Negligible compared to LLM latency.                                              |
| Gemini Context Caching API has a TTL and storage cost                                  | Set TTL to match typical session duration (1 hour). Only activate for agents with >32K context.                                        |

---

## References

- `core/agent-runtime/src/systemPromptBuilder.ts:254–268` — time-context section (P0.1 target)
- `core/agent-runtime/src/loop.ts:329–343` — message array construction
- `core/agent-runtime/src/loop.ts:246–280` — tool gating and filtering
- `core/agent-runtime/src/contextManager.ts:503–676` — compaction logic
- `core/agent-runtime/src/contextManager.ts:207–232` — clearOldToolResults
- `core/src/llm/ContextAssembler.ts:111–290` — core-side context assembly
- `core/src/llm/ContextAssembler.ts:244` — RAG injection into system prompt
- `core/src/llm/ContextCompactionService.ts:44–249` — core-side compaction
- `core/agent-runtime/src/llmClient.ts:164–300` — LLM client (request formatting)
- `core/agent-runtime/src/llmClient.ts:327–328` — cache token parsing
- `core/agent-runtime/src/json.ts:107–120` — safeStringify (no sorted keys)
- `core/agent-runtime/src/tools/definitions.ts:20–416` — static tool definitions
- `core/agent-runtime/src/manifest.ts:149–214` — generateSystemPrompt
- `core/agent-runtime/src/bootContext.ts:17–117` — boot context loading
- `core/src/routes/llmProxy.ts:144–209` — proxy context assembly + compaction
- `core/src/llm/LlmRouter.ts:553–623` — provider dispatch (no provider-specific transforms)
