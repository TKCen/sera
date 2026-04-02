import { describe, it, expect, vi, beforeEach, type Mocked } from 'vitest';
import { LlmRouter } from './LlmRouter.js';
import type { ProviderRegistry, ProviderConfig } from './ProviderRegistry.js';
import type { ChatCompletionRequest } from './LlmRouter.js';
import type { AssistantMessage } from '@mariozechner/pi-ai';

// Mock pi-ai
vi.mock('@mariozechner/pi-ai', () => ({
  // We'll mock the specific provider functions instead
}));

const mockStream = {
  result: vi.fn(),
  [Symbol.asyncIterator]: vi.fn(),
};

vi.mock('@mariozechner/pi-ai/openai-completions', () => ({
  streamOpenAICompletions: vi.fn(() => mockStream),
}));

vi.mock('@mariozechner/pi-ai/anthropic', () => ({
  streamAnthropic: vi.fn(() => mockStream),
}));

describe('LlmRouter', () => {
  let registry: Mocked<ProviderRegistry>;
  let router: LlmRouter;

  beforeEach(() => {
    vi.clearAllMocks();
    registry = {
      resolve: vi.fn(),
      listWithStatus: vi.fn(),
      register: vi.fn(),
      unregister: vi.fn(),
      save: vi.fn(),
    } as unknown as Mocked<ProviderRegistry>;
    router = new LlmRouter(registry);
  });

  describe('chatCompletion', () => {
    it('dispatches to openai-completions for openai api', async () => {
      const config: ProviderConfig = {
        modelName: 'gpt-4o',
        api: 'openai-completions',
        provider: 'openai',
      };
      registry.resolve.mockReturnValue(config);

      const mockResponse = {
        role: 'assistant',
        content: [{ type: 'text', text: 'Hello' }],
        usage: { input: 10, output: 5, totalTokens: 15 },
        stopReason: 'stop',
      };
      mockStream.result.mockResolvedValue(mockResponse as AssistantMessage);

      const request: ChatCompletionRequest = {
        model: 'gpt-4o',
        messages: [{ role: 'user', content: 'Hi' }],
      };

      const result = await router.chatCompletion(request, 'agent-1');
      const response = result.response;

      expect(registry.resolve).toHaveBeenCalledWith('gpt-4o');
      expect(response.choices[0]!.message.content).toBe('Hello');
      expect(response.usage?.total_tokens).toBe(15);
    });

    it('dispatches to anthropic-messages for anthropic api', async () => {
      const config: ProviderConfig = {
        modelName: 'claude-3',
        api: 'anthropic-messages',
        provider: 'anthropic',
      };
      registry.resolve.mockReturnValue(config);

      const mockResponse = {
        role: 'assistant',
        content: [{ type: 'text', text: 'Hello from Claude' }],
        usage: { input: 20, output: 10, totalTokens: 30 },
        stopReason: 'stop',
      };
      mockStream.result.mockResolvedValue(mockResponse as AssistantMessage);

      const request: ChatCompletionRequest = {
        model: 'claude-3',
        messages: [{ role: 'user', content: 'Hi' }],
      };

      const result = await router.chatCompletion(request, 'agent-1');
      const response = result.response;

      expect(response.choices[0]!.message.content).toBe('Hello from Claude');
    });

    it('handles tool calls in request and response', async () => {
      const config: ProviderConfig = {
        modelName: 'gpt-4o',
        api: 'openai-completions',
        provider: 'openai',
      };
      registry.resolve.mockReturnValue(config);

      const mockResponse = {
        role: 'assistant',
        content: [
          { type: 'text', text: 'Thinking...' },
          {
            type: 'toolCall',
            id: 'call_1',
            name: 'get_weather',
            arguments: { city: 'London' },
          },
        ],
        usage: { input: 50, output: 20, totalTokens: 70 },
        stopReason: 'toolUse',
      };
      mockStream.result.mockResolvedValue(mockResponse as AssistantMessage);

      const request: ChatCompletionRequest = {
        model: 'gpt-4o',
        messages: [{ role: 'user', content: 'Weather in London?' }],
        tools: [{ type: 'function', function: { name: 'get_weather', parameters: {} } }],
      };

      const result = await router.chatCompletion(request, 'agent-1');
      const response = result.response;

      expect(response.choices[0]!.message.tool_calls).toHaveLength(1);
      expect(response.choices[0]!.message.tool_calls?.[0]).toMatchObject({
        id: 'call_1',
        function: { name: 'get_weather', arguments: '{"city":"London"}' },
      });
      expect(response.choices[0]!.finish_reason).toBe('tool_calls');
    });
  });

  describe('testModel', () => {
    it('returns ok for successful ping', async () => {
      registry.resolve.mockReturnValue({
        modelName: 'test-model',
        api: 'openai-completions',
      });
      mockStream.result.mockResolvedValue({ usage: {} } as AssistantMessage);

      const result = await router.testModel('test-model');
      expect(result.ok).toBe(true);
    });

    it('returns error for failed ping', async () => {
      registry.resolve.mockReturnValue({
        modelName: 'test-model',
        api: 'openai-completions',
      });
      mockStream.result.mockRejectedValue(new Error('Connection failed'));

      const result = await router.testModel('test-model');
      expect(result.ok).toBe(false);
      expect(result.error).toBe('Connection failed');
    });
  });

  describe('listModels', () => {
    it('returns models from registry without sensitive info', async () => {
      registry.listWithStatus.mockReturnValue([
        {
          modelName: 'm1',
          api: 'openai-completions',
          authStatus: 'configured',
          apiKey: '***',
        } as unknown as ProviderConfig & { authStatus: 'configured' },
      ]);

      const models = await router.listModels();
      expect(models).toHaveLength(1);
      expect(models[0]!.modelName).toBe('m1');
      expect((models[0] as unknown as Record<string, unknown>)['apiKey']).toBeUndefined();
    });
  });
});
