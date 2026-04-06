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
import type { ChatMessage, ILLMClient } from './llmClient.js';
import { log } from './logger.js';

const DEFAULT_CONTEXT_WINDOW = 128_000;
const DEFAULT_HIGH_WATER_PCT = 0.95;
const DEFAULT_CLEAR_THRESHOLD_PCT = 0.8;
const AGGRESSIVE_COMPACT_PCT = 0.5;
const DEFAULT_TOOL_OUTPUT_MAX_TOKENS = 4_000;
const DEFAULT_RESPONSE_RESERVE = 4_096;
const DEFAULT_EMERGENCY_TOOL_TOKENS = 500;
const DEFAULT_PRESERVE_RECENT_MESSAGES = 4;

/** Enrichment markers injected by ContextAssembler / SkillInjector — stripped before summarization. */
const ENRICHMENT_PATTERNS = [
  /<memory>[\s\S]*?<\/memory>/g,
  /<skills>[\s\S]*?<\/skills>/g,
  /<constitution>[\s\S]*?<\/constitution>/g,
  /<context>[\s\S]*?<\/context>/g,
  /<tool-descriptions>[\s\S]*?<\/tool-descriptions>/g,
  /<circle-context>[\s\S]*?<\/circle-context>/g,
];

export type CompactionStrategy = 'sliding-window' | 'summarise';

export interface CompactionResult {
  droppedCount: number;
  retainedCount: number;
  tokensBefore: number;
  tokensAfter: number;
  reflectMessage: string;
  strategy: CompactionStrategy;
  isFallback?: boolean;
}

// ── ContextManager ────────────────────────────────────────────────────────────

export class ContextManager {
  private enc: Tiktoken;
  private modelName: string;
  private contextWindow: number;
  private highWaterMark: number;
  private clearThreshold: number;
  private strategy: CompactionStrategy;
  private toolOutputMaxTokens: number;
  private preserveRecentMessages: number;
  private memoryFlushEnabled: boolean;

  constructor(modelName: string, contextWindowOverride?: number) {
    this.modelName = modelName;
    // Priority: explicit override → CONTEXT_WINDOW env var → hardcoded lookup → default
    const envContextWindow = process.env['CONTEXT_WINDOW'];
    this.contextWindow =
      contextWindowOverride ??
      (envContextWindow ? parseInt(envContextWindow, 10) : undefined) ??
      this.resolveContextWindow(modelName);

    const thresholdPctEnv = process.env['CONTEXT_COMPACTION_THRESHOLD'];
    const highWaterPct = thresholdPctEnv ? parseFloat(thresholdPctEnv) : DEFAULT_HIGH_WATER_PCT;

    const clearThresholdPctEnv = process.env['CONTEXT_CLEAR_THRESHOLD'];
    const clearThresholdPct = clearThresholdPctEnv
      ? parseFloat(clearThresholdPctEnv)
      : DEFAULT_CLEAR_THRESHOLD_PCT;

    const maxTokensEnv = process.env['MAX_CONTEXT_TOKENS'];
    this.highWaterMark = maxTokensEnv
      ? parseInt(maxTokensEnv, 10)
      : Math.floor(this.contextWindow * highWaterPct);

    this.clearThreshold = Math.floor(this.contextWindow * clearThresholdPct);

    const strategyEnv = process.env['CONTEXT_COMPACTION_STRATEGY'] as
      | CompactionStrategy
      | undefined;
    this.strategy = strategyEnv === 'summarise' ? 'summarise' : 'sliding-window';

    const toolMaxEnv = process.env['TOOL_OUTPUT_MAX_TOKENS'];
    this.toolOutputMaxTokens = toolMaxEnv
      ? parseInt(toolMaxEnv, 10)
      : DEFAULT_TOOL_OUTPUT_MAX_TOKENS;

    const preserveEnv = process.env['PRESERVE_RECENT_MESSAGES'];
    this.preserveRecentMessages = preserveEnv
      ? parseInt(preserveEnv, 10)
      : DEFAULT_PRESERVE_RECENT_MESSAGES;

    const flushEnabledEnv = process.env['MEMORY_FLUSH_BEFORE_COMPACTION'];
    this.memoryFlushEnabled = flushEnabledEnv !== 'false';

    // Use cl100k_base encoding as a universal approximation for any model.
    // 5-10% error is acceptable per the spec.
    this.enc = getEncoding('cl100k_base');

    log(
      'info',
      `ContextManager init: model=${modelName} window=${this.contextWindow} highWater=${this.highWaterMark} strategy=${this.strategy}`
    );
  }

  /** Get the configured context window size. */
  getContextWindow(): number {
    return this.contextWindow;
  }

  /** Count tokens in a string or content blocks. */
  countTokens(content: string | import('./llmClient.js').MessageContentBlock[]): number {
    if (!content) return 0;
    if (typeof content === 'string') {
      return this.enc.encode(content).length;
    }
    let total = 0;
    for (const block of content) {
      if (block.type === 'text' && block.text) {
        total += this.enc.encode(block.text).length;
      } else if (block.type === 'image_url') {
        // Rough estimate for images: 1105 tokens for high detail, 85 for low.
        // OpenAI pricing: 85 tokens for low detail, 1105 for high detail (1024x1024).
        // We'll use 1105 as a safe upper bound for auto/high.
        total += block.image_url?.detail === 'low' ? 85 : 1105;
      }
    }
    return total;
  }

  /** Count tokens across all messages. */
  countMessageTokens(messages: ChatMessage[]): number {
    let total = 0;
    for (const msg of messages) {
      if (msg.tokens === undefined || typeof msg.content !== 'string') {
        msg.tokens = this.estimateMessageTokens(msg);
      }
      total += msg.tokens;
    }
    return total;
  }

  /** Estimate tokens for a single message. */
  estimateMessageTokens(msg: ChatMessage): number {
    // ~4 tokens per-message overhead (role + delimiters) per OpenAI docs
    let total = 4;
    total += this.countTokens(msg.content);
    if (msg.tool_calls) {
      for (const tc of msg.tool_calls) {
        total += this.countTokens(tc.function.name);
        total += this.countTokens(tc.function.arguments);
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

  /** Get the threshold for clearing old tool results. */
  getClearThreshold(): number {
    return this.clearThreshold;
  }

  /**
   * Replace tool results with a placeholder if they are not among the most
   * recent `preserveCount` tool results.
   */
  clearOldToolResults(messages: ChatMessage[], preserveCount: number = 3): number {
    const placeholder = '[cleared — re-read if needed]';
    let clearedCount = 0;

    // Find indices of all tool messages
    const toolIndices: number[] = [];
    for (let i = 0; i < messages.length; i++) {
      if (messages[i]!.role === 'tool') {
        toolIndices.push(i);
      }
    }

    // Determine which tool results to clear (all but the last `preserveCount`)
    const toClear = toolIndices.slice(0, Math.max(0, toolIndices.length - preserveCount));

    for (const index of toClear) {
      const msg = messages[index]!;
      if (msg.content !== placeholder) {
        msg.content = placeholder;
        msg.tokens = this.estimateMessageTokens(msg);
        clearedCount++;
      }
    }

    return clearedCount;
  }

  isMemoryFlushEnabled(): boolean {
    return this.memoryFlushEnabled;
  }

  /**
   * Compact the message history using the configured strategy.
   * Always preserves system messages and the N most recent messages verbatim.
   *
   * @param messages   Current message history (mutated in place).
   * @param llmClient  Optional LLM client for summarization.
   * @returns CompactionResult describing what happened.
   */
  async compact(messages: ChatMessage[], llmClient?: ILLMClient): Promise<CompactionResult> {
    return this.performCompaction(messages, this.highWaterMark, 'Context compacted', llmClient);
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
   * @param llmClient  Optional LLM client for summarization.
   */
  async aggressiveCompact(
    messages: ChatMessage[],
    llmClient?: ILLMClient
  ): Promise<CompactionResult> {
    const aggressiveTarget = Math.floor(this.contextWindow * AGGRESSIVE_COMPACT_PCT);
    return this.performCompaction(messages, aggressiveTarget, 'Aggressive compaction', llmClient);
  }

  /**
   * Retroactively truncate all tool result messages to the given token limit.
   * One-shot last resort for overflow recovery.
   *
   * @returns The number of tool messages that were truncated.
   */
  truncateAllToolResults(
    messages: ChatMessage[],
    maxTokens: number = DEFAULT_EMERGENCY_TOOL_TOKENS
  ): number {
    let truncatedCount = 0;
    for (const msg of messages) {
      if (
        msg.role === 'tool' &&
        typeof msg.content === 'string' &&
        this.countTokens(msg.content) > maxTokens
      ) {
        const notice = `\n\n[SERA: tool result retroactively truncated to ${maxTokens} tokens for overflow recovery]`;
        const noticeTokens = this.countTokens(notice);
        msg.content = this.truncateToFit(msg.content, maxTokens - noticeTokens) + notice;
        msg.tokens = undefined; // Force recalculation
        truncatedCount++;
      }
    }
    if (truncatedCount > 0) {
      log(
        'info',
        `ContextManager: retroactively truncated ${truncatedCount} tool result(s) to ${maxTokens} tokens`
      );
    }
    return truncatedCount;
  }

  /**
   * Returns how many tokens remain before hitting the high-water mark,
   * minus a reserve for the LLM's response. Clamped to 0.
   */
  getAvailableBudget(
    messages: ChatMessage[],
    responseReserve: number = DEFAULT_RESPONSE_RESERVE
  ): number {
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
    responseReserve: number = DEFAULT_RESPONSE_RESERVE
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
    // Basic heuristic for known model families (CONTEXT_WINDOW env var takes priority — see constructor)
    const lower = modelName.toLowerCase();
    if (lower.includes('claude')) return 200_000;
    if (lower.includes('gpt-4')) return 128_000;
    if (lower.includes('gemini')) return 1_000_000;
    return DEFAULT_CONTEXT_WINDOW;
  }

  /** Remove enrichment XML blocks from a message's content. */
  private stripEnrichment(content: string): string {
    let stripped = content;
    for (const pattern of ENRICHMENT_PATTERNS) {
      stripped = stripped.replace(pattern, '');
    }
    // Clean up leftover double newlines from removed blocks
    return stripped.replace(/\n{3,}/g, '\n\n').trim();
  }

  private truncateToFit(content: string, targetTokens: number): string {
    if (targetTokens <= 0) return '';
    let low = 0;
    let high = content.length;
    while (low < high) {
      const mid = Math.floor((low + high + 1) / 2);
      if (this.enc.encode(content.slice(0, mid)).length <= targetTokens) {
        low = mid;
      } else {
        high = mid - 1;
      }
    }
    return content.slice(0, low);
  }

  private async performCompaction(
    messages: ChatMessage[],
    targetTokens: number,
    label: string,
    llmClient?: ILLMClient
  ): Promise<CompactionResult> {
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
        strategy: this.strategy,
      };
    }

    let isFallback = false;
    if (this.strategy === 'summarise' && llmClient) {
      // Summarization is only for text. If there are images, skip.
      const hasImages = messages.some(
        (m) => Array.isArray(m.content) && m.content.some((b) => b.type === 'image_url')
      );
      if (hasImages) {
        log('info', 'ContextManager: images detected, skipping summarization strategy');
      } else {
        try {
          return await this.performSummarizeCompaction(
            messages,
            targetTokens,
            label,
            llmClient,
            tokensBefore
          );
        } catch (err) {
          log(
            'warn',
            `ContextManager: summarization failed, falling back to sliding-window: ${err}`
          );
          isFallback = true;
        }
      }
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
    if (
      droppedMessages.length === 0 &&
      nonSystemMessages.length > 1 &&
      tokensBefore >= targetTokens
    ) {
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
        this.countMessageTokens([...systemMessages, continuationMsg, ...nonSystemMessages]) >=
          targetTokens
      ) {
        droppedMessages.push(nonSystemMessages.shift()!);
        droppedCount++;
        continuationMsg = generateContinuation(droppedMessages);
      }

      // Final check: if dropping all didn't help (huge system prompt or single huge recent message),
      // keep 1 but it will be truncated later.

      // If still over, truncate the first retained message
      const total = this.countMessageTokens([
        ...systemMessages,
        continuationMsg,
        ...nonSystemMessages,
      ]);
      if (total > targetTokens && nonSystemMessages.length > 0) {
        const first = nonSystemMessages[0]!;
        if (typeof first.content !== 'string') {
          // Can't easily truncate multi-modal. Just drop it if it's the only one.
          nonSystemMessages.shift();
          return this.performCompaction(messages, targetTokens, label, llmClient);
        }
        const notice = '[SERA: message truncated due to context window overflow]';
        const noticeTokens = this.countTokens(notice);
        const otherTokens = this.countMessageTokens([
          ...systemMessages,
          continuationMsg,
          ...nonSystemMessages.slice(1),
        ]);
        const available = targetTokens - otherTokens;
        if (available > noticeTokens) {
          first.content =
            this.truncateToFit(first.content, available - noticeTokens) + '\n' + notice;
        } else {
          first.content = notice;
        }
        first.tokens = undefined; // Force recalculation
      }
      messages.splice(0, messages.length, ...systemMessages, continuationMsg, ...nonSystemMessages);
    } else {
      // No messages dropped, but might be over if a single message is huge
      const total = this.countMessageTokens([...systemMessages, ...nonSystemMessages]);
      if (total > targetTokens && nonSystemMessages.length > 0) {
        const first = nonSystemMessages[0]!;
        if (typeof first.content !== 'string') {
          nonSystemMessages.shift();
          return this.performCompaction(messages, targetTokens, label, llmClient);
        }
        const notice = '[SERA: message truncated due to context window overflow]';
        const noticeTokens = this.countTokens(notice);
        const otherTokens = this.countMessageTokens([
          ...systemMessages,
          ...nonSystemMessages.slice(1),
        ]);
        const available = targetTokens - otherTokens;
        if (available > noticeTokens) {
          first.content =
            this.truncateToFit(first.content, available - noticeTokens) + '\n' + notice;
        } else {
          first.content = notice;
        }
        first.tokens = undefined; // Force recalculation
      }
      messages.splice(0, messages.length, ...systemMessages, ...nonSystemMessages);
    }

    const tokensAfter = this.countMessageTokens(messages);
    const retainedCount = nonSystemMessages.length;
    const reflectMessage = `${label}${isFallback ? ' (fallback)' : ''}: dropped ${droppedCount} messages, retained ${retainedCount} non-system messages (${tokensBefore} → ${tokensAfter} tokens)`;

    log('info', `ContextManager: ${reflectMessage}`);
    return {
      droppedCount,
      retainedCount,
      tokensBefore,
      tokensAfter,
      reflectMessage,
      strategy: 'sliding-window',
      isFallback,
    };
  }

  private async performSummarizeCompaction(
    messages: ChatMessage[],
    targetTokens: number,
    label: string,
    llmClient: ILLMClient,
    tokensBefore: number
  ): Promise<CompactionResult> {
    const systemMessages = messages.filter((m) => m.role === 'system');
    const nonSystem = messages.filter((m) => m.role !== 'system');
    const recentK = Math.min(this.preserveRecentMessages, nonSystem.length);
    const oldest = nonSystem.slice(0, nonSystem.length - recentK);
    const recent = nonSystem.slice(nonSystem.length - recentK);

    if (oldest.length === 0) {
      // Nothing to summarize — trigger sliding window
      throw new Error('Nothing to summarize');
    }

    // Partition logic might need to drop more from "recent" if the summary
    // itself is expected to be large. But we'll start with standard K.

    const conversationText = oldest
      .map((m) => {
        const content =
          typeof m.content === 'string' ? this.stripEnrichment(m.content ?? '') : '[Media Content]';
        return `${m.role}: ${content}`;
      })
      .join('\n\n');

    const prompt = `Summarize the following conversation history, preserving:
- Key decisions and conclusions
- Important facts and data points mentioned
- Current task state and progress
- Any commitments or action items

Be concise but preserve all actionable information.

Conversation:
${conversationText}`;

    const compactionModel = process.env['CONTEXT_COMPACTION_MODEL'];

    const response = await llmClient.chat(
      [{ role: 'user', content: prompt }],
      undefined,
      0.3, // Low temperature for summarization
      undefined,
      undefined,
      compactionModel
    );

    const summary = response.content;
    const summaryUserMsg: ChatMessage = {
      role: 'user',
      content: `[Context Summary]\n${summary}`,
    };
    const summaryAckMsg: ChatMessage = {
      role: 'assistant',
      content: 'Understood. I have the summarized context and will continue from here.',
    };

    const newMessages = [...systemMessages, summaryUserMsg, summaryAckMsg, ...recent];

    // If still over target, fall back to sliding window or truncate further?
    // Let's try to fit.
    if (this.countMessageTokens(newMessages) > targetTokens) {
      // If summary + recent is still too big, we have to drop from recent or truncate summary.
      // For now, let's just let it be and let the next turn's compaction handle it if needed,
      // or fall back to sliding window if it's really bad.
    }

    messages.splice(0, messages.length, ...newMessages);

    const tokensAfter = this.countMessageTokens(messages);
    const droppedCount = oldest.length;
    const retainedCount = recent.length + 2; // Summary messages + recent
    const reflectMessage = `${label} (summarize): dropped ${droppedCount} messages, retained ${retainedCount} messages (${tokensBefore} → ${tokensAfter} tokens)`;

    log('info', `ContextManager: ${reflectMessage}`);
    return {
      droppedCount,
      retainedCount,
      tokensBefore,
      tokensAfter,
      reflectMessage,
      strategy: 'summarise',
    };
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

      const content =
        typeof msg.content === 'string'
          ? msg.content
          : msg.content.map((b) => (b.type === 'text' ? b.text : '')).join(' ');
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
        const text = typeof msg.content === 'string' ? msg.content : '[Media Content]';
        const truncated = text.trim().slice(0, 160).replace(/\s+/g, ' ');
        return `- ${msg.role}: ${truncated}`;
      })
      .join('\n');

    const summaryParts = [
      `Scope: ${counts['user']} user, ${counts['assistant']} assistant, ${counts['tool']} tool`,
      `Tools mentioned: ${Array.from(tools).join(', ') || 'none'}`,
      `Recent user requests:\n${last3UserRequests.map((r) => `- ${r}`).join('\n') || 'none'}`,
      `Pending work:\n${
        pendingWork
          .slice(-5)
          .map((p) => `- ${p}`)
          .join('\n') || 'none'
      }`,
      `Key files: ${Array.from(keyFiles).join(', ') || 'none'}`,
      `Current work: ${currentWork.slice(0, 500)}${currentWork.length > 500 ? '...' : ''}`,
      `Key timeline:\n${timeline}`,
    ];

    return summaryParts.join('\n\n');
  }
}
