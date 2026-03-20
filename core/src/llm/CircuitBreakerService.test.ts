import { describe, it, expect, vi, beforeEach } from 'vitest';
import { CircuitBreakerService, providerFromModel } from './CircuitBreakerService.js';
import type { LlmRouter } from './LlmRouter.js';

describe('providerFromModel', () => {
  it.each([
    ['gpt-4o-mini', 'openai'],
    ['gpt-3.5-turbo', 'openai'],
    ['o1-preview', 'openai'],
    ['claude-haiku', 'anthropic'],
    ['claude-sonnet-4-6', 'anthropic'],
    ['gemini-pro', 'google'],
    ['ollama-llama3', 'ollama'],
    ['lmstudio-default', 'lmstudio'],
    ['lmstudio-qwen', 'lmstudio'],
    ['custom-model-v2', 'custom'],
    ['mymodel', 'mymodel'],
  ])('maps %s → %s', (model, expected) => {
    expect(providerFromModel(model)).toBe(expected);
  });
});

describe('CircuitBreakerService', () => {
  let mockClient: LlmRouter;
  let service: CircuitBreakerService;

  const mockResponse = {
    response: {
      id: 'test',
      object: 'chat.completion' as const,
      created: 1234567890,
      model: 'gpt-4o-mini',
      choices: [
        {
          index: 0,
          message: { role: 'assistant', content: 'hi' },
          finish_reason: 'stop',
        },
      ],
      usage: { prompt_tokens: 5, completion_tokens: 5, total_tokens: 10 },
    },
    latencyMs: 50,
  };

  beforeEach(() => {
    mockClient = {
      chatCompletion: vi.fn().mockResolvedValue(mockResponse),
      chatCompletionStream: vi.fn(),
      listModels: vi.fn(),
      addModel: vi.fn(),
      deleteModel: vi.fn(),
      testModel: vi.fn(),
    } as unknown as LlmRouter;

    service = new CircuitBreakerService(mockClient);
  });

  describe('call()', () => {
    it('should forward a successful call to the client', async () => {
      const request = {
        model: 'gpt-4o-mini',
        messages: [{ role: 'user' as const, content: 'hello' }],
      };

      const result = await service.call(request, 'agent-001');

      expect(mockClient.chatCompletion).toHaveBeenCalledWith(
        request,
        'agent-001',
        expect.any(Number)
      );
      expect(result.response.object).toBe('chat.completion');
      expect(result.latencyMs).toBe(50);
    });

    it('should return state for all known circuits', async () => {
      const request = {
        model: 'gpt-4o-mini',
        messages: [{ role: 'user' as const, content: 'hello' }],
      };

      await service.call(request, 'agent-001');

      const states = service.getState();
      expect(states).toHaveLength(1);
      expect(states[0]!.provider).toBe('openai');
      expect(states[0]!.state).toBe('closed');
    });

    it('should return null for unknown provider state', () => {
      const state = service.getProviderState('unknown-provider');
      expect(state).toBeNull();
    });

    it('should track separate circuits per provider', async () => {
      const requests = [
        { model: 'gpt-4o-mini', messages: [{ role: 'user' as const, content: 'a' }] },
        { model: 'claude-haiku', messages: [{ role: 'user' as const, content: 'b' }] },
        { model: 'lmstudio-default', messages: [{ role: 'user' as const, content: 'c' }] },
      ];

      for (const req of requests) {
        await service.call(req, 'agent-001');
      }

      const states = service.getState();
      const providers = states.map((s) => s.provider).sort();
      expect(providers).toContain('openai');
      expect(providers).toContain('anthropic');
      expect(providers).toContain('lmstudio');
    });
  });
});
