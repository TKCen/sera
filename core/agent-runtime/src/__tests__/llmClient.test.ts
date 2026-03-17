import { describe, it, expect, vi, beforeEach } from 'vitest';
import axios, { AxiosError } from 'axios';
import { LLMClient, type ChatMessage } from '../llmClient.js';

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
    it('sends correct Authorization header', async () => {
      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');

      expect(mockCreate).toHaveBeenCalledWith(
        expect.objectContaining({
          headers: expect.objectContaining({
            Authorization: 'Bearer test-token',
          }),
        })
      );
    });

    it('parses OpenAI response with content', async () => {
      mockPost.mockResolvedValueOnce({
        data: {
          choices: [
            {
              message: {
                content: 'Hello world!',
              },
            },
          ],
        },
      });

      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const messages: ChatMessage[] = [{ role: 'user', content: 'Hi' }];

      const response = await client.chat(messages);
      expect(response.content).toBe('Hello world!');
      expect(response.toolCalls).toBeUndefined();
    });

    it('parses OpenAI response with tool_calls', async () => {
      mockPost.mockResolvedValueOnce({
        data: {
          choices: [
            {
              message: {
                content: null,
                tool_calls: [
                  {
                    id: 'call_abc',
                    type: 'function',
                    function: {
                      name: 'file-read',
                      arguments: '{"path":"test.txt"}',
                    },
                  },
                ],
              },
            },
          ],
        },
      });

      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const messages: ChatMessage[] = [{ role: 'user', content: 'Read a file' }];

      const response = await client.chat(messages);
      expect(response.content).toBe('');
      expect(response.toolCalls).toBeDefined();
      expect(response.toolCalls?.length).toBe(1);
      expect(response.toolCalls?.[0].id).toBe('call_abc');
      expect(response.toolCalls?.[0].function.name).toBe('file-read');
    });

    it('throws on 429 budget-exceeded response', async () => {
      const error = new AxiosError('Request failed with status code 429');
      error.response = {
        status: 429,
        data: { error: { message: 'budget exceeded' } },
        statusText: 'Too Many Requests',
        headers: {},
        config: {} as any,
      };
      mockPost.mockRejectedValueOnce(error);

      const client = new LLMClient('http://core:3000', 'test-token', 'gpt-4');
      const messages: ChatMessage[] = [{ role: 'user', content: 'Hi' }];

      await expect(client.chat(messages)).rejects.toThrow(/budget exceeded/);
    });
  });
});
