/**
 * ContextManager — tracks token usage across the LLM message history and
 * compacts it when approaching the model's context window limit.
 *
 * Story 5.7 acceptance criteria:
 * - Token estimation via js-tiktoken (local, no round-trip to Core)
 * - MAX_CONTEXT_TOKENS env var (default: 80% of model's declared context window)
 * - CONTEXT_COMPACTION_STRATEGY: 'sliding-window' (default) | 'summarise'
 * - TOOL_OUTPUT_MAX_TOKENS: pre-truncate tool outputs before adding to history
 * - Reflect thought logged on compaction
 */

import { getEncoding, type Tiktoken } from 'js-tiktoken';
import type { ChatMessage } from './llmClient.js';
import { log } from './logger.js';

// ── Model context window sizes (tokens) ───────────────────────────────────────

const MODEL_CONTEXT_WINDOWS: Record<string, number> = {
  'gpt-4o': 128_000,
  'gpt-4o-mini': 128_000,
  'gpt-4-turbo': 128_000,
  'gpt-4': 8_192,
  'gpt-3.5-turbo': 16_385,
  'claude-opus-4': 200_000,
  'claude-sonnet-4': 200_000,
  'claude-haiku-4': 200_000,
  'claude-3-5-sonnet': 200_000,
  'claude-3-5-haiku': 200_000,
  'claude-3-opus': 200_000,
  'qwen2.5-coder-7b': 32_768,
  'qwen2.5-coder-32b': 32_768,
  'qwen3.5-35b-a3b': 131_072,
  'llama3.1:8b': 128_000,
  'llama3.2': 128_000,
};

const DEFAULT_CONTEXT_WINDOW = 32_768;
const DEFAULT_HIGH_WATER_PCT = 0.80;
const AGGRESSIVE_COMPACT_PCT = 0.50;
const DEFAULT_TOOL_OUTPUT_MAX_TOKENS = 4_000;
const DEFAULT_RESPONSE_RESERVE = 4_096;
const DEFAULT_EMERGENCY_TOOL_TOKENS = 500;
const DEFAULT_PRESERVE_RECENT_MESSAGES = 4;

export type CompactionStrategy = 'sliding-window' | 'summarise';

export interface CompactionResult {
  droppedCount: number;
  retainedCount: number;
  tokensBefore: number;
  tokensAfter: number;
  reflectMessage: string;
}

// ── ContextManager ────────────────────────────────────────────────────────────

export class ContextManager {
  private enc: Tiktoken;
  private modelName: string;
  private contextWindow: number;
  private highWaterMark: number;
  private strategy: CompactionStrategy;
  private toolOutputMaxTokens: number;
  private preserveRecentMessages: number;

  constructor(modelName: string, contextWindowOverride?: number) {
    this.modelName = modelName;
    // Priority: explicit override → CONTEXT_WINDOW env var → hardcoded lookup → default
    const envContextWindow = process.env['CONTEXT_WINDOW'];
    this.contextWindow =
      contextWindowOverride ??
      (envContextWindow ? parseInt(envContextWindow, 10) : undefined) ??
      this.resolveContextWindow(modelName);

    const maxTokensEnv = process.env['MAX_CONTEXT_TOKENS'];
    this.highWaterMark = maxTokensEnv
      ? parseInt(maxTokensEnv, 10)
      : Math.floor(this.contextWindow * DEFAULT_HIGH_WATER_PCT);

    const strategyEnv = process.env['CONTEXT_COMPACTION_STRATEGY'] as CompactionStrategy | undefined;
    this.strategy = strategyEnv === 'summarise' ? 'summarise' : 'sliding-window';

    const toolMaxEnv = process.env['TOOL_OUTPUT_MAX_TOKENS'];
    this.toolOutputMaxTokens = toolMaxEnv
      ? parseInt(toolMaxEnv, 10)
      : DEFAULT_TOOL_OUTPUT_MAX_TOKENS;

    const preserveEnv = process.env['PRESERVE_RECENT_MESSAGES'];
    this.preserveRecentMessages = preserveEnv
      ? parseInt(preserveEnv, 10)
      : DEFAULT_PRESERVE_RECENT_MESSAGES;

    // Use cl100k_base encoding as a universal approximation for any model.
    // 5-10% error is acceptable per the spec.
    this.enc = getEncoding('cl100k_base');

    log('info', `ContextManager init: model=${modelName} window=${this.contextWindow} highWater=${this.highWaterMark} strategy=${this.strategy}`);
  }

  /** Count tokens in a string. */
  countTokens(text: string): number {
    if (!text) return 0;
    return this.enc.encode(text).length;
  }

  /** Count tokens across all messages. */
  countMessageTokens(messages: ChatMessage[]): number {
    let total = 0;
    for (const msg of messages) {
      // ~4 tokens per-message overhead (role + delimiters) per OpenAI docs
      total += 4;
      total += this.countTokens(msg.content);
      if (msg.tool_calls) {
        for (const tc of msg.tool_calls) {
          total += this.countTokens(tc.function.name);
          total += this.countTokens(tc.function.arguments);
        }
      }
    }
    return total;
  }

  /**
   * Pre-truncate a tool output string to TOOL_OUTPUT_MAX_TOKENS if it exceeds
   * the limit. Always appends a truncation notice when truncated.
   */
  truncateToolOutput(content: string): string {
    const tokens = this.countTokens(content);
    if (tokens <= this.toolOutputMaxTokens) return content;

    const notice = `\n\n[SERA: output truncated — exceeded ${this.toolOutputMaxTokens} tokens]`;
    const noticeTokens = this.countTokens(notice);
    const targetTokens = this.toolOutputMaxTokens - noticeTokens;

    // Binary-search the character offset that brings us under the token limit
    let low = 0;
    let high = content.length;
    while (low < high) {
      const mid = Math.floor((low + high + 1) / 2);
      if (this.countTokens(content.slice(0, mid)) <= targetTokens) {
        low = mid;
      } else {
        high = mid - 1;
      }
    }

    return content.slice(0, low) + notice;
  }

  /**
   * Returns true when the message list is approaching the high-water mark.
   */
  isNearLimit(messages: ChatMessage[]): boolean {
    return this.countMessageTokens(messages) >= this.highWaterMark;
  }

  /**
   * Compact the message history using the configured strategy.
   * Always preserves system messages and the N most recent messages verbatim.
   *
   * @param messages   Current message history (mutated in place).
   * @returns CompactionResult describing what happened.
   */
  compact(messages: ChatMessage[]): CompactionResult {
    return this.performCompaction(messages, this.highWaterMark, 'Context compacted');
  }

  /**
   * Returns the context utilization as a ratio (0.0–1.0) of current tokens to context window.
   */
  getUtilization(messages: ChatMessage[]): number {
    return this.countMessageTokens(messages) / this.contextWindow;
  }

  /**
   * Aggressive compaction — targets 50% of the context window instead of the
   * normal 80% high-water mark. Used for overflow/timeout recovery where more
   * headroom is needed to succeed on retry.
   *
   * @param messages   Current message history (mutated in place).
   */
  aggressiveCompact(messages: ChatMessage[]): CompactionResult {
    const aggressiveTarget = Math.floor(this.contextWindow * AGGRESSIVE_COMPACT_PCT);
    return this.performCompaction(messages, aggressiveTarget, 'Aggressive compaction');
  }

  /**
   * Retroactively truncate all tool result messages to the given token limit.
   * One-shot last resort for overflow recovery.
   *
   * @returns The number of tool messages that were truncated.
   */
  truncateAllToolResults(messages: ChatMessage[], maxTokens: number = DEFAULT_EMERGENCY_TOOL_TOKENS): number {
    let truncatedCount = 0;
    for (const msg of messages) {
      if (msg.role === 'tool' && this.countTokens(msg.content) > maxTokens) {
        const notice = `\n\n[SERA: tool result retroactively truncated to ${maxTokens} tokens for overflow recovery]`;
        const noticeTokens = this.countTokens(notice);
        msg.content = this.truncateToFit(msg.content, maxTokens - noticeTokens) + notice;
        truncatedCount++;
      }
    }
    if (truncatedCount > 0) {
      log('info', `ContextManager: retroactively truncated ${truncatedCount} tool result(s) to ${maxTokens} tokens`);
    }
    return truncatedCount;
  }

  /**
   * Returns how many tokens remain before hitting the high-water mark,
   * minus a reserve for the LLM's response. Clamped to 0.
   */
  getAvailableBudget(messages: ChatMessage[], responseReserve: number = DEFAULT_RESPONSE_RESERVE): number {
    return Math.max(0, this.highWaterMark - this.countMessageTokens(messages) - responseReserve);
  }

  /**
   * Context-aware truncation: ensures a tool result fits within the remaining
   * context budget. Applied after the per-result TOOL_OUTPUT_MAX_TOKENS cap.
   *
   * @returns The (possibly truncated) content, whether the budget was exceeded,
   *          and whether compaction is needed before adding the result.
   */
  truncateToContextBudget(
    content: string,
    messages: ChatMessage[],
    responseReserve: number = DEFAULT_RESPONSE_RESERVE,
  ): { content: string; budgetExceeded: boolean; compactionNeeded: boolean } {
    const budget = this.getAvailableBudget(messages, responseReserve);

    if (budget === 0) {
      return { content, budgetExceeded: true, compactionNeeded: true };
    }

    const tokens = this.countTokens(content);
    if (tokens <= budget) {
      return { content, budgetExceeded: false, compactionNeeded: false };
    }

    const notice = `\n\n[SERA: tool result truncated to fit context budget — ${tokens} → ${budget} tokens]`;
    const noticeTokens = this.countTokens(notice);
    const truncated = this.truncateToFit(content, Math.max(1, budget - noticeTokens)) + notice;

    return { content: truncated, budgetExceeded: true, compactionNeeded: false };
  }

  /** Release the tiktoken encoding (no-op for js-tiktoken, kept for API compatibility). */
  free(): void {
    // js-tiktoken does not require explicit resource cleanup
  }

  // ── Private helpers ───────────────────────────────────────────────────────

  private resolveContextWindow(modelName: string): number {
    // Exact match
    if (MODEL_CONTEXT_WINDOWS[modelName] !== undefined) {
      return MODEL_CONTEXT_WINDOWS[modelName]!;
    }
    // Prefix match (e.g. 'qwen2.5-coder-7b-instruct' → 'qwen2.5-coder-7b')
    for (const [key, value] of Object.entries(MODEL_CONTEXT_WINDOWS)) {
      if (modelName.startsWith(key) || key.startsWith(modelName)) {
        return value;
      }
    }
    return DEFAULT_CONTEXT_WINDOW;
  }

  private truncateToFit(content: string, targetTokens: number): string {
    if (targetTokens <= 0) return '';
    let low = 0;
    let high = content.length;
    while (low < high) {
      const mid = Math.floor((low + high + 1) / 2);
      if (this.countTokens(content.slice(0, mid)) <= targetTokens) {
        low = mid;
      } else {
        high = mid - 1;
      }
    }
    return content.slice(0, low);
  }

  private performCompaction(messages: ChatMessage[], targetTokens: number, label: string): CompactionResult {
    const tokensBefore = this.countMessageTokens(messages);

    const systemMessages = messages.filter((m) => m.role === 'system');
    const nonSystemMessages = messages.filter((m) => m.role !== 'system');

    if (tokensBefore < targetTokens) {
      return {
        droppedCount: 0,
        retainedCount: nonSystemMessages.length,
        tokensBefore,
        tokensAfter: tokensBefore,
        reflectMessage: `${label}: no compaction needed`,
      };
    }

    const droppedMessages: ChatMessage[] = [];
    const keepLimit = Math.min(nonSystemMessages.length, this.preserveRecentMessages);

    // Initial drop to reach target (before accounting for summary overhead)
    while (
      nonSystemMessages.length > keepLimit &&
      this.countMessageTokens([...systemMessages, ...nonSystemMessages]) >= targetTokens
    ) {
      droppedMessages.push(nonSystemMessages.shift()!);
    }

    // Force at least one drop if we are over limit, to trigger summary injection logic
    if (droppedMessages.length === 0 && nonSystemMessages.length > 1 && tokensBefore >= targetTokens) {
      droppedMessages.push(nonSystemMessages.shift()!);
    }

    let droppedCount = droppedMessages.length;
    let continuationMsg: ChatMessage | undefined;

    if (droppedCount > 0) {
      const generateContinuation = (msgs: ChatMessage[]): ChatMessage => ({
        role: 'system',
        content: `This session is being continued from a previous conversation that ran out of context.
The summary below covers the earlier portion of the conversation.

Summary:
${this.summarizeMessages(msgs)}

Recent messages are preserved verbatim.
Continue the conversation from where it left off without asking the user any further questions.
Resume directly — do not acknowledge the summary, do not recap what was happening, and do not preface with continuation text.`,
      });

      continuationMsg = generateContinuation(droppedMessages);

      // If summary overhead pushes us back over, drop more if allowed
      while (
        nonSystemMessages.length > 1 &&
        this.countMessageTokens([...systemMessages, continuationMsg, ...nonSystemMessages]) >= targetTokens
      ) {
        droppedMessages.push(nonSystemMessages.shift()!);
        droppedCount++;
        continuationMsg = generateContinuation(droppedMessages);
      }

      // Final check: if dropping all didn't help (huge system prompt or single huge recent message),
      // keep 1 but it will be truncated later.

      // If still over, truncate the first retained message
      const total = this.countMessageTokens([...systemMessages, continuationMsg, ...nonSystemMessages]);
      if (total > targetTokens && nonSystemMessages.length > 0) {
        const first = nonSystemMessages[0]!;
        const notice = '[SERA: message truncated due to context window overflow]';
        const noticeTokens = this.countTokens(notice);
        const otherTokens = this.countMessageTokens([...systemMessages, continuationMsg, ...nonSystemMessages.slice(1)]);
        const available = targetTokens - otherTokens;
        if (available > noticeTokens) {
          first.content = this.truncateToFit(first.content, available - noticeTokens) + '\n' + notice;
        } else {
          first.content = notice;
        }
      }
      messages.splice(0, messages.length, ...systemMessages, continuationMsg, ...nonSystemMessages);
    } else {
      // No messages dropped, but might be over if a single message is huge
      const total = this.countMessageTokens([...systemMessages, ...nonSystemMessages]);
      if (total > targetTokens && nonSystemMessages.length > 0) {
        const first = nonSystemMessages[0]!;
        const notice = '[SERA: message truncated due to context window overflow]';
        const noticeTokens = this.countTokens(notice);
        const otherTokens = this.countMessageTokens([...systemMessages, ...nonSystemMessages.slice(1)]);
        const available = targetTokens - otherTokens;
        if (available > noticeTokens) {
          first.content = this.truncateToFit(first.content, available - noticeTokens) + '\n' + notice;
        } else {
          first.content = notice;
        }
      }
      messages.splice(0, messages.length, ...systemMessages, ...nonSystemMessages);
    }

    const tokensAfter = this.countMessageTokens(messages);
    const retainedCount = nonSystemMessages.length;
    const reflectMessage = `${label}: dropped ${droppedCount} messages, retained ${retainedCount} non-system messages (${tokensBefore} → ${tokensAfter} tokens)`;

    log('info', `ContextManager: ${reflectMessage}`);
    return { droppedCount, retainedCount, tokensBefore, tokensAfter, reflectMessage };
  }

  private summarizeMessages(dropped: ChatMessage[]): string {
    const counts: Record<string, number> = { user: 0, assistant: 0, tool: 0, system: 0 };
    const tools = new Set<string>();
    const userRequests: string[] = [];
    const pendingWork: string[] = [];
    const keyFiles = new Set<string>();
    let currentWork = '';

    const PENDING_KEYWORDS = ['todo', 'next', 'pending', 'follow up', 'remaining'];
    const FILE_PATH_REGEX = /\b[\w./\\-]+\.(?:ts|rs|json|md)\b/g;

    const toolCallIdToName = new Map<string, string>();
    for (const msg of dropped) {
      if (msg.tool_calls) {
        for (const tc of msg.tool_calls) {
          toolCallIdToName.set(tc.id, tc.function.name);
          tools.add(tc.function.name);
        }
      }
    }

    for (const msg of dropped) {
      counts[msg.role] = (counts[msg.role] || 0) + 1;

      if (msg.role === 'tool' && msg.tool_call_id) {
        const name = toolCallIdToName.get(msg.tool_call_id);
        if (name) tools.add(name);
      }

      const content = msg.content || '';
      if (msg.role === 'user' && content.trim()) {
        userRequests.push(content.trim().slice(0, 160));
      }

      const lowerContent = content.toLowerCase();
      if (PENDING_KEYWORDS.some((kw) => lowerContent.includes(kw))) {
        pendingWork.push(content.trim().slice(0, 160));
      }

      const matches = content.match(FILE_PATH_REGEX);
      if (matches) {
        for (const m of matches) {
          if (keyFiles.size < 8) keyFiles.add(m);
        }
      }

      if (content.trim()) {
        currentWork = content.trim();
      }
    }

    const last3UserRequests = userRequests.slice(-3);

    const timeline = dropped
      .map((msg) => {
        const truncated = (msg.content || '').trim().slice(0, 160).replace(/\s+/g, ' ');
        return `- ${msg.role}: ${truncated}`;
      })
      .join('\n');

    const summaryParts = [
      `Scope: ${counts['user']} user, ${counts['assistant']} assistant, ${counts['tool']} tool`,
      `Tools mentioned: ${Array.from(tools).join(', ') || 'none'}`,
      `Recent user requests:\n${last3UserRequests.map((r) => `- ${r}`).join('\n') || 'none'}`,
      `Pending work:\n${pendingWork.slice(-5).map((p) => `- ${p}`).join('\n') || 'none'}`,
      `Key files: ${Array.from(keyFiles).join(', ') || 'none'}`,
      `Current work: ${currentWork.slice(0, 500)}${currentWork.length > 500 ? '...' : ''}`,
      `Key timeline:\n${timeline}`,
    ];

    return summaryParts.join('\n\n');
  }
}
