import { describe, it, expect, vi, beforeEach } from 'vitest';
import { ContextCompactionService } from './ContextCompactionService.js';
import type { ChatMessage } from './LlmRouter.js';
import type { ProviderConfig } from './ProviderRegistry.js';

// ── Mocks ────────────────────────────────────────────────────────────────────

function mockRegistry(config: Partial<ProviderConfig> = {}) {
  return {
    resolve: vi.fn().mockReturnValue({
      modelName: 'test-model',
      api: 'openai-completions' as const,
      contextWindow: 1000,
      contextHighWaterMark: 0.8,
      contextStrategy: undefined,
      contextCompactionModel: undefined,
      ...config,
    }),
  } as unknown as import('./ProviderRegistry.js').ProviderRegistry;
}

function mockRouter(summaryText = 'Summary of the conversation.') {
  return {
    chatCompletion: vi.fn().mockResolvedValue({
      response: {
        id: 'cmp-1',
        object: 'chat.completion' as const,
        created: Date.now(),
        model: 'test-compaction-model',
        choices: [
          { index: 0, message: { role: 'assistant', content: summaryText }, finish_reason: 'stop' },
        ],
      },
      latencyMs: 100,
    }),
  } as unknown as import('./LlmRouter.js').LlmRouter;
}

function makeMessages(count: number, charsPerMessage = 100): ChatMessage[] {
  const msgs: ChatMessage[] = [{ role: 'system', content: 'You are a helpful assistant.' }];
  for (let i = 0; i < count; i++) {
    msgs.push({
      role: i % 2 === 0 ? 'user' : 'assistant',
      content: 'x'.repeat(charsPerMessage),
    });
  }
  return msgs;
}

// ── Tests ────────────────────────────────────────────────────────────────────

describe('ContextCompactionService', () => {
  let events: Array<{ stage: string; detail: Record<string, unknown> }>;
  let onEvent: (event: { stage: string; detail: Record<string, unknown> }) => void;

  beforeEach(() => {
    events = [];
    onEvent = (e) => events.push(e);
  });

  describe('compact()', () => {
    it('returns messages unchanged when under threshold', async () => {
      const registry = mockRegistry({ contextWindow: 100_000 });
      const router = mockRouter();
      const service = new ContextCompactionService(router, registry);

      const msgs = makeMessages(4, 50); // ~5 messages, very short
      const result = await service.compact(msgs, 'test-model', onEvent);

      expect(result).toBe(msgs); // Same reference — unchanged
      expect(events[0]!.stage).toBe('compaction.skipped');
    });

    it('skips compaction when model not in registry', async () => {
      const registry = {
        resolve: vi.fn().mockImplementation(() => {
          throw new Error('Model not found');
        }),
      } as unknown as import('./ProviderRegistry.js').ProviderRegistry;
      const router = mockRouter();
      const service = new ContextCompactionService(router, registry);

      const msgs = makeMessages(4);
      const result = await service.compact(msgs, 'unknown-model', onEvent);

      expect(result).toBe(msgs);
      expect(events[0]!.stage).toBe('compaction.skipped');
      expect(events[0]!.detail.reason).toBe('model not in registry');
    });

    it('uses sliding-window when no compaction model is configured', async () => {
      // contextWindow: 1000 tokens, highWaterMark: 0.80 → threshold: 800 tokens
      // 20 messages × 400 chars each = 8000 chars ≈ 2000 tokens (way over)
      const registry = mockRegistry({
        contextWindow: 1000,
        contextStrategy: 'sliding-window',
      });
      const router = mockRouter();
      const service = new ContextCompactionService(router, registry);

      const msgs = makeMessages(20, 400);
      const result = await service.compact(msgs, 'test-model', onEvent);

      // Should have dropped some messages
      expect(result.length).toBeLessThan(msgs.length);
      // System message preserved
      expect(result[0]!.role).toBe('system');
      // Recent messages preserved
      expect(result[result.length - 1]!.role).toBe('assistant');
      // Compaction event emitted
      const completed = events.find((e) => e.stage === 'compaction.completed');
      expect(completed).toBeDefined();
      expect(completed!.detail.strategy).toBe('sliding-window');
    });

    it('summarizes when strategy is summarize and compaction model configured', async () => {
      const registry = mockRegistry({
        contextWindow: 1000,
        contextStrategy: 'summarize',
        contextCompactionModel: 'fast-model',
      });
      const router = mockRouter('- User asked about weather\n- Assistant provided forecast');
      const service = new ContextCompactionService(router, registry);

      const msgs = makeMessages(20, 400);
      const result = await service.compact(msgs, 'test-model', onEvent);

      // Should have system + summary-user + summary-ack + recent K messages
      expect(result[0]!.role).toBe('system');
      expect(result[1]!.role).toBe('user');
      expect(result[1]!.content).toContain('[Previous conversation summary');
      expect(result[1]!.content).toContain('weather');
      expect(result[2]!.role).toBe('assistant');
      expect(result[2]!.content).toContain('context from the conversation summary');

      // LlmRouter.chatCompletion was called
      expect(router.chatCompletion).toHaveBeenCalledOnce();
      const call = (router.chatCompletion as ReturnType<typeof vi.fn>).mock.calls[0]!;
      expect(call[0].model).toBe('fast-model');

      // Events
      const summarizing = events.find((e) => e.stage === 'compaction.summarizing');
      expect(summarizing).toBeDefined();
      const completed = events.find((e) => e.stage === 'compaction.completed');
      expect(completed!.detail.strategy).toBe('summarize');
    });

    it('falls back to sliding-window when summarization fails', async () => {
      const registry = mockRegistry({
        contextWindow: 1000,
        contextStrategy: 'summarize',
        contextCompactionModel: 'fast-model',
      });
      const router = {
        chatCompletion: vi.fn().mockRejectedValue(new Error('Model unavailable')),
      } as unknown as import('./LlmRouter.js').LlmRouter;
      const service = new ContextCompactionService(router, registry);

      const msgs = makeMessages(20, 400);
      const result = await service.compact(msgs, 'test-model', onEvent);

      // Should still return compacted result via sliding-window
      expect(result.length).toBeLessThan(msgs.length);
      expect(result[0]!.role).toBe('system');

      // Fallback event
      const fallback = events.find((e) => e.stage === 'compaction.fallback');
      expect(fallback).toBeDefined();
      expect(fallback!.detail.reason).toContain('Model unavailable');
    });

    it('strips enrichment markers before summarization', async () => {
      const registry = mockRegistry({
        contextWindow: 200,
        contextStrategy: 'summarize',
        contextCompactionModel: 'fast-model',
      });
      const router = mockRouter('Summary');
      const service = new ContextCompactionService(router, registry);

      const msgs: ChatMessage[] = [
        {
          role: 'system',
          content:
            'Base prompt\n<memory>\n<block>secret data</block>\n</memory>\n<skills>tool defs</skills>',
        },
        { role: 'user', content: 'x'.repeat(400) },
        { role: 'assistant', content: 'y'.repeat(400) },
        { role: 'user', content: 'z'.repeat(400) },
        { role: 'assistant', content: 'w'.repeat(400) },
        { role: 'user', content: 'recent1' },
        { role: 'assistant', content: 'recent2' },
        { role: 'user', content: 'recent3' },
        { role: 'assistant', content: 'recent4' },
        { role: 'user', content: 'recent5' },
        { role: 'assistant', content: 'recent6' },
      ];

      await service.compact(msgs, 'test-model', onEvent);

      // Check that the summarization prompt doesn't contain enrichment
      const call = (router.chatCompletion as ReturnType<typeof vi.fn>).mock.calls[0]!;
      const promptContent = call[0].messages[0].content as string;
      expect(promptContent).not.toContain('<memory>');
      expect(promptContent).not.toContain('<skills>');
      expect(promptContent).not.toContain('secret data');
    });

    it('preserves recent K messages (default 6)', async () => {
      const registry = mockRegistry({
        contextWindow: 500,
        contextStrategy: 'summarize',
        contextCompactionModel: 'fast-model',
      });
      const router = mockRouter('Summary');
      const service = new ContextCompactionService(router, registry);

      // 12 non-system messages: oldest 6 summarized, recent 6 preserved
      const msgs: ChatMessage[] = [
        { role: 'system', content: 'sys' },
        ...Array.from({ length: 12 }, (_, i) => ({
          role: (i % 2 === 0 ? 'user' : 'assistant') as 'user' | 'assistant',
          content: `msg-${i}-${'x'.repeat(200)}`,
        })),
      ];

      const result = await service.compact(msgs, 'test-model', onEvent);

      // system + summary-user + summary-ack + 6 recent = 9
      expect(result.length).toBe(9);
      // Last 6 are the recent messages
      expect(result[3]!.content).toContain('msg-6');
      expect(result[8]!.content).toContain('msg-11');
    });

    it('handles edge case: only system message', async () => {
      const registry = mockRegistry({ contextWindow: 100_000 });
      const router = mockRouter();
      const service = new ContextCompactionService(router, registry);

      const msgs: ChatMessage[] = [{ role: 'system', content: 'sys' }];
      const result = await service.compact(msgs, 'test-model', onEvent);

      expect(result).toBe(msgs);
    });
  });
});
