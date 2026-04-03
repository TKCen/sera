import { describe, it, expect, vi, beforeEach } from 'vitest';
import axios, { AxiosError } from 'axios';
import {
  LLMClient,
  BudgetExceededError,
  ProviderUnavailableError,
  ContextOverflowError,
  LLMTimeoutError,
} from '../llmClient.js';
import { Readable } from 'stream';

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
        })
      );
    });

    function createMockStream(events: any[]): Readable {
      const stream = new Readable({
        read() {},
      });
      for (const event of events) {
        stream.push(`data: ${JSON.stringify(event)}\n\n`);
      }
      stream.push('data: [DONE]\n\n');
      stream.push(null);
      return stream;
    }

    it('parses OpenAI response with content', async () => {
      const stream = createMockStream([
        { choices: [{ delta: { content: 'Hello ' } }] },
        { choices: [{ delta: { content: 'world!' } }] },
      ]);
      mockPost.mockResolvedValueOnce({ data: stream });

      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const response = await client.chat([{ role: 'user', content: 'Hi' }]);
      expect(response.content).toBe('Hello world!');
      expect(response.toolCalls).toBeUndefined();
    });

    it('parses OpenAI response with tool_calls', async () => {
      const stream = createMockStream([
        {
          choices: [
            {
              delta: {
                tool_calls: [{ index: 0, id: 'call_abc', function: { name: 'file-read' } }],
              },
            },
          ],
        },
        {
          choices: [{ delta: { tool_calls: [{ index: 0, function: { arguments: '{"path":' } }] } }],
        },
        {
          choices: [
            { delta: { tool_calls: [{ index: 0, function: { arguments: '"test.txt"}' } }] } },
          ],
        },
      ]);
      mockPost.mockResolvedValueOnce({ data: stream });

      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const response = await client.chat([{ role: 'user', content: 'Read a file' }]);
      expect(response.toolCalls).toHaveLength(1);
      expect(response.toolCalls![0]!.function.name).toBe('file-read');
      expect(JSON.parse(response.toolCalls![0]!.function.arguments)).toEqual({ path: 'test.txt' });
    });

    it('parses usage stats', async () => {
      const stream = createMockStream([
        {
          choices: [{ delta: { content: 'done' } }],
          usage: { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 },
        },
      ]);
      mockPost.mockResolvedValueOnce({ data: stream });

      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const response = await client.chat([{ role: 'user', content: 'Hi' }]);
      expect(response.usage?.promptTokens).toBe(10);
      expect(response.usage?.completionTokens).toBe(5);
    });

    it('parses usage-only chunks without choices', async () => {
      const stream = new Readable({
        read() {},
      });
      stream.push(
        `data: ${JSON.stringify({ usage: { prompt_tokens: 50, completion_tokens: 20, total_tokens: 70 } })}\n\n`
      );
      stream.push('data: [DONE]\n\n');
      stream.push(null);
      mockPost.mockResolvedValueOnce({ data: stream });

      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const response = await client.chat([{ role: 'user', content: 'Hi' }]);
      expect(response.usage?.totalTokens).toBe(70);
    });

    it('throws BudgetExceededError on HTTP 429', async () => {
      const error = new AxiosError('Request failed');
      error.response = {
        status: 429,
        data: { error: { message: 'budget exceeded' } },
        statusText: 'Too Many Requests',
        headers: {},
        config: {} as any,
      };
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(
        BudgetExceededError
      );
    });

    it('throws ProviderUnavailableError on HTTP 503', async () => {
      const error = new AxiosError('Service Unavailable');
      error.response = {
        status: 503,
        data: { error: { message: 'circuit open' } },
        statusText: 'Service Unavailable',
        headers: {},
        config: {} as any,
      };
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(
        ProviderUnavailableError
      );
    });

    it('uses LLM_TIMEOUT_MS env var for timeout', () => {
      process.env['LLM_TIMEOUT_MS'] = '60000';
      new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      expect(mockCreate).toHaveBeenCalledWith(expect.objectContaining({ timeout: 60000 }));
      delete process.env['LLM_TIMEOUT_MS'];
    });

    it('throws ContextOverflowError on HTTP 400 with context_length_exceeded code', async () => {
      const error = new AxiosError('Bad Request');
      error.response = {
        status: 400,
        data: { error: { code: 'context_length_exceeded', message: 'too long' } },
        statusText: 'Bad Request',
        headers: {},
        config: {} as any,
      };
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(
        ContextOverflowError
      );
    });

    it('throws ContextOverflowError on HTTP 400 with "maximum context length" in message', async () => {
      const error = new AxiosError('Bad Request');
      error.response = {
        status: 400,
        data: { error: { message: 'This model has a maximum context length of 128000 tokens' } },
        statusText: 'Bad Request',
        headers: {},
        config: {} as any,
      };
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(
        ContextOverflowError
      );
    });

    it('throws ContextOverflowError on HTTP 400 with "prompt is too long" message', async () => {
      const error = new AxiosError('Bad Request');
      error.response = {
        status: 400,
        data: { error: { message: 'prompt is too long: 200000 tokens > 128000' } },
        statusText: 'Bad Request',
        headers: {},
        config: {} as any,
      };
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(
        ContextOverflowError
      );
    });

    it('does NOT throw ContextOverflowError on HTTP 400 with unrelated message', async () => {
      const error = new AxiosError('Bad Request');
      error.response = {
        status: 400,
        data: { error: { message: 'invalid model name' } },
        statusText: 'Bad Request',
        headers: {},
        config: {} as any,
      };
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(Error);
      // Reset and verify it's NOT a ContextOverflowError
      mockPost.mockRejectedValueOnce(error);
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.not.toThrow(
        ContextOverflowError
      );
    });

    it('throws LLMTimeoutError on ECONNABORTED', async () => {
      const error = new AxiosError('timeout of 120000ms exceeded');
      error.code = 'ECONNABORTED';
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(LLMTimeoutError);
    });

    it('throws LLMTimeoutError on ETIMEDOUT', async () => {
      const error = new AxiosError('connect ETIMEDOUT');
      error.code = 'ETIMEDOUT';
      mockPost.mockRejectedValueOnce(error);
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      await expect(client.chat([{ role: 'user', content: 'Hi' }])).rejects.toThrow(LLMTimeoutError);
    });
  });
});
