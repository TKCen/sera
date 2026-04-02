/**
 * ContextCompactionService — summarizes or trims conversation history when
 * the estimated token count exceeds the model's context window high-water mark.
 *
 * Strategies:
 *  - 'summarize': Use a compaction LLM to summarize oldest messages (default when compactionModel set)
 *  - 'sliding-window': Drop oldest non-system messages until under budget
 *  - 'truncate': Hard-drop oldest messages (same as sliding-window, alias)
 *
 * @see docs/epics/04-llm-proxy-and-governance.md
 * @see #387
 */

import type { LlmRouter, ChatMessage } from './LlmRouter.js';
import type { ProviderRegistry } from './ProviderRegistry.js';
import type {
  ContextAssemblyEvent,
  ContextEventListener,
  ContextAssemblyStage,
} from './ContextAssembler.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('ContextCompactionService');

/** Number of recent messages to preserve (default: 3 user/assistant pairs). */
const DEFAULT_RECENT_K = 6;

/** Enrichment markers injected by ContextAssembler / SkillInjector — stripped before summarization. */
const ENRICHMENT_PATTERNS = [
  /<memory>[\s\S]*?<\/memory>/g,
  /<injected_memory>[\s\S]*?<\/injected_memory>/g,
  /<skills>[\s\S]*?<\/skills>/g,
  /<constitution>[\s\S]*?<\/constitution>/g,
  /<tool-descriptions>[\s\S]*?<\/tool-descriptions>/g,
  /<circle-context>[\s\S]*?<\/circle-context>/g,
];

const SUMMARIZATION_PROMPT = `Summarize the following conversation concisely, preserving key facts, decisions, tool results, and any unresolved questions. Use bullet points. Do not include greetings or pleasantries.

<conversation>
{CONVERSATION}
</conversation>`;

export class ContextCompactionService {
  constructor(
    private readonly router: LlmRouter,
    private readonly registry: ProviderRegistry
  ) {}

  /**
   * Compact a message array if it exceeds the model's context window budget.
   * Returns the (possibly compacted) message array.
   */
  async compact(
    messages: ChatMessage[],
    modelName: string,
    onEvent?: ContextEventListener
  ): Promise<ChatMessage[]> {
    const emit = (
      stage: ContextAssemblyStage,
      detail: Record<string, unknown>,
      durationMs?: number
    ) => {
      const event: ContextAssemblyEvent = { stage, detail };
      if (durationMs !== undefined) event.durationMs = durationMs;
      onEvent?.(event);
    };

    // Resolve config
    let config;
    try {
      config = this.registry.resolve(modelName);
    } catch {
      // Model not in registry — can't determine context limits, skip compaction
      emit('compaction.skipped' as ContextAssemblyStage, {
        reason: 'model not in registry',
        modelName,
      });
      return messages;
    }

    const contextWindow = config.contextWindow ?? 128_000;
    const highWaterMark = config.contextHighWaterMark ?? 0.8;
    const threshold = Math.floor(contextWindow * highWaterMark);
    const strategy =
      config.contextStrategy ?? (config.contextCompactionModel ? 'summarize' : 'sliding-window');
    const compactionModel = config.contextCompactionModel;

    // Estimate tokens
    const estimatedTokens = estimateTokens(messages);

    if (estimatedTokens < threshold) {
      emit('compaction.skipped' as ContextAssemblyStage, {
        estimatedTokens,
        threshold,
        contextWindow,
      });
      return messages;
    }

    emit('compaction.started' as ContextAssemblyStage, {
      estimatedTokens,
      threshold,
      contextWindow,
      strategy,
      messageCount: messages.length,
    });

    const start = Date.now();

    // Route by strategy
    if (strategy === 'summarize' && compactionModel) {
      try {
        const result = await this.summarize(messages, compactionModel, threshold, emit);
        emit(
          'compaction.completed' as ContextAssemblyStage,
          {
            strategy: 'summarize',
            tokensBefore: estimatedTokens,
            tokensAfter: estimateTokens(result),
            messagesDropped: messages.length - result.length,
          },
          Date.now() - start
        );
        return result;
      } catch (err) {
        logger.error('Summarization failed, falling back to sliding-window:', err);
        emit('compaction.fallback' as ContextAssemblyStage, {
          reason: err instanceof Error ? err.message : String(err),
          fallbackStrategy: 'sliding-window',
        });
        // Fall through to sliding-window
      }
    }

    // Sliding-window / truncate
    const result = this.slidingWindow(messages, threshold);
    emit(
      'compaction.completed' as ContextAssemblyStage,
      {
        strategy: 'sliding-window',
        tokensBefore: estimatedTokens,
        tokensAfter: estimateTokens(result),
        messagesDropped: messages.length - result.length,
      },
      Date.now() - start
    );
    return result;
  }

  /**
   * Summarize oldest messages using the compaction model.
   */
  private async summarize(
    messages: ChatMessage[],
    compactionModel: string,
    threshold: number,
    emit: (
      stage: ContextAssemblyStage,
      detail: Record<string, unknown>,
      durationMs?: number
    ) => void
  ): Promise<ChatMessage[]> {
    // Partition: [system, ...oldest, ...recent]
    const systemMsg = messages.find((m) => m.role === 'system');
    const nonSystem = messages.filter((m) => m.role !== 'system');
    const recentK = Math.min(DEFAULT_RECENT_K, nonSystem.length);
    const oldest = nonSystem.slice(0, nonSystem.length - recentK);
    const recent = nonSystem.slice(nonSystem.length - recentK);

    if (oldest.length === 0) {
      // Nothing to summarize — fall back to sliding-window
      return this.slidingWindow(messages, threshold);
    }

    // Strip enrichment markers from messages before summarizing
    const cleanOldest = oldest.map((m) => {
      let content = '';
      if (typeof m.content === 'string') {
        content = m.content;
      } else if (Array.isArray(m.content)) {
        content = m.content
          .map((c) => (c.type === 'text' ? c.text : '[image]'))
          .join('')
          .trim();
      }
      return {
        ...m,
        content: stripEnrichment(content),
      };
    });

    // Format conversation for the summarization prompt
    const conversationText = cleanOldest.map((m) => `${m.role}: ${m.content ?? ''}`).join('\n\n');

    const prompt = SUMMARIZATION_PROMPT.replace('{CONVERSATION}', conversationText);

    emit('compaction.summarizing' as ContextAssemblyStage, {
      oldestCount: oldest.length,
      recentCount: recent.length,
      compactionModel,
      promptTokens: estimateTokensStr(prompt),
    });

    const start = Date.now();

    // Call compaction model (non-streaming)
    const { response } = await this.router.chatCompletion(
      {
        model: compactionModel,
        messages: [{ role: 'user', content: prompt }],
        temperature: 0.3,
      },
      '_compaction_service',
      start
    );

    const summary = response.choices[0]?.message?.content ?? '';

    emit('compaction.summarized' as ContextAssemblyStage, {
      summaryTokens: estimateTokensStr(summary),
      durationMs: Date.now() - start,
    });

    // Reassemble: [system, summary-user, summary-ack-assistant, ...recent]
    const result: ChatMessage[] = [];
    if (systemMsg) result.push(systemMsg);
    result.push({
      role: 'user',
      content: `[Previous conversation summary — ${oldest.length} messages compacted]:\n${summary}`,
    });
    result.push({
      role: 'assistant',
      content:
        "Understood, I have the context from the conversation summary. I'll continue from here.",
    });
    result.push(...recent);

    return result;
  }

  /**
   * Drop oldest non-system messages until estimated tokens are under threshold.
   * Preserves recent K messages when possible.
   */
  private slidingWindow(messages: ChatMessage[], threshold: number): ChatMessage[] {
    const systemMsg = messages.find((m) => m.role === 'system');
    const nonSystem = messages.filter((m) => m.role !== 'system');

    // Start with system + all non-system, drop from the front
    const result: ChatMessage[] = [...nonSystem];
    while (result.length > DEFAULT_RECENT_K) {
      const candidate = systemMsg ? [systemMsg, ...result] : [...result];
      if (estimateTokens(candidate) <= threshold) break;
      result.shift();
    }

    return systemMsg ? [systemMsg, ...result] : result;
  }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/** Estimate tokens for a message array using char/4 approximation. */
function estimateTokens(messages: ChatMessage[]): number {
  let chars = 0;
  for (const m of messages) {
    // ~4 tokens overhead per message (role, delimiters)
    chars += 16;
    let content = '';
    if (typeof m.content === 'string') {
      content = m.content;
    } else if (Array.isArray(m.content)) {
      content = m.content
        .map((c) => (c.type === 'text' ? c.text : '[image]'))
        .join('')
        .trim();
    }
    chars += content.length;
    if (m.tool_calls) {
      chars += JSON.stringify(m.tool_calls).length;
    }
  }
  return Math.ceil(chars / 4);
}

/** Estimate tokens for a single string. */
function estimateTokensStr(text: string): number {
  return Math.ceil(text.length / 4);
}

/** Remove enrichment XML blocks from a message's content. */
function stripEnrichment(content: string): string {
  let stripped = content;
  for (const pattern of ENRICHMENT_PATTERNS) {
    stripped = stripped.replace(pattern, '');
  }
  // Clean up leftover double newlines from removed blocks
  return stripped.replace(/\n{3,}/g, '\n\n').trim();
}
