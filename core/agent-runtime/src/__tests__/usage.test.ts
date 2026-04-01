import { describe, it, expect } from 'vitest';
import { UsageTracker, type TokenUsage } from '../usage.js';

describe('UsageTracker', () => {
  it('should start with zero usage', () => {
    const tracker = new UsageTracker();
    expect(tracker.turns()).toBe(0);
    expect(tracker.totalTokens()).toBe(0);
    expect(tracker.cumulativeUsage()).toEqual({
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationInputTokens: 0,
      cacheReadInputTokens: 0,
    });
  });

  it('should record usage correctly', () => {
    const tracker = new UsageTracker();
    const usage: TokenUsage = {
      inputTokens: 100,
      outputTokens: 50,
      cacheCreationInputTokens: 20,
      cacheReadInputTokens: 10,
    };

    tracker.record(usage);
    expect(tracker.turns()).toBe(1);
    expect(tracker.totalTokens()).toBe(180);
    expect(tracker.cumulativeUsage()).toEqual(usage);

    tracker.record(usage);
    expect(tracker.turns()).toBe(2);
    expect(tracker.totalTokens()).toBe(360);
    expect(tracker.cumulativeUsage()).toEqual({
      inputTokens: 200,
      outputTokens: 100,
      cacheCreationInputTokens: 40,
      cacheReadInputTokens: 20,
    });
  });

  it('should estimate cost for known models', () => {
    const tracker = new UsageTracker();
    // 1M input, 1M output, 1M cacheRead
    tracker.record({
      inputTokens: 1_000_000,
      outputTokens: 1_000_000,
      cacheCreationInputTokens: 0,
      cacheReadInputTokens: 1_000_000,
    });

    // GPT-4o: $2.50 (in) + $10.00 (out) + $1.25 (cacheRead) = $13.75
    expect(tracker.estimatedCostUsd('gpt-4o')).toBeCloseTo(13.75, 2);

    // Claude 3.5 Sonnet: $3.00 (in) + $15.00 (out) + $0.30 (cacheRead) = $18.30
    expect(tracker.estimatedCostUsd('claude-3-5-sonnet')).toBeCloseTo(18.30, 2);
  });

  it('should return 0 cost for unknown models', () => {
    const tracker = new UsageTracker();
    tracker.record({
      inputTokens: 1000,
      outputTokens: 1000,
      cacheCreationInputTokens: 0,
      cacheReadInputTokens: 0,
    });
    expect(tracker.estimatedCostUsd('local-model')).toBe(0);
  });

  it('should reconstruct from messages', () => {
    const messages = [
      { content: 'hi' },
      {
        content: 'hello',
        usage: {
          inputTokens: 10,
          outputTokens: 5,
          cacheCreationInputTokens: 0,
          cacheReadInputTokens: 0,
        },
      },
      {
        content: 'bye',
        usage: {
          inputTokens: 20,
          outputTokens: 10,
          cacheCreationInputTokens: 5,
          cacheReadInputTokens: 5,
        },
      },
    ];

    const tracker = UsageTracker.fromMessages(messages as any);
    expect(tracker.turns()).toBe(2);
    expect(tracker.cumulativeUsage()).toEqual({
      inputTokens: 30,
      outputTokens: 15,
      cacheCreationInputTokens: 5,
      cacheReadInputTokens: 5,
    });
  });
});
