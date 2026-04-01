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
const DEFAULT_EMERGENCY_TOOL_TOKENS = 500;

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
   * Always preserves the system prompt (first message if role === 'system').
   *
   * Strategy 'sliding-window': drops oldest non-system messages until under limit.
   * Strategy 'summarise': not implemented in the runtime (requires a synchronous
   *   LLM call) — falls back to sliding-window with a warning.
   *
   * @param messages   Current message history (mutated in place).
   * @returns CompactionResult describing what happened.
   */
  compact(messages: ChatMessage[]): CompactionResult {
    const tokensBefore = this.countMessageTokens(messages);

    if (this.strategy === 'summarise') {
      // DECISION: 'summarise' requires a synchronous LLM round-trip which is
      // complex to fit in the synchronous compact() interface. Falling back to
      // sliding-window and recording a warning in the reflect message.
      log('warn', 'ContextManager: summarise strategy not yet implemented — using sliding-window');
    }

    // Identify system messages and mutable messages separately
    const systemMessages = messages.filter((m) => m.role === 'system');
    const nonSystemMessages = messages.filter((m) => m.role !== 'system');

    let droppedCount = 0;
    while (
      nonSystemMessages.length > 1 &&
      this.countMessageTokens([...systemMessages, ...nonSystemMessages]) >= this.highWaterMark
    ) {
      nonSystemMessages.shift();
      droppedCount++;
    }

    // If a single non-system message still puts us over (e.g. enormous tool result),
    // truncate it rather than silently drop the system prompt.
    if (nonSystemMessages.length > 0) {
      const first = nonSystemMessages[0]!;
      const totalWithFirst = this.countMessageTokens([...systemMessages, ...nonSystemMessages]);
      if (totalWithFirst > this.contextWindow) {
        const notice = '[SERA: message truncated due to context window overflow]';
        first.content = this.truncateToFit(
          first.content,
          this.highWaterMark - this.countMessageTokens([...systemMessages, ...nonSystemMessages.slice(1)]),
        ) + '\n' + notice;
      }
    }

    // Rebuild the messages array in place
    messages.splice(0, messages.length, ...systemMessages, ...nonSystemMessages);

    const tokensAfter = this.countMessageTokens(messages);
    const retainedCount = nonSystemMessages.length;
    const reflectMessage = `Context compacted: dropped ${droppedCount} messages, retained ${retainedCount} non-system messages (${tokensBefore} → ${tokensAfter} tokens)`;

    log('info', `ContextManager: ${reflectMessage}`);
    return { droppedCount, retainedCount, tokensBefore, tokensAfter, reflectMessage };
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
    const tokensBefore = this.countMessageTokens(messages);
    const aggressiveTarget = Math.floor(this.contextWindow * AGGRESSIVE_COMPACT_PCT);

    const systemMessages = messages.filter((m) => m.role === 'system');
    const nonSystemMessages = messages.filter((m) => m.role !== 'system');

    let droppedCount = 0;
    while (
      nonSystemMessages.length > 1 &&
      this.countMessageTokens([...systemMessages, ...nonSystemMessages]) >= aggressiveTarget
    ) {
      nonSystemMessages.shift();
      droppedCount++;
    }

    // Emergency truncation if a single message still overflows
    if (nonSystemMessages.length > 0) {
      const totalWithFirst = this.countMessageTokens([...systemMessages, ...nonSystemMessages]);
      if (totalWithFirst > this.contextWindow) {
        const first = nonSystemMessages[0]!;
        const notice = '[SERA: message truncated due to aggressive compaction]';
        first.content = this.truncateToFit(
          first.content,
          aggressiveTarget - this.countMessageTokens([...systemMessages, ...nonSystemMessages.slice(1)]),
        ) + '\n' + notice;
      }
    }

    messages.splice(0, messages.length, ...systemMessages, ...nonSystemMessages);

    const tokensAfter = this.countMessageTokens(messages);
    const retainedCount = nonSystemMessages.length;
    const reflectMessage = `Aggressive compaction: dropped ${droppedCount} messages, retained ${retainedCount} non-system messages (${tokensBefore} → ${tokensAfter} tokens)`;

    log('info', `ContextManager: ${reflectMessage}`);
    return { droppedCount, retainedCount, tokensBefore, tokensAfter, reflectMessage };
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
}
