import { describe, it, expect, vi, beforeEach } from 'vitest';
import axios, { AxiosError } from 'axios';
import { LLMClient, BudgetExceededError, ProviderUnavailableError, type ChatMessage } from '../llmClient.js';

vi.mock('axios');
const mockedAxios = vi.mocked(axios);

describe('LLMClient', () => {
  let mockPost: ReturnType<typeof vi.fn>;
  let mockCreate: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockPost = vi.fn();
    mockCreate = vi.fn().mockReturnValue({ post: mockPost });
    mockedAxios.create = mockCreate;
  });

  describe('chat()', () => {
    it('sends correct Authorization header', () => {
      new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      expect(mockCreate).toHaveBeenCalledWith(
        expect.objectContaining({
          headers: expect.objectContaining({ Authorization: 'Bearer test-token' }),
        }),
      );
    });

    it('parses OpenAI response with content', async () => {
      mockPost.mockResolvedValueOnce({
        data: { choices: [{ message: { content: 'Hello world!' } }] },
      });
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const response = await client.chat([{ role: 'user', content: 'Hi' }]);
      expect(response.content).toBe('Hello world!');
      expect(response.toolCalls).toBeUndefined();
    });

    it('parses OpenAI response with tool_calls', async () => {
      mockPost.mockResolvedValueOnce({
        data: {
          choices: [{
            message: {
              content: null,
              tool_calls: [{ id: 'call_abc', type: 'function', function: { name: 'file-read', arguments: '{"path":"test.txt"}' } }],
            },
          }],
        },
      });
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const response = await client.chat([{ role: 'user', content: 'Read a file' }]);
      expect(response.toolCalls).toHaveLength(1);
      expect(response.toolCalls![0]!.function.name).toBe('file-read');
    });

    it('parses usage stats', async () => {
      mockPost.mockResolvedValueOnce({
        data: {
          choices: [{ message: { content: 'done' } }],
          usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
        },
      });
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const response = await client.chat([{ role: 'user', content: 'Hi' }]);
      expect(response.usage?.promptTokens).toBe(10);
      expect(response.usage?.completionTokens).toBe(5);
    });

    it('throws BudgetExceededError on HTTP 429', async () => {
      const error = new AxiosError('Request failed');
      error.response = { status: 429, data: { error: { message: 'budget exceeded' } }, statusText: 'Too Many Requests', headers: {}, config: {} as any };
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(BudgetExceededError);
    });

    it('throws ProviderUnavailableError on HTTP 503', async () => {
      const error = new AxiosError('Service Unavailable');
      error.response = { status: 503, data: { error: { message: 'circuit open' } }, statusText: 'Service Unavailable', headers: {}, config: {} as any };
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(ProviderUnavailableError);
    });

    it('uses LLM_TIMEOUT_MS env var for timeout', () => {
      process.env['LLM_TIMEOUT_MS'] = '60000';
      new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      expect(mockCreate).toHaveBeenCalledWith(
        expect.objectContaining({ timeout: 60000 }),
      );
      delete process.env['LLM_TIMEOUT_MS'];
    });
  });
});
