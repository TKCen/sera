/**
 * Usage tracking with cache awareness and cost estimation.
 * Follows claw-code's usage tracker pattern.
 */

export interface TokenUsage {
  inputTokens: number;
  outputTokens: number;
  cacheCreationInputTokens: number;
  cacheReadInputTokens: number;
}

export class UsageTracker {
  private _turns: number = 0;
  private _cumulative: TokenUsage = {
    inputTokens: 0,
    outputTokens: 0,
    cacheCreationInputTokens: 0,
    cacheReadInputTokens: 0,
  };

  /** Record a single LLM response's token usage. */
  record(usage: TokenUsage): void {
    this._turns++;
    this._cumulative.inputTokens += usage.inputTokens || 0;
    this._cumulative.outputTokens += usage.outputTokens || 0;
    this._cumulative.cacheCreationInputTokens += usage.cacheCreationInputTokens || 0;
    this._cumulative.cacheReadInputTokens += usage.cacheReadInputTokens || 0;
  }

  /** Total number of LLM turns recorded. */
  turns(): number {
    return this._turns;
  }

  /** Cumulative token usage across all turns. */
  cumulativeUsage(): TokenUsage {
    return { ...this._cumulative };
  }

  /** Total tokens (sum of all input and output types). */
  totalTokens(): number {
    return (
      this._cumulative.inputTokens +
      this._cumulative.outputTokens +
      this._cumulative.cacheCreationInputTokens +
      this._cumulative.cacheReadInputTokens
    );
  }

  /**
   * Estimate the cost of cumulative usage in USD based on the model.
   * Uses placeholder pricing for common frontier models and $0 for local/unknown.
   */
  estimatedCostUsd(model: string): number {
    // Pricing per 1M tokens (Input, Output, CacheRead, CacheCreation)
    // Values are placeholders for standard frontier model rates.
    const pricing: Record<string, { input: number; output: number; cacheRead?: number; cacheCreation?: number }> = {
      'gpt-4o': { input: 2.50, output: 10.00, cacheRead: 1.25 },
      'claude-3-5-sonnet': { input: 3.00, output: 15.00, cacheRead: 0.30, cacheCreation: 3.75 },
      'deepseek-chat': { input: 0.14, output: 0.28, cacheRead: 0.014 },
      'deepseek-reasoner': { input: 0.55, output: 2.19, cacheRead: 0.14 },
    };

    const modelLower = model.toLowerCase();
    const key = Object.keys(pricing).find((k) => modelLower.includes(k));

    if (!key) {
      return 0;
    }

    const p = pricing[key]!;
    const u = this._cumulative;

    const inputCost = (u.inputTokens / 1_000_000) * p.input;
    const outputCost = (u.outputTokens / 1_000_000) * p.output;
    const cacheReadCost = (u.cacheReadInputTokens / 1_000_000) * (p.cacheRead ?? p.input);
    const cacheCreationCost = (u.cacheCreationInputTokens / 1_000_000) * (p.cacheCreation ?? p.input);

    return inputCost + outputCost + cacheReadCost + cacheCreationCost;
  }

  /** Reconstruct a UsageTracker from saved message history. */
  static fromMessages(messages: Array<{ usage?: TokenUsage }>): UsageTracker {
    const tracker = new UsageTracker();
    for (const msg of messages) {
      if (msg.usage) {
        tracker.record(msg.usage);
      }
    }
    return tracker;
  }
}
